<p align="center">
  <img src="logo.svg" alt="GhostPwn" width="200">
</p>

<h1 align="center">GhostPWN</h1>

<p align="center">
  <strong>Autonomous Web Penetration Testing Agent</strong><br>
  <em>Multi-provider LLM support · Human-in-the-loop · Lightweight architecture</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/runtime-Bun-f472b6?style=flat-square" alt="Bun">
  <img src="https://img.shields.io/badge/lang-TypeScript-3178c6?style=flat-square" alt="TypeScript">
  <img src="https://img.shields.io/badge/ui-OpenTUI-a855f7?style=flat-square" alt="OpenTUI">
  <img src="https://img.shields.io/badge/license-MIT-7c3aed?style=flat-square" alt="License">
  <img src="https://img.shields.io/badge/status-In%20Development-facc15?style=flat-square" alt="Status">
</p>

---

## Overview

**GhostPWN** is an autonomous web penetration testing agent designed for academic research in offensive security. It orchestrates multiple LLM providers to perform grey-box web application testing through a multi-agent pipeline, with human oversight at every critical decision point.

The core research contribution is **comparative analysis of LLM providers** (Claude, GPT, Gemini) on offensive security tasks — evaluating reasoning quality, vulnerability detection accuracy, and exploit generation across providers.

---

#### Features

- ratatui + crossterm terminal interface
- Provider adapters for OpenAI, Anthropic, and Google
- Native provider streaming (SSE/chunk streaming) across all 3 adapters
- JSON-first agent loop with tool-calling
- Local tools: `readFile`, `listDirectory`, `searchFiles`, `grep`, `runCommand`, `fileInfo`
- Workspace boundary enforcement for filesystem and shell tools
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
- `/connect <provider> <api_key>` connects a key in-memory for this session
- `/clear` resets in-memory conversation
- `/quit` or `/exit` exits the TUI
- `Ctrl+C` exits immediately
- Status bar includes `AUTO-SCROLL ON/OFF` indicator

## Configuration

- `GHOSTPWN_PROVIDER`: `anthropic` | `openai` | `google`
- `GHOSTPWN_MODEL`: optional model override for selected provider
- Provider key env vars:
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `GOOGLE_GENERATIVE_AI_API_KEY`
- `GHOSTPWN_WORKSPACE`: optional root path used by tools as a hard safety boundary

## Architecture

- `src/main.rs`: bootstrap and dependency wiring
- `src/agent.rs`: orchestration loop and tool-execution cycle
- `src/providers/`: model adapters by vendor
- `src/tools/mod.rs`: built-in local tool implementations
- `src/ui/mod.rs`: ratatui terminal app
- `src/config.rs`: environment-based configuration
- `src/models.rs`: shared data models and events

## Notes

- The runtime expects model responses as JSON envelopes.
- Assistant text is streamed from provider responses and incrementally rendered in TUI.
- Tool command execution is constrained to the workspace root.
- Keys passed via `/connect` are kept in memory only (not persisted to `.env`).

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
