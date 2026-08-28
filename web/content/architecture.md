# Architecture

- `src/main.rs`: bootstrap and dependency wiring
- `src/agent.rs`: orchestration loop, tool-execution cycle, and provider/model switching
- `src/providers/`: model adapters by vendor, including Copilot OAuth support
- `src/skills.rs`: optional local skill discovery, search, and read support
- `src/tools/mod.rs`: built-in local tool implementations with workspace safety checks
- `src/ui/mod.rs`: `ratatui` terminal app and command handling
- `src/config.rs`: environment-based configuration and provider defaults
- `src/secrets.rs`: OS keychain and local JSON state-file persistence
- `src/models.rs`: shared data models and events

## Notes

- The runtime expects model responses as JSON envelopes.
- When local skills are configured, the system prompt requires `searchSkills`/`readSkill` for matching specialized workflows before the agent proceeds.
- Assistant text is streamed from provider responses and incrementally rendered in the TUI.
- Model-requested commands and file mutations pause for explicit user approval.
- Filesystem tools reject paths outside the configured workspace root.
- `runCommand` uses the configured workspace as its current directory, runs through PowerShell on Windows and `sh` on Unix/macOS, and enforces a bounded timeout; do not treat it as a security sandbox.
- `webSearch` uses DuckDuckGo HTML results and may fail if the page structure changes or rate limits requests.
- Provider keys can come from environment variables, the OS keychain, or the local state-file fallback.
