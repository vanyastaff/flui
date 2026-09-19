# FLUI CLI

Command-line interface for the FLUI framework.

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../../LICENSE-MIT)

## Installation

FLUI is not published to crates.io yet, so build from source:

```bash
git clone https://github.com/vanyastaff/flui.git
cd flui
cargo install --path crates/flui-cli
```

## Quick Start

```bash
# From the FLUI checkout, create an application outside it
flui create my_app --local --path ../apps

# Run in development mode
cd ../apps/my_app
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
# Default counter template, from the FLUI checkout
flui create my_app --local

# With organization ID
flui create my_app --local --org com.example

# Specific template
flui create my_app --local --template basic

# Explicit checkout, from any working directory
flui create my_app --local=/path/to/flui --path /path/to/apps
```

Until FLUI is published, use local source dependencies. Bare `--local` selects
the current working directory as the FLUI checkout; `--local=/path/to/flui`
selects an explicit checkout. The `=` is required when supplying a path so that
`flui create --local my_app` still treats `my_app` as the project name.

`--path` selects the parent directory of the generated application. It can be
outside the checkout and at any directory depth. Generated manifests contain
absolute paths to the selected checkout, including in the hot-reload workspace:

```bash
flui create my_app --local=/path/to/flui --hot-reload --path /path/to/apps
```

Keep the selected checkout available while using a local project. Moving the
application does not break those dependency paths; moving the checkout requires
updating them. Omit `--local` to generate registry dependencies once the matching
FLUI version is published. Invalid local source paths fail before project creation.

### Templates

All templates declare `flui` as their only framework dependency. Generated code
imports the public facade, beginning with `use flui::prelude::*;`. Local projects
point that dependency at the checkout root; registry projects select the CLI's
framework version. The hot-reload workspace enables `flui`'s `hot-reload` feature
in its host, worker, and shared-types crates, and keeps sibling paths relative.

`tests/cli_create.rs` checks all three generated project shapes in external
temporary directories using Cargo.

- **counter** (default) — `Column` of `Text` widgets showing a static count.
  This template does not yet demonstrate interactive state updates.
- **basic** — minimal "Hello, FLUI!" `StatelessView`.
- **todo** — Todo list application (planned)
- **dashboard** — Dashboard with multiple widgets (planned)

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
