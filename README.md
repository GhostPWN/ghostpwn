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
  <a href="https://ghostpwn.github.io/ghostpwn/docs/configuration/"><img alt="Six AI providers" src="https://shieldcn.dev/badge/AI_Providers-6-7c3aed.svg?variant=secondary&amp;logo=ri:Sparkling2Line"></a>
  <a href="https://github.com/GhostPWN/ghostpwn/blob/main/LICENSE"><img alt="MIT License" src="https://shieldcn.dev/github/license/GhostPWN/ghostpwn.svg?variant=secondary"></a>
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
