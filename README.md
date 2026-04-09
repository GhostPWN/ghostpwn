# GhostPWN Rust

Rust rewrite of GhostPWN with a terminal-first architecture, multi-provider LLM support, and local tool execution.

## Features

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

- `/clear` resets in-memory conversation
- `/model` shows current provider/model
- `/quit` exits the TUI
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
