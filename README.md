<p align="center">
  <img src="https://raw.githubusercontent.com/GhostPWN/.github/main/profile/logo.svg" alt="GhostPWN" width="200">
</p>

<h1 align="center">GhostPWN</h1>

<p align="center">
  <strong>Autonomous penetration testing agent</strong><br>
  A Rust TUI with multi-provider LLM support, human-in-the-loop tool execution, and workspace boundaries.
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img alt="Rust 2024" src="https://shieldcn.dev/badge/Rust-2024-dea584.svg?variant=secondary&amp;logo=rust&amp;logoColor=171717"></a>
  <a href="https://ratatui.rs/"><img alt="Ratatui 0.30" src="https://shieldcn.dev/badge/Ratatui-0.30-171717.svg?variant=secondary&amp;logo=ri:TerminalBoxLine"></a>
  <a href="https://github.com/GhostPWN/ghostpwn/blob/main/LICENSE"><img alt="MIT License" src="https://shieldcn.dev/github/license/GhostPWN/ghostpwn.svg?variant=secondary"></a>
</p>

<p align="center">
  <a href="https://www.anthropic.com/claude"><img alt="Anthropic Claude" src="https://shieldcn.dev/badge/Claude-Anthropic-d97757.svg?variant=secondary&amp;logo=anthropic"></a>
  <a href="https://openai.com/codex/"><img alt="OpenAI Codex" src="https://shieldcn.dev/badge/Codex-OpenAI-10a37f.svg?variant=secondary&amp;logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyByb2xlPSJpbWciIHZpZXdCb3g9IjAgMCAyNCAyNCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48cGF0aCBmaWxsPSIjZmZmIiBkPSJNMjIuMjgxOSA5LjgyMTFhNS45ODQ3IDUuOTg0NyAwIDAgMC0uNTE1Ny00LjkxMDggNi4wNDYyIDYuMDQ2MiAwIDAgMC02LjUwOTgtMi45QTYuMDY1MSA2LjA2NTEgMCAwIDAgNC45ODA3IDQuMTgxOGE1Ljk4NDcgNS45ODQ3IDAgMCAwLTMuOTk3NyAyLjkgNi4wNDYyIDYuMDQ2MiAwIDAgMCAuNzQyNyA3LjA5NjYgNS45OCA1Ljk4IDAgMCAwIC41MTEgNC45MTA3IDYuMDUxIDYuMDUxIDAgMCAwIDYuNTE0NiAyLjkwMDFBNS45ODQ3IDUuOTg0NyAwIDAgMCAxMy4yNTk5IDI0YTYuMDU1NyA2LjA1NTcgMCAwIDAgNS43NzE4LTQuMjA1OCA1Ljk4OTQgNS45ODk0IDAgMCAwIDMuOTk3Ny0yLjkwMDEgNi4wNTU3IDYuMDU1NyAwIDAgMC0uNzQ3NS03LjA3Mjl6bS05LjAyMiAxMi42MDgxYTQuNDc1NSA0LjQ3NTUgMCAwIDEtMi44NzY0LTEuMDQwOGwuMTQxOS0uMDgwNCA0Ljc3ODMtMi43NTgyYS43OTQ4Ljc5NDggMCAwIDAgLjM5MjctLjY4MTN2LTYuNzM2OWwyLjAyIDEuMTY4NmEuMDcxLjA3MSAwIDAgMSAuMDM4LjA1MnY1LjU4MjZhNC41MDQgNC41MDQgMCAwIDEtNC40OTQ1IDQuNDk0NHptLTkuNjYwNy00LjEyNTRhNC40NzA4IDQuNDcwOCAwIDAgMS0uNTM0Ni0zLjAxMzdsLjE0Mi4wODUyIDQuNzgzIDIuNzU4MmEuNzcxMi43NzEyIDAgMCAwIC43ODA2IDBsNS44NDI4LTMuMzY4NXYyLjMzMjRhLjA4MDQuMDgwNCAwIDAgMS0uMDMzMi4wNjE1TDkuNzQgMTkuOTUwMmE0LjQ5OTIgNC40OTkyIDAgMCAxLTYuMTQwOC0xLjY0NjR6TTIuMzQwOCA3Ljg5NTZhNC40ODUgNC40ODUgMCAwIDEgMi4zNjU1LTEuOTcyOFYxMS42YS43NjY0Ljc2NjQgMCAwIDAgLjM4NzkuNjc2NWw1LjgxNDQgMy4zNTQzLTIuMDIwMSAxLjE2ODVhLjA3NTcuMDc1NyAwIDAgMS0uMDcxIDBsLTQuODMwMy0yLjc4NjVBNC41MDQgNC41MDQgMCAwIDEgMi4zNDA4IDcuODcyem0xNi41OTYzIDMuODU1OEwxMy4xMDM4IDguMzY0IDE1LjExOTIgNy4yYS4wNzU3LjA3NTcgMCAwIDEgLjA3MSAwbDQuODMwMyAyLjc5MTNhNC40OTQ0IDQuNDk0NCAwIDAgMS0uNjc2NSA4LjEwNDJ2LTUuNjc3MmEuNzkuNzkgMCAwIDAtLjQwNy0uNjY3em0yLjAxMDctMy4wMjMxbC0uMTQyLS4wODUyLTQuNzczNS0yLjc4MThhLjc3NTkuNzc1OSAwIDAgMC0uNzg1NCAwTDkuNDA5IDkuMjI5N1Y2Ljg5NzRhLjA2NjIuMDY2MiAwIDAgMSAuMDI4NC0uMDYxNWw0LjgzMDMtMi43ODY2YTQuNDk5MiA0LjQ5OTIgMCAwIDEgNi42ODAyIDQuNjZ6TTguMzA2NSAxMi44NjNsLTIuMDItMS4xNjM4YS4wODA0LjA4MDQgMCAwIDEtLjAzOC0uMDU2N1Y2LjA3NDJhNC40OTkyIDQuNDk5MiAwIDAgMSA3LjM3NTctMy40NTM3bC0uMTQyLjA4MDVMOC43MDQgNS40NTlhLjc5NDguNzk0OCAwIDAgMC0uMzkyNy42ODEzem0xLjA5NzYtMi4zNjU0bDIuNjAyLTEuNDk5OCAyLjYwNjkgMS40OTk4djIuOTk5NGwtMi41OTc0IDEuNDk5Ny0yLjYwNjctMS40OTk3WiIvPjwvc3ZnPg%3D%3D"></a>
  <a href="https://gemini.google.com/"><img alt="Google Gemini" src="https://shieldcn.dev/badge/Gemini-Google-4285f4.svg?variant=secondary&amp;logo=googlegemini"></a>
  <a href="https://github.com/features/copilot"><img alt="GitHub Copilot" src="https://shieldcn.dev/badge/Copilot-GitHub-171717.svg?variant=secondary&amp;logo=githubcopilot"></a>
  <a href="https://ollama.com/"><img alt="Ollama" src="https://shieldcn.dev/badge/Ollama-Local-f4f4f5.svg?variant=secondary&amp;logo=ollama&amp;logoColor=171717"></a>
