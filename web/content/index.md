# GhostPWN

Autonomous penetration testing agent. A Rust TUI that streams from multiple LLM providers and runs local tools inside a workspace boundary.

## Install

```bash
brew install GhostPWN/tap/ghostpwn
ghostpwn
```

## Overview

GhostPWN is a Rust terminal assistant for offensive security research. It uses a `ratatui` + `crossterm` TUI, streams responses from multiple LLM providers, and constrains filesystem tools to a configured workspace boundary.

The current code base focuses on:

- provider support for OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via OS keychain fallback
- streaming assistant output with auto-scroll and transcript controls

## Explore

- [Installation](/docs/getting-started/installation)
- [Commands](/docs/getting-started/commands)
- [Configuration](/docs/configuration)
- [Architecture](/docs/architecture)
