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

**GhostPWN** is a Rust-based terminal assistant for offensive security research. It uses a `ratatui` + `crossterm` TUI, streams responses from multiple LLM providers, and constrains filesystem tools to a configured workspace boundary.

The current code base focuses on:
- provider support for OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via `.env` and OS keychain fallback
- streaming assistant output with auto-scroll and transcript controls

---

#### Features

- `ratatui` + `crossterm` terminal interface
- Provider adapters for OpenAI, Anthropic, Google, GitHub Copilot, and Ollama
- Native streaming support across provider adapters
- GitHub Copilot OAuth with automatic model discovery
- Keyboard model selector via `/model`
- JSON-first agent loop with tool-calling
- Local tools: `listSkills`, `searchSkills`, `readSkill`, `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `fileInfo`, `generateDiff`, `writeFile`, `editFile`, `multiEdit`, `applyPatch`, `todoRead`, `todoWrite`, `webFetch`, `webSearch`
- Local skills loaded from `src/skills/*/SKILL.md` with automatic skill search/read guidance in theë system prompt
- Claude Code, Codex, and OpenCode-compatible tool aliases for common read/write/edit/shell/search operations
- Diff rendering for fenced `diff` blocks in assistant output
- Workspace boundary enforcement for filesystem tools
- Shell commands run from the configured workspace but are not an OS-level sandbox
- Persistent secrets via `.env` and OS keychain
- Transcript scroll controls: moëuse wheel + `Up`, `Down`, `PgUp`, `PgDn`, `Home`, `End`

## Setup

```bash
cargo run
```

## Commands

- `/help` shows all commands
- `/model` opens the keyboard model selector (`Left`/`Right` provider, `Up`/`Down` model, `Enter` switch, `Esc` close)
- `/connect` shows provider connection status
- `/connect <provider> <api_key>` connects and persists key to `.env` and keychain when available
- `/connect github` or `/copilot` starts GitHub Copilot device authorization
- GitHub Copilot model list is fetched automatically after successful device authorization
- `/connect ollama [model]` switches to local Ollama without an API key
- `/disconnect <provider>` disconnects and removes key from `.env` and keychain when available
- `/clear` resets in-memory conversation
- `/quit` or `/exit` exits the TUI
- `Ctrl+C` exits immediately
- Status bar shows streaming state and live/manual scroll position

## Configuration

- `GHOSTPWN_PROVIDER`: `anthropic` | `openai` | `google` | `copilot` | `ollama`
- `GHOSTPWN_MODEL`: optional model override for the selected provider
- Provider key env vars:
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `GOOGLE_GENERATIVE_AI_API_KEY`
  - `GITHUB_COPILOT_TOKEN`
- `GHOSTPWN_WORKSPACE`: optional root path used as the filesystem-tool boundary and command working directory
- `GHOSTPWN_ENV_FILE`: optional `.env` path override for secret persistence
- API keys are loaded from environment first, then from `.env`, and finally from OS keychain entries under service `ghostpwn-rust`
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
- `runCommand` uses the configured workspace as its current directory and enforces a bounded timeout; do not treat it as a security sandbox.
- `webSearch` uses DuckDuckGo HTML results and may fail if the page structure changes or rate limits requests.
- Provider keys can come from environment variables, `.env`, or the OS keychain.
- GitHub Copilot is supported through `/connect github` or `/copilot`.

---

## License

MIT License. See LICENSE for details.

---

## Contributing

Contributions are welcome. Please open an issue before submitting a PR to discuss the proposed change.

---

<p align="center">
  <sub>Built for academic research in offensive security.</sub>
</p>
