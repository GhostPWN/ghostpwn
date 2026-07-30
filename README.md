<p align="center">
  <img src="https://raw.githubusercontent.com/GhostPWN/.github/main/profile/logo.svg" alt="GhostPWN" width="200">
</p>

<h1 align="center">GhostPWN</h1>

<p align="center">
  <strong>Autonomous penetration testing agent</strong><br>
  A Rust TUI with multi-provider LLM support, human-in-the-loop tool execution, and workspace boundaries.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/runtime-Rust-000000?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/lang-2024%20Edition-dea584?style=flat-square" alt="Rust 2024 Edition">
  <img src="https://img.shields.io/badge/ui-ratatui%20%2B%20crossterm-4a5568?style=flat-square" alt="ratatui + crossterm">
  <img src="https://img.shields.io/badge/providers-OpenAI%20%7C%20Anthropic%20%7C%20Google%20%7C%20Copilot%20%7C%20Ollama-7c3aed?style=flat-square" alt="Providers">
  <img src="https://img.shields.io/badge/license-MIT-7c3aed?style=flat-square" alt="License">
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

## Contributing

Contributions are welcome. Open an issue before submitting a pull request.

## License

[MIT](LICENSE)
