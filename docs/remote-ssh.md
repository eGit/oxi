# Remote workspaces over SSH

The desktop app can open a folder on another machine and run its agents there.
The host owns the project files, agent processes and saved chats. The client shows
the conversation, approvals, editor, search, Git panel and an SSH terminal.

Use the same oxi build on the client and host. To use the host's agents from a
phone, see [oxi server](server.md).

## Prepare the host

Install oxi and the agent CLI you want to use on the host. Sign in to that agent
there, and configure any API providers in the host's oxi settings.

On macOS, from a terminal in the host's logged-in desktop session, run:

```sh
/path/to/oxi server install
/path/to/oxi server status
```

For an app bundle the binary is `/Applications/oxi.app/Contents/MacOS/oxi`.
For a source build it is `target/release/oxi` after `cargo build --release --locked`.
Keep that binary at a stable location: the service points to it.

Installation creates the `dev.oxi.server` LaunchAgent and a
`~/.local/bin/oxi-server` link. The agent runs at login and restarts the daemon
when needed. Keep the host's user session logged in. After moving the binary,
run `server install` again. To remove the service and link, run `oxi server uninstall`.
This leaves settings and chats in place.

On Linux, `oxi server install` sets up the `oxi-server.service` systemd user
service; on Windows it starts the server at sign-in. The desktop app can do the
same from **Settings → Server & phone**. See [oxi server](server.md).

## Connect from the desktop app

1. Check that the client can log in without an interactive SSH prompt:

   ```sh
   ssh -o BatchMode=yes manus-mac-mini true
   ```

   Use your own host alias or `user@host`. Complete the initial SSH host-key check
   in your terminal and load your key into ssh-agent if necessary.
2. Open **File → Connect to Remote Host…**, or use the server icon above the
   chat list. The command palette also has **Remote: Open a Folder on an SSH Host**.
3. Enter the host, click **Connect**, browse to a project folder and open it.
   Host aliases from `~/.ssh/config` appear as suggestions.
4. Open or create a chat and choose an agent installed on the host.

The workspace row shows its folder and host. Remote workspaces are restored
after restarting the client. If the SSH connection drops, the client retries;
turns already accepted by the daemon continue on the host. Reconnecting loads
the server's current transcript and pending approval.

## Credentials

The managed macOS server copies oxi's stored credentials from the keychain on
its first start. Allow that initial access on the host if macOS asks. It then
uses `server-secrets.json` in oxi's config folder, with owner-only permissions,
so replacing the oxi binary does not repeatedly block it at a keychain prompt.
On macOS that folder is normally `~/Library/Application Support/oxi`.

This is a separate credential copy. Keys changed in the desktop app on the same
machine are written to it too, and the server picks them up. Agent CLIs such as
Claude Code retain their own sign-in storage; sign in on the host.

## Troubleshooting

- **SSH login fails:** check the same host with the command above. The workspace
  connection uses system SSH with `BatchMode=yes`, keys and ssh-agent.
- **oxi is not installed:** check `~/.local/bin/oxi-server` on the host and run
  `oxi server install` from the intended binary.
- **Server is too old:** install the same build on both machines and reinstall
  the host service. The connection checks for desktop protocol support.
- **macOS daemon does not start:** check `oxi server status` in the host's desktop
  session. The LaunchAgent needs that user session. Diagnostics are in `oxi.log`
  and `server.log` in oxi's config folder.

## Development validation

The ignored GUI review uses a disposable remote Git repository with a committed
`README.md`, an uncommitted change to it, and `src/main.rs`. It sends two short
requests to Claude Code, saves the README, and exercises the remote panels:

```sh
OXI_GALLERY=/tmp/oxi-remote-gallery \
OXI_REMOTE_REVIEW=ssh://your-host/absolute/path/to/disposable-project \
cargo test --release --locked --lib render_gallery -- --ignored --nocapture
```

Use a disposable project: the review edits its README and restores the fixture
content afterwards. Claude Code must already be signed in on the host.
To use another configured agent, set `OXI_REMOTE_REVIEW_PROVIDER` to its provider
slug, for example `codexacp` for Codex.
Set `OXI_REMOTE_PANELS_ONLY=1` to check files, search, Git and the terminal
without sending requests to an agent, for example when its quota is exhausted.
