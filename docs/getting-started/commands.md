# Commands

- `/help` shows all commands
- `/model` opens the keyboard model selector (`Left`/`Right` provider, `Up`/`Down` model, `Enter` switch, `c` connect, `d` disconnect, `Esc` close)
- Connecting a provider from `/model` makes it active immediately and remembers the provider/model for the next launch
- GitHub Copilot uses device authorization from the Copilot tab and fetches its model list after successful authorization
- Codex uses ChatGPT/Codex OAuth from the Codex tab, opening a browser first and falling back to device authorization when browser login is unavailable
- Non-Copilot cloud providers accept pasted API keys from their `/model` tab
- Disconnecting a provider from `/model` removes its key from the OS keychain when available
- `/clear` resets in-memory conversation
- `/quit` or `/exit` exits the TUI
- `Ctrl+C` exits immediately
- Status bar shows streaming state and live/manual scroll position

## Scroll controls

Transcript scrolling responds to the mouse wheel plus `Up`, `Down`, `PgUp`, `PgDn`, `Home`, and `End`.

## Tools

Local tools available to the agent loop:

`listSkills`, `searchSkills`, `readSkill`, `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `fileInfo`, `generateDiff`, `writeFile`, `editFile`, `multiEdit`, `applyPatch`, `webFetch`, `webSearch`.

Claude Code, Codex, and OpenCode-compatible tool aliases are provided for common read/write/edit/shell/search operations.
