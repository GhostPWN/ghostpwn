<p align="center">
  <img src="logo.svg" alt="GhostPWN Logo" width="200">
</p>

<h1 align="center">GhostPWN</h1>

<p align="center">
  <strong>Autonomous Penetration Testing Agent</strong><br>
  <em>Multi-provider LLM support · Human-in-the-loop · Lightweight architecture</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/runtime-Rust-000000?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/lang-2024%20Edition-dea584?style=flat-square" alt="Rust 2024 Edition">
  <img src="https://img.shields.io/badge/ui-ratatui%20%2B%20crossterm-4a5568?style=flat-square" alt="ratatui + crossterm">
  <img src="https://img.shields.io/badge/providers-OpenAI%20%7C%20Anthropic%20%7C%20Google%20%7C%20Copilot%20%7C%20Ollama-7c3aed?style=flat-square" alt="Providers">
  <img src="https://img.shields.io/badge/license-MIT-7c3aed?style=flat-square" alt="License">
</p>

---

## Overview

GhostPWN is a Rust terminal assistant for offensive security research. It uses a `ratatui` + `crossterm` TUI, streams responses from multiple LLM providers, and constrains filesystem tools to a configured workspace boundary.

The current code base focuses on:
- provider support for OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via OS keychain fallback
- streaming assistant output with auto-scroll and transcript controls

---

#### Features

- `ratatui` + `crossterm` terminal interface
- Provider adapters for OpenAI, Anthropic, Google, GitHub Copilot, and Ollama
- Native streaming support across provider adapters
- GitHub Copilot OAuth with automatic model discovery
- Codex OAuth with ChatGPT/Codex OAuth browser login
- Keyboard model selector via `/model`
- JSON-first agent loop with tool-calling
- Local tools: `listSkills`, `searchSkills`, `readSkill`, `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `fileInfo`, `generateDiff`, `writeFile`, `editFile`, `multiEdit`, `applyPatch`, `todoRead`, `todoWrite`, `webFetch`, `webSearch`
- Local skills loaded from `src/skills/*/SKILL.md` with automatic skill search/read guidance in the system prompt
- Claude Code, Codex, and OpenCode-compatible tool aliases for common read/write/edit/shell/search operations
- Diff rendering for fenced `diff` blocks in assistant output
- Workspace boundary enforcement for filesystem tools
- Shell commands run from the configured workspace using `sh` on Unix/macOS and PowerShell on Windows, but are not an OS-level sandbox
- Persistent secrets via OS keychain
- Transcript scroll controls: mouse wheel + `Up`, `Down`, `PgUp`, `PgDn`, `Home`, `End`

## Install

### macOS

```bash
brew install GhostPWN/tap/ghostpwn
ghostpwn
```

The Homebrew formula lives in `Formula/ghostpwn.rb`.

### Linux

Install Rust and common native build dependencies, then install GhostPWN with Cargo:

```bash
sudo apt-get install -y build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

For non-Debian distributions, install the equivalent C compiler/toolchain and `pkg-config` packages before running `cargo install`.

### Windows

Install Rust, then build or install GhostPWN with Cargo:

```powershell
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

Windows CI builds also publish a `ghostpwn-windows-x86_64.zip` artifact containing `ghostpwn.exe`.

## Development

```bash
cargo run
```

## Commands

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

## Configuration

- `GHOSTPWN_PROVIDER`: optional startup provider override (`anthropic` | `openai` | `google` | `copilot` | `codex` | `ollama`)
- `GHOSTPWN_MODEL`: optional startup model override for the selected provider
- Provider key env vars:
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `GOOGLE_GENERATIVE_AI_API_KEY`
  - `GITHUB_COPILOT_TOKEN`
  - `CODEX_OAUTH_TOKEN` (serialized Codex OAuth credentials; normally managed by `/model`)
- `GHOSTPWN_WORKSPACE`: optional root path used as the filesystem-tool boundary and command working directory
- `GHOSTPWN_STATE_FILE`: optional local state-file override
- Local state fallback defaults to `%APPDATA%\ghostpwn\state.json` on Windows, `$XDG_CONFIG_HOME/ghostpwn/state.json` on Unix/macOS, then the user's home config path
- API keys are loaded from environment first, then OS keychain entries under service `ghostpwn-rust`, then the local state-file fallback
- The latest provider/model selection is stored as `latest_provider` and `latest_model` in the OS keychain and local state file
- Ollama uses `http://localhost:11434` and does not require an API key

## Architecture

- `src/main.rs`: bootstrap and dependency wiring
- `src/agent.rs`: orchestration loop, tool-execution cycle, and provider/model switching
- `src/providers/`: model adapters by vendor, including Copilot OAuth support
- `src/skills.rs`: local skill discovery, search, and read support for `src/skills`
- `src/tools/mod.rs`: built-in local tool implementations with workspace safety checks
- `src/ui/mod.rs`: `ratatui` terminal app and command handling
- `src/config.rs`: environment-based configuration and provider defaults
- `src/secrets.rs`: `.env` and keychain persistence helpers
- `src/models.rs`: shared data models and events

## Notes

- The runtime expects model responses as JSON envelopes.
- The system prompt requires `searchSkills`/`readSkill` for matching specialized workflows before the agent proceeds.
- Assistant text is streamed from provider responses and incrementally rendered in the TUI.
- Filesystem tools reject paths outside the configured workspace root.
- `runCommand` uses the configured workspace as its current directory, runs through PowerShell on Windows and `sh` on Unix/macOS, and enforces a bounded timeout; do not treat it as a security sandbox.
- `webSearch` uses DuckDuckGo HTML results and may fail if the page structure changes or rate limits requests.
- Provider keys can come from environment variables, the OS keychain, or the local state-file fallback.
- GitHub Copilot is supported through the Copilot tab in `/model`.

---

## License

MIT License. See LICENSE for details.

---

## Contributing

Contributions are welcome. Open an issue before submitting a PR to discuss the proposed change.

---

<p align="center">
  <sub>Built for academic research in offensive security.</sub>
</p>
