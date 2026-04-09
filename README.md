<p align="center">
  <img src="logo.svg" alt="GhostPwn" width="200">
</p>

<h1 align="center">GhostPWN</h1>

<p align="center">
  <strong>Autonomous Web Penetration Testing Agent</strong><br>
  <em>Multi-provider LLM support · Human-in-the-loop · Lightweight architecture</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/runtime-Rust-000000?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/lang-2024%20Edition-dea584?style=flat-square" alt="Rust 2024 Edition">
  <img src="https://img.shields.io/badge/ui-ratatui%20%2B%20crossterm-4a5568?style=flat-square" alt="ratatui + crossterm">
  <img src="https://img.shields.io/badge/providers-OpenAI%20%7C%20Anthropic%20%7C%20Google%20%7C%20Copilot-7c3aed?style=flat-square" alt="Providers">
  <img src="https://img.shields.io/badge/license-MIT-7c3aed?style=flat-square" alt="License">
</p>

---

## Overview

**GhostPWN** is a Rust-based terminal assistant for offensive security research. It uses a `ratatui` + `crossterm` TUI, streams responses from multiple LLM providers, and keeps command execution constrained to a configured workspace boundary.

The current code base focuses on:
- provider support for OpenAI, Anthropic, Google, and GitHub Copilot
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via `.env` and OS keychain fallback
- streaming assistant output with auto-scroll and transcript controls

---

#### Features

- `ratatui` + `crossterm` terminal interface
- Provider adapters for OpenAI, Anthropic, Google, and GitHub Copilot
- Native streaming support across provider adapters
- JSON-first agent loop with tool-calling
- Local tools: `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `fileInfo`
- Workspace boundary enforcement for filesystem and shell tools
- Persistent secrets via `.env` and OS keychain
- Transcript scroll controls: mouse wheel + `Up`, `Down`, `PgUp`, `PgDn`, `Home`, `End`

## Setup

```bash
cp .env.example .env
# fill API keys and provider
cargo run
```

## Commands

- `/help` shows all commands
- `/model` shows active provider/model
- `/models` lists providers and suggested models
- `/models <provider> [model]` switches active provider/model in-session
- `/connect` shows provider connection status
- `/connect <provider> <api_key>` connects and persists key to `.env` and keychain when available
- `/disconnect <provider>` disconnects and removes key from `.env` and keychain when available
- `/copilot` starts GitHub Copilot device authorization
- `/clear` resets in-memory conversation
- `/quit` or `/exit` exits the TUI
- `Ctrl+C` exits immediately
- Status bar includes `AUTO-SCROLL ON/OFF` indicator

## Configuration

- `GHOSTPWN_PROVIDER`: `anthropic` | `openai` | `google` | `copilot`
- `GHOSTPWN_MODEL`: optional model override for the selected provider
- Provider key env vars:
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `GOOGLE_GENERATIVE_AI_API_KEY`
  - `GITHUB_COPILOT_TOKEN`
- `GHOSTPWN_WORKSPACE`: optional root path used by tools as a hard safety boundary
- `GHOSTPWN_ENV_FILE`: optional `.env` path override for secret persistence
- API keys are loaded from environment first, then from `.env`, and finally from OS keychain entries under service `ghostpwn-rust`

## Architecture

- `src/main.rs`: bootstrap and dependency wiring
- `src/agent.rs`: orchestration loop, tool-execution cycle, and provider/model switching
- `src/providers/`: model adapters by vendor, including Copilot OAuth support
- `src/tools/mod.rs`: built-in local tool implementations with workspace safety checks
- `src/ui/mod.rs`: `ratatui` terminal app and command handling
- `src/config.rs`: environment-based configuration and provider defaults
- `src/secrets.rs`: `.env` and keychain persistence helpers
- `src/models.rs`: shared data models and events

## Notes

- The runtime expects model responses as JSON envelopes.
- Assistant text is streamed from provider responses and incrementally rendered in the TUI.
- Tool command execution is constrained to the configured workspace root.
- Provider keys can come from environment variables, `.env`, or the OS keychain.
- GitHub Copilot is supported through a separate `/copilot` device-flow command.

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
