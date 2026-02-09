# FLUI CLI

Command-line interface for the FLUI framework.

[![Crates.io](https://img.shields.io/crates/v/flui_cli.svg)](https://crates.io/crates/flui_cli)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../../LICENSE-MIT)

## Installation

```bash
cargo install flui_cli
```

Or build from source:

```bash
git clone https://github.com/vanyastaff/flui.git
cd flui
cargo install --path crates/flui_cli
```

## Quick Start

```bash
# Create a new project
flui create my_app

# Run in development mode
cd my_app
flui run

# Build for production
flui build desktop --release
```

## Commands

| Command | Description |
|---------|-------------|
| `flui create <name>` | Create a new FLUI project |
| `flui run` | Run the application |
| `flui build <platform>` | Build for target platform |
| `flui test` | Run tests |
| `flui analyze` | Run clippy analysis |
| `flui format` | Format source code |
| `flui clean` | Clean build artifacts |
| `flui doctor` | Check environment setup |
| `flui devices` | List available devices |
| `flui upgrade` | Update CLI and dependencies |
| `flui platform` | Manage platform support |
| `flui completions` | Generate shell completions |

## Project Creation

```bash
# Default counter template
flui create my_app

# With organization ID
flui create my_app --org com.example

# Specific template
flui create my_app --template basic

# Skip git initialization
flui create my_app --no-git
```

### Templates

- **counter** (default) - Counter app with state management
- **basic** - Minimal "Hello, FLUI!" application
- **todo** - Todo list application (planned)
- **dashboard** - Dashboard with multiple widgets (planned)

## Building

```bash
# Desktop (current platform)
flui build desktop --release

# Android
flui build android --release
flui build android --release --split-per-abi

# iOS
flui build ios --release
flui build ios --release --universal

# Web
flui build web --release
flui build web --release --optimize-wasm
```

## Development

```bash
# Run with hot reload
flui run

# Run in release mode
flui run --release

# Run on specific device
flui run --device pixel_5
```

## Code Quality

```bash
# Run tests
flui test

# Analyze with clippy
flui analyze
flui analyze --fix

# Format code
flui format
flui format --check
```

## Environment

Check your development environment:

```bash
flui doctor
flui doctor --verbose
flui doctor --android
```

### Requirements

**Desktop:**
- Rust 1.75+
- Platform build tools (MSVC, GCC, or Xcode)

**Android:**
- Android SDK
- Android NDK r25+
- JDK 11+

**iOS (macOS only):**
- Xcode 14+

**Web:**
- wasm-pack (optional)
- Browser with WebGPU support

## Configuration

### Project (flui.toml)

```toml
[app]
name = "my_app"
version = "0.1.0"
organization = "com.example"

[build]
target_platforms = ["windows", "linux", "macos"]

[assets]
directories = ["assets"]
```

### Global (~/.flui/config.toml)

```toml
[sdk]
channel = "stable"

[build]
jobs = 4

[devtools]
port = 9100
```

## Shell Completions

```bash
# Bash
flui completions bash > ~/.local/share/bash-completion/completions/flui

# Zsh
flui completions zsh > ~/.zfunc/_flui

# Fish
flui completions fish > ~/.config/fish/completions/flui.fish

# PowerShell
flui completions powershell >> $PROFILE
```

## License

MIT OR Apache-2.0
