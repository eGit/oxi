#!/bin/sh
# Installs the latest oxi release on macOS (Apple Silicon) and Linux (x86_64).
#
#   curl -fsSL https://maziluiosif.github.io/oxi/install.sh | sh
#
# Files fetched with curl never get macOS's quarantine attribute, so the
# ad-hoc signed app opens without the `xattr` step a browser download needs.
#
# Environment overrides:
#   OXI_VERSION      release tag to install (e.g. v1.8.0); defaults to the latest
#   OXI_BIN_DIR      where the `oxi` command goes; defaults to ~/.local/bin
#   OXI_APP_DIR      macOS only: where oxi.app goes; defaults to /Applications
#                    when writable, ~/Applications otherwise
#   OXI_SUBLIME      1 always installs the Sublime Text package, 0 never does;
#                    by default it is installed when Sublime Text is

set -eu

REPO="maziluiosif/oxi"

say() { printf 'oxi: %s\n' "$*"; }
fail() { printf 'oxi: error: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || fail "'$1' is required but was not found"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Checks a file in $tmp against the release's SHA256SUMS.
verify() {
    expected=$(awk -v f="$1" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
    [ -n "$expected" ] || fail "$1 is missing from SHA256SUMS"
    actual=$(sha256_of "$tmp/$1")
    [ "$expected" = "$actual" ] || fail "checksum mismatch for $1 (expected $expected, got $actual)"
}

# The Sublime Text package (oxi's chat inside Sublime), next to the app.
install_sublime() {
    want="${OXI_SUBLIME:-}"
    case "$want" in 0 | no | false) return 0 ;; esac
    if [ "$os" = "Darwin" ]; then
        data="$HOME/Library/Application Support/Sublime Text"
    else
        data="${XDG_CONFIG_HOME:-$HOME/.config}/sublime-text"
    fi
    if [ ! -d "$data" ]; then
        case "$want" in 1 | yes | true) ;; *) return 0 ;; esac
    fi
    if [ -e "$data/Packages/Oxi" ]; then
        say "kept the Sublime Text package already in $data/Packages/Oxi"
        return 0
    fi
    package="oxi-sublime.sublime-package"
    if ! curl -fsSL --proto '=https' --tlsv1.2 -o "$tmp/$package" "$base/$package" 2>/dev/null; then
        say "this release has no Sublime Text package; skipped it"
        return 0
    fi
    verify "$package"
    mkdir -p "$data/Installed Packages"
    mv "$tmp/$package" "$data/Installed Packages/Oxi.sublime-package"
    say "installed the Sublime Text package; in Sublime run \`Oxi: Toggle Chat Panel\`"
}

# Linux application menu entry (KDE, GNOME, …). oxi writes the same file on start
# (src/desktop_entry.rs), so installs updated in-app get it too; keep the two in sync.
install_desktop_entry() {
    data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
    entry="$data_home/applications/oxi.desktop"
    # A user who deleted the entry after oxi created it keeps it deleted.
    if [ ! -e "$entry" ] && [ -e "$data_home/oxi/desktop-entry-installed" ]; then
        return 0
    fi
    if [ -e "$entry" ] && ! grep -q '^X-Oxi-Managed=true$' "$entry"; then
        return 0
    fi
    exec_path="$bin_dir/oxi"
    case "$exec_path" in
        *[!A-Za-z0-9/._+-]*) exec_path="\"$exec_path\"" ;;
    esac
    mkdir -p "$data_home/applications" "$data_home/icons/hicolor/256x256/apps"
    cat >"$entry" <<ENTRY
[Desktop Entry]
Type=Application
Name=oxi
GenericName=AI Coding Agent
Comment=Chat with coding agents, edit and review code
Exec=$exec_path
Icon=oxi
Terminal=false
Categories=Development;IDE;
StartupWMClass=oxi
X-Oxi-Managed=true
ENTRY
    # Best effort: oxi also writes the icon on its first start.
    curl -fsSL --proto '=https' --tlsv1.2 -o "$data_home/icons/hicolor/256x256/apps/oxi.png" \
        "https://raw.githubusercontent.com/$REPO/master/assets/windows/app-icon-256.png" 2>/dev/null || true
    say "added oxi to the application menu"
}

main() {
    need curl
    need tar
    command -v sha256sum >/dev/null 2>&1 || need shasum

    os=$(uname -s)
    arch=$(uname -m)
    case "$os/$arch" in
        Darwin/arm64) asset="oxi-macos-arm64.tar.gz" ;;
        Linux/x86_64 | Linux/amd64) asset="oxi-linux-x86_64.tar.gz" ;;
        Darwin/*) fail "prebuilt macOS builds are Apple Silicon only; build from source on $arch: https://github.com/$REPO#build-and-run-from-source" ;;
        *) fail "no prebuilt build for $os/$arch; build from source: https://github.com/$REPO#build-and-run-from-source" ;;
    esac

    version="${OXI_VERSION:-}"
    if [ -n "$version" ]; then
        case "$version" in v*) ;; *) version="v$version" ;; esac
        base="https://github.com/$REPO/releases/download/$version"
    else
        base="https://github.com/$REPO/releases/latest/download"
    fi

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "downloading $asset${version:+ ($version)}"
    curl -fL --proto '=https' --tlsv1.2 --progress-bar -o "$tmp/$asset" "$base/$asset" \
        || fail "download failed: $base/$asset"
    curl -fsSL --proto '=https' --tlsv1.2 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" \
        || fail "download failed: $base/SHA256SUMS"

    verify "$asset"
    say "checksum verified"

    mkdir -p "$tmp/extract"
    tar -xzf "$tmp/$asset" -C "$tmp/extract"

    bin_dir="${OXI_BIN_DIR:-$HOME/.local/bin}"
    mkdir -p "$bin_dir"

    if [ "$os" = "Darwin" ]; then
        app_dir="${OXI_APP_DIR:-}"
        if [ -z "$app_dir" ]; then
            if [ -w /Applications ]; then app_dir="/Applications"; else app_dir="$HOME/Applications"; fi
        fi
        mkdir -p "$app_dir"
        app="$app_dir/oxi.app"
        rm -rf "$app"
        mv "$tmp/extract/oxi.app" "$app"
        # Not needed for a curl download, but clears the flag if the app came
        # from somewhere that set it (e.g. a proxy or an earlier browser install).
        xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
        ln -sf "$app/Contents/MacOS/oxi" "$bin_dir/oxi"
        say "installed $app"
    else
        install -m 755 "$tmp/extract/oxi" "$bin_dir/oxi"
        install_desktop_entry
    fi
    say "installed the oxi command at $bin_dir/oxi"
    install_sublime

    case ":$PATH:" in
        *":$bin_dir:"*) ;;
        *)
            say "$bin_dir is not on your PATH; add this line to your shell profile:"
            printf '\n    export PATH="%s:$PATH"\n\n' "$bin_dir"
            ;;
    esac

    if [ "$os" = "Darwin" ]; then
        say "done. Open oxi from Launchpad, or run \`oxi\` in a project directory."
    else
        say "done. Open oxi from your application menu, or run \`oxi\` in a project directory."
    fi
}

# Everything runs from main() so a truncated download cannot execute half a script.
main "$@"
