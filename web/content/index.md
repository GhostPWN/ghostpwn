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

- provider support for OpenAI, Anthropic, Google, GitHub Copilot, Codex, and local Ollama
- retained image input for vision-capable models
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via the OS keychain, with a local state-file fallback
- streaming assistant output with auto-scroll and transcript controls

## Project

GhostPWN is the annual project for the 3rd year of the Bachelor in Cybersecurity at [ESGI Paris](https://esgi.fr/).

Contributions are welcome. Open an issue before submitting a pull request. GhostPWN is available under the [MIT license](https://github.com/GhostPWN/ghostpwn/blob/main/LICENSE).

## Explore

- [Installation](/docs/getting-started/installation)
- [Commands](/docs/getting-started/commands)
- [Image input](/docs/getting-started/image-input)
- [Configuration](/docs/configuration)
- [Architecture](/docs/architecture)
