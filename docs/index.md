---
hide:
  - navigation
  - toc
---

<div class="gp-hero" markdown>

<img class="gp-logo" src="https://raw.githubusercontent.com/GhostPWN/.github/main/profile/logo.svg" alt="GhostPWN logo">

# GhostPWN

<p class="gp-tagline">Autonomous penetration testing agent. A Rust TUI that streams from multiple LLM providers and runs local tools inside a workspace boundary.</p>

<div class="gp-cta" markdown>
[Get started](getting-started/installation.md){ .md-button .md-button--primary }
[Commands](getting-started/commands.md){ .md-button }
[View on GitHub](https://github.com/GhostPWN/ghostpwn){ .md-button }
</div>

<div class="gp-terminal">
  <span class="gp-terminal__dots"><i></i><i></i><i></i></span>
  <code class="gp-terminal__cmd"><span class="gp-prompt">$</span> brew install GhostPWN/tap/ghostpwn</code>
</div>

</div>

!!! warning "Usage boundary"
    GhostPWN is built for academic research in offensive security. `runCommand` is **not** an OS-level sandbox. Use it only on targets you are allowed to access.

## Overview

GhostPWN is a Rust terminal assistant for offensive security research. It uses a `ratatui` + `crossterm` TUI, streams responses from multiple LLM providers, and constrains filesystem tools to a configured workspace boundary.

The current code base focuses on:

- provider support for OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama
- in-session provider/model switching
- local tools for reading files, listing directories, searching, and running commands
- persistent API key storage via OS keychain fallback
- streaming assistant output with auto-scroll and transcript controls

## Features

<div class="grid cards" markdown>

-   :material-console:{ .lg .middle } Terminal interface

    ---

    A `ratatui` + `crossterm` TUI with streaming output, auto-scroll, and transcript controls.

    [:octicons-arrow-right-24: Commands](getting-started/commands.md)

-   :material-swap-horizontal:{ .lg .middle } Multi-provider

    ---

    OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama, with in-session model switching.

    [:octicons-arrow-right-24: Configuration](configuration.md)

-   :material-key-chain:{ .lg .middle } Secure key storage

    ---

    Persistent API keys via the OS keychain, with environment and local state-file fallbacks.

    [:octicons-arrow-right-24: Configuration](configuration.md)

-   :material-tools:{ .lg .middle } Local tools

    ---

    Read, list, search, diff, edit, and run commands, all bounded to a configured workspace root.

    [:octicons-arrow-right-24: Architecture](architecture.md)

-   :material-shield-account:{ .lg .middle } OAuth providers

    ---

    GitHub Copilot device authorization and Codex ChatGPT/Codex OAuth browser login.

    [:octicons-arrow-right-24: Installation](getting-started/installation.md)

-   :material-sitemap:{ .lg .middle } Clear architecture

    ---

    A JSON-first agent loop, vendor provider adapters, and workspace-safe tool implementations.

    [:octicons-arrow-right-24: Architecture](architecture.md)

</div>
