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
- `/paste-image` queues an image from the system clipboard
- `/clear-images` removes queued clipboard images
- `/clear` resets the in-memory conversation and removes queued clipboard images
- `/quit` or `/exit` exits the TUI
- `Ctrl`+`C` exits immediately
- `Ctrl`+`V` queues clipboard bitmap data as PNG, or pastes normalized text when no bitmap is available
- Status bar shows streaming state and live/manual scroll position

See [Image input](/docs/getting-started/image-input) for path syntax, limits, retention, and provider requirements.

## Scroll controls

Transcript scrolling responds to the mouse wheel plus `↑`, `↓`, `PgUp`, `PgDn`, `Home`, and `End`.

## Tools

Local tools available to the agent loop:

`listSkills`, `searchSkills`, `readSkill`, `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `auditDependencies`, `fileInfo`, `generateDiff`, `writeFile`, `editFile`, `multiEdit`, `applyPatch`, `webFetch`, `webSearch`.

Model-requested commands and file mutations pause until you approve them with `y` or deny them with `n`/`Enter`/`Esc`. Shell commands are clearly labeled as unsandboxed; the workspace controls their starting directory, not their operating-system access.

Claude Code, Codex, and OpenCode-compatible tool aliases are provided for common read/write/edit/shell/search operations.

Fenced `diff` blocks in assistant responses are rendered as highlighted diffs.