</p>

> **Academic project.** GhostPWN is the annual project for the 3rd year of the
> Bachelor in Cybersecurity at [ESGI Paris](https://esgi.fr/).

## Install

### macOS

```bash
brew install GhostPWN/tap/ghostpwn
ghostpwn
```

### Linux

```bash
sudo apt-get install -y build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

### Windows

Install [Rust](https://rustup.rs/), then:

```powershell
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

See the [documentation](https://ghostpwn.github.io/ghostpwn/docs/) for detailed
requirements, configuration, commands, and architecture.

## Image input

Vision-capable models can analyze PNG, JPEG, and WebP files from the current
workspace. Reference paths directly in a prompt:

```text
Review @screenshots/login.png for security issues
Compare @before.png with @"screenshots/after login.webp"
```

Use `\@` when an image-like reference should remain literal. Paths outside the
workspace, symlinks, remote URLs, and other image formats are rejected.

Press `Ctrl+V` to attach an image from the system clipboard. If the clipboard
contains text, GhostPWN pastes it into the input instead. `/paste-image` provides
a fallback for terminals that intercept `Ctrl+V`, and `/clear-images` removes
queued clipboard images.

Each message accepts up to 10 images and 15 MiB of image data. Attachments stay
in conversation history for follow-up questions and are resent to the active
provider. `/clear` removes them with the rest of the chat. Image data is sent to
the selected provider, so only attach files you intend to share. Provider and
model limits still apply, and non-vision models return an error.

## Contributing

Contributions are welcome. Open an issue before submitting a pull request.

## License

[MIT](LICENSE)
