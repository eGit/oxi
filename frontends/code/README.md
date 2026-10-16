# oxi-code

A terminal interface for oxi that looks and behaves like Claude Code. It is a client of
`oxi headless` (the shared oxi daemon), so its chats are the same ones the oxi app and the
Sublime package show, and turns started elsewhere stream here live.

Built with [Ink](https://github.com/vadimdemedes/ink), the React terminal renderer Claude Code
uses: finished messages go to the terminal's scrollback, only the live part is redrawn.

## Run

Needs Node 22+ and an oxi build with `oxi headless`.

```bash
cd frontends/code && npm install && npm run build && npm link
```

Then, in any project folder:

```bash
oxi-code
```

`oxi-code -c` continues the last chat in the folder, `oxi-code -r` picks one, and
`oxi-code "fix the tests"` sends a first prompt. oxi is found through `--oxi <path>`,
`$OXI_PATH`, `PATH`, `~/.local/bin/oxi` and `/Applications/oxi.app`.

## What is there

- Welcome box, `>` prompt between rules, `? for shortcuts` footer, a statusline with
  provider, model, effort and plan usage (5h / 7d)
- `⏺` messages and tool calls (`Bash(…)`, `Read(…)`, `Update(…)`, `Search(…)`, MCP tools)
  with `⎿` results, diffs with line numbers on red/green rows, `Update Todos` checklists,
  `✻ Thinking…`, the `✻ Verb… (12s · ↓ 1.2k tokens · esc to interrupt)` spinner with the
  current todo
- Permission prompts: Yes / Yes, and don't ask again (Bash prefix or tool, saved in oxi) / No,
  with the proposed edit's diff
- Modes on shift+tab: default, `⏵⏵ accept edits on`, `⏸ plan mode on`
- `/` commands with completion: `/clear`, `/resume`, `/model`, `/effort`, `/plan`,
  `/context`, `/usage`, `/status`, `/rename`, `/help`, `/exit`
- `@` file completion, `!` bash mode (output goes to the next prompt), ctrl+v pastes an
  image, long pastes fold into `[Pasted text #1 +40 lines]`, prompt history per folder
- esc interrupts, esc esc clears the input, ctrl+r verbose transcript, ctrl+t todos,
  `\`+Enter or option+Enter for a new line (shift+Enter in kitty, WezTerm and Ghostty),
  ctrl+c twice to exit
