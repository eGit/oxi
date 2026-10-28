# Bundled fonts

Oxi embeds every font it renders with; nothing is installed or downloaded at runtime. egui's own
default fonts are disabled (`default_fonts` is off for release builds), since these faces cover
all of their glyphs. An installed font picked in Settings → Appearance goes first; the bundled
faces stay behind it as fallbacks.

| File | Role | License |
| --- | --- | --- |
| `Inter-Regular.ttf` (4.1) | Default interface and chat face | `Inter-OFL.txt` |
| `JetBrainsMono-Regular.ttf` (2.304) | Default code, diff and terminal face | `JetBrainsMono-OFL.txt` |
| `IBMPlexSans-Regular.ttf`, `IBMPlexSans-SemiBold.ttf` (3.005) | Selectable interface face (Zed's); semibold sets bold prose | `IBMPlexSans-OFL.txt` |
| `Lilex-Regular.ttf` (2.601) | Selectable code face (Zed's) | `Lilex-OFL.txt` |
| `NotoSans-Regular.ttf` | Selectable interface face; Latin/Greek/Cyrillic fallback | `NotoScripts-OFL.txt` |
| `UbuntuMono-R.ttf` | Selectable code face; monospace fallback | `UbuntuMono-UFL.txt` |
| `NotoEmoji-Regular.ttf` | Emoji, monochrome (no color or distinct skin tones) | `NotoScripts-OFL.txt` |
| `NotoSans{Arabic,Hebrew,Devanagari,Armenian,Georgian}-Regular.ttf` | Script fallbacks | `NotoScripts-OFL.txt` |
| `NotoSans{Math,Symbols,Symbols2}-Regular.ttf` | Math, arrows, technical symbols, braille, shapes | `NotoScripts-OFL.txt` |
| `SymbolsNerdFontMono-Regular.ttf` | Tool-pill icons, only in the `icons` family | `SymbolsNerdFont-LICENSE.txt` |

Sources: [Inter](https://github.com/rsms/inter/releases/tag/v4.1),
[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono/tree/v2.304/fonts/ttf),
[IBM Plex Sans and Lilex](https://github.com/zed-industries/zed/tree/main/assets/fonts) (as Zed ships them),
[Noto fonts](https://github.com/notofonts/noto-fonts/tree/main/hinted/ttf),
[Nerd Fonts](https://github.com/ryanoasis/nerd-fonts). All licenses are embedded and readable in
Settings → About → Bundled font licenses.

Apple Symbols was removed on 2026-10-14: it is Apple's proprietary font and may not be
redistributed. Noto Math and Symbols replace its coverage, except Runic, Ogham, Deseret, Aegean
numbers, musical notation and seven scan-line symbols (U+23B7–U+23BD).

## Rendering notes

- Selectable transcript text routes emoji graphemes (keycaps, ZWJ sequences, flags) to the
  `emoji` family so they shape as one glyph; text and selection offsets are unchanged.
- Japanese, Chinese and Korean come from the system, not from this folder: a bundled CJK face
  would add ~16 MiB to every download. At startup a background thread picks the fewest installed
  faces covering kana, Han and Hangul (Hiragino Sans + Apple SD Gothic Neo on macOS, Yu Gothic /
  Microsoft YaHei / Malgun Gothic on Windows, Noto/Source Han Sans CJK on Linux) and memory-maps
  them, so only rendered glyph pages are resident. Without such a face CJK shows as boxes.
- egui 0.35 has no full bidi layout: mixed right-to-left and left-to-right lines may be ordered
  incorrectly even though every glyph renders.
- Visual check: `cargo test --locked render_multilingual_font_review --lib -- --ignored` writes
  `/tmp/oxi-multilingual-font-review.png` (needs a GPU adapter).

## SHA-256

```text
975dcda37d80f038dcd143c22e33ca2d97a0cc5a929aace1c749153b0fe1afa5  IBMPlexSans-Regular.ttf
a20caf8286023a6a7a85e40b1d2a4ae9fc3e3b1f9eda8f4c542dd4986af67bb1  IBMPlexSans-SemiBold.ttf
40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82  Inter-Regular.ttf
a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f  JetBrainsMono-Regular.ttf
8a467ad5a1941dc7c9cebaee34e3ed9364a73b48393034e363e4b528bd711d04  Lilex-Regular.ttf
de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551  NotoEmoji-Regular.ttf
b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5  NotoSans-Regular.ttf
ceea25b464a656dc3b26849bab9356740401af62aedf1bfa8b7f0d9b75925b1b  NotoSansArabic-Regular.ttf
c3332abfe298018517d7f5b687a9c0f5c92f163ea9258f23934eaa7a9378f40e  NotoSansArmenian-Regular.ttf
385e78e6359a9d88a0f243d53b1209d7548361ba2194e2b9ec779bcaa7e8949d  NotoSansDevanagari-Regular.ttf
b36ab61cbdd820ffd32f957f704f5bf0c53655e820b7a64001091058919a11a6  NotoSansGeorgian-Regular.ttf
a7fa16fffb27bedb060a0866267c29e9859aeb9c21cc33f5b3aaf6eb062eca85  NotoSansHebrew-Regular.ttf
80b61fd613d3519197e64fff6f7e71fdc7f3e6526440ea4115b554ef7fd59af7  NotoSansMath-Regular.ttf
8f02f31959bbdf6061547a188248e13f84dc5fdd940326ec494675f453f072bb  NotoSansSymbols-Regular.ttf
630846d528dbe4c4981370a4d0a9475a1fd1491a129bb411f8e157cdb5de13c6  NotoSansSymbols2-Regular.ttf
f0f624d9b474bea1662cf7e862d44aebe1ae1f6c7f9cb7a0ca5d0e5ac9561c60  SymbolsNerdFontMono-Regular.ttf
b35dd9d2131d5d83a9b87fe9ad22c6288fa3d17688d43302c14da29812417d63  UbuntuMono-R.ttf
```
