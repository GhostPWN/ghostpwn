<table align="center">
<tr>
<td valign="top" width="200">
<img src="ghostpwn-logo.svg" alt="GhostPwn" width="200">
</td>
<td align="right">
<pre>
<span style="color: #9b59b6;">
  ▄████  ██░ ██  ▒█████    ██████ ▄▄▄█████▓ ██▓███   █     █░ ███▄    █ 
 ██▒ ▀█▒▓██░ ██▒▒██▒  ██▒▒██    ▒ ▓  ██▒ ▓▒▓██░  ██▒▓█░ █ ░█░ ██ ▀█   █ 
▒██░▄▄▄░▒██▀▀██░▒██░  ██▒░ ▓██▄   ▒ ▓██░ ▒░▓██░ ██▓▒▒█░ █ ░█ ▓██  ▀█ ██▒
░▓█  ██▓░▓█ ░██ ▒██   ██░  ▒   ██▒░ ▓██▓ ░ ▒██▄█▓▒ ▒░█░ █ ░█ ▓██▒  ▐▌██▒
░▒▓███▀▒░▓█▒░██▓░ ████▓▒░▒██████▒▒  ▒██▒ ░ ▒██▒ ░  ░░░██▒██▓ ▒██░   ▓██░
 ░▒   ▒  ▒ ░░▒░▒░ ▒░▒░▒░ ▒ ▒▓▒ ▒ ░  ▒ ░░   ▒▓▒░ ░  ░░ ▓░▒ ▒  ░ ▒░   ▒ ▒ 
  ░   ░  ▒ ░▒░ ░  ░ ▒ ▒░ ░ ░▒  ░ ░    ░    ░▒ ░       ▒ ░ ░  ░ ░░   ░ ▒░
░ ░   ░  ░  ░░ ░░ ░ ░ ▒  ░  ░  ░    ░      ░░         ░   ░     ░   ░ ░ 
      ░  ░  ░  ░    ░ ░        ░                        ░             ░ 
</span>
</pre>
</td>
</tr>
</table>

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

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    GhostPWN Agent                   │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌─────────┐  ┌──────┐  │
│  │  Recon   │→ │ Analysis │→ │ Exploit │→ │Report│  │
│  │  Agent   │  │  Agent   │  │  Agent  │  │Agent │  │
│  └──────────┘  └──────────┘  └─────────┘  └──────┘  │
│       │              │             │           │    │
│       └──────────────┴─────────────┴───────────┘    │
│                        │                            │
│              ┌─────────▼─────────┐                  │
│              │  Vercel AI SDK    │                  │
│              │  (Orchestrator)   │                  │
│              └─────────┬─────────┘                  │
│         ┌──────────────┼──────────────┐             │
│         ▼              ▼              ▼             │
│    ┌─────────┐   ┌──────────┐   ┌────────┐          │
│    │ OpenAI  │   │Anthropic │   │ Other  │          │
│    └─────────┘   └──────────┘   └────────┘          │
│                                                     │
│  ┌──────────────────┐  ┌────────────────────────┐   │
│  │      SQLite      │  │  OpenTUI Terminal UI   │   │
│  └──────────────────┘  └────────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │    HTTP Clients: Playwright · Selenium ·     │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

**Pipeline:** Recon → Analysis → Exploit → Report

Each agent operates autonomously within its phase but requires **human approval** before exploit execution.

---

## Tech Stack

| Layer            | Technology                                  |
|:-----------------|:--------------------------------------------|
| Runtime          | Bun                                         |
| Language         | TypeScript (strict, no `any`)               |
| Framework        | React (OpenTUI template)                    |
| LLM Orchestration| Vercel AI SDK                               |  
| Terminal UI      | OpenTUI (React-based TUI)                   |
| Database         | SQLite                                      |
| HTTP Clients     | Playwright, Selenium                        |

---

## Core Deliverables

1. **Agent Orchestration Framework** — Multi-agent pipeline powered by Vercel AI SDK with phase-based task delegation.
2. **OpenTUI Terminal Interface** — Real-time state sync and interactive terminal UI for monitoring agent progress.
3. **Multi-Provider LLM Integration** — Comparative execution across OpenAI, Anthropic, and local models.
4. **Human-in-the-Loop Workflow** — Mandatory human validation before exploit execution.
5. **SQLite Knowledge Base** — Persistent storage for reconnaissance data, vulnerability fingerprints, and session history.
6. **Academic Documentation** — Publication-ready analysis and methodology documentation.

---

## Key Constraints

- **No Docker/Temporal dependencies** — Self-contained, single-process architecture.
- **Lightweight** — Minimal external dependencies; runs on a single machine.
- **Research-grade** — Code quality and documentation suitable for academic publication.
- **Ethical boundaries** — Designed for authorized testing against known vulnerable applications (OWASP Juice Shop, DVWA).

---

## Evaluation Targets

| Target               | Type           | Purpose                          |
|:---------------------|:---------------|:---------------------------------|
| OWASP Juice Shop     | Intentionally vulnerable | Grey-box web app testing  |
| DVWA                 | Intentionally vulnerable | Baseline vulnerability coverage |

---

## Research Goals

- **Comparative LLM Analysis** — Benchmark Claude, GPT, and Gemini on vulnerability detection, exploit reasoning, and report generation.
- **Autonomous Agent Evaluation** — Measure end-to-end pipeline effectiveness across different provider configurations.

---

## Getting Started

```bash
# Clone
git clone https://github.com/<your-org>/ghostpwn.git
cd ghostpwn

# Install dependencies
bun install

# Run
bun run start
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Contributing

Contributions are welcome. Please open an issue before submitting a PR to discuss the proposed change.

---

<p align="center">
  <sub>Built for academic research in offensive security.</sub>
</p>
