# Commands

- `/help` shows all commands
- `/model` opens the keyboard model selector (`←`/`→` provider, `↑`/`↓` model, `Enter` switch, `c` connect, `d` disconnect, `Esc` close)
- `/audit` runs a read-only security audit of the workspace; use `/audit <path or focus>` to narrow it
- `/audit --fix [path or focus]` additionally applies scoped file fixes after individual approval
- Audit mode blocks shell commands and general web access; Rust dependency checks query OSV.dev only after approval
- Type a slash-command prefix and press `Tab` to autocomplete it
- Connecting a provider from `/model` makes it active immediately and remembers the provider/model for the next launch
- GitHub Copilot uses device authorization from the Copilot tab and fetches its model list after successful authorization
- Codex uses ChatGPT/Codex OAuth from the Codex tab, opening a browser first and falling back to device authorization when browser login is unavailable
- Non-Copilot cloud providers accept pasted API keys from their `/model` tab
- Disconnecting a provider from `/model` removes its key from the OS keychain when available
- `/clear` resets in-memory conversation
- `/quit` or `/exit` exits the TUI
- `Ctrl`+`C` exits immediately
- Status bar shows streaming state and live/manual scroll position

## Scroll controls

Transcript scrolling responds to the mouse wheel plus `↑`, `↓`, `PgUp`, `PgDn`, `Home`, and `End`.

## Tools

Local tools available to the agent loop:

`listSkills`, `searchSkills`, `readSkill`, `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `auditDependencies`, `fileInfo`, `generateDiff`, `writeFile`, `editFile`, `multiEdit`, `applyPatch`, `webFetch`, `webSearch`.

Model-requested commands and file mutations pause until you approve them with `y` or deny them with `n`/`Enter`/`Esc`. Shell commands are clearly labeled as unsandboxed; the workspace controls their starting directory, not their operating-system access.

Claude Code, Codex, and OpenCode-compatible tool aliases are provided for common read/write/edit/shell/search operations.

Fenced `diff` blocks in assistant responses are rendered as highlighted diffs.
