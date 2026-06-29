# Installation

## macOS

```bash
brew install GhostPWN/tap/ghostpwn
ghostpwn
```

The Homebrew formula lives in `Formula/ghostpwn.rb`.

## Linux

Install Rust and common native build dependencies, then install GhostPWN with Cargo:

```bash
sudo apt-get install -y build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

For non-Debian distributions, install the equivalent C compiler/toolchain and `pkg-config` packages before running `cargo install`.

## Windows

Install Rust, then build or install GhostPWN with Cargo:

```powershell
cargo install --git https://github.com/GhostPWN/ghostpwn
ghostpwn
```

Windows CI builds also publish a `ghostpwn-windows-x86_64.zip` artifact containing `ghostpwn.exe`.

## Development

Run from a clone:

```bash
cargo run
```
