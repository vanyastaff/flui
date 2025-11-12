# FLUI CLI

**Command-line interface for FLUI** - Project creation, building, and deployment automation.

[![Crates.io](https://img.shields.io/crates/v/flui_cli.svg)](https://crates.io/crates/flui_cli)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../LICENSE)

## Overview

`flui_cli` is a command-line tool that provides a Flutter-like developer experience for FLUI applications. It handles project scaffolding, building, running, testing, and deployment across multiple platforms.

## Installation

### From source

```bash
# Clone the repository
git clone https://github.com/vanyastaff/flui.git
cd flui

# Install CLI globally
cargo install --path crates/flui_cli

# Verify installation
flui --version
```

### From crates.io (when published)

```bash
cargo install flui_cli
flui --version
```

## Quick Start

### Create a new project

```bash
# Create a counter app (default template)
flui create my_app

# Create with custom organization
flui create my_app --org com.mycompany

# Create with specific template
flui create my_app --template basic
```

### Run your app

```bash
cd my_app

# Run in debug mode with hot reload
flui run

# Run in release mode
flui run --release
```

### Build for production

```bash
# Build for current desktop platform
flui build desktop --release

# Build for Android
flui build android --release

# Build for Web
flui build web --release
```

## Available Commands

### Project Management

- **`flui create <name>`** - Create a new FLUI project
  - `--org <ORG>` - Organization name (reverse domain notation)
  - `--template <TEMPLATE>` - Project template (counter, basic, todo, dashboard)
  - `--platforms <PLATFORMS>` - Target platforms (comma-separated)

### Development

- **`flui run`** - Run the application
  - `-r, --release` - Build in release mode
  - `--device <DEVICE>` - Target device
  - `--hot-reload` - Enable hot reload (default: true)

- **`flui test`** - Run tests
  - `--unit` - Run unit tests only
  - `--integration` - Run integration tests only

- **`flui analyze`** - Analyze code for issues
  - `--fix` - Automatically fix issues
  - `--pedantic` - Enable pedantic lints

- **`flui format`** - Format source code
  - `--check` - Check formatting without modifying files

### Build & Deploy

- **`flui build <platform>`** - Build for target platform
  - Platforms: `android`, `ios`, `web`, `windows`, `linux`, `macos`, `desktop`
  - `-r, --release` - Build in release mode
  - `-o, --output <PATH>` - Output directory
  - `--split-per-abi` - Android: Create separate APKs per ABI
  - `--optimize-wasm` - Web: Optimize WASM size
  - `--universal` - iOS: Build universal binary

### Utilities

- **`flui doctor`** - Check environment setup
  - `-v, --verbose` - Show detailed information
  - `--android` - Check only Android toolchain
  - `--ios` - Check only iOS toolchain
  - `--web` - Check only Web toolchain

- **`flui devices`** - List available devices
  - `--details` - Show detailed device information

- **`flui clean`** - Clean build artifacts
  - `--deep` - Deep clean (including cargo caches)

- **`flui upgrade`** - Update flui_cli and dependencies
  - `--self` - Update flui_cli only
  - `--dependencies` - Update project dependencies only

- **`flui platform`** - Manage platform support
  - `add <platform>` - Add platform support
  - `remove <platform>` - Remove platform support
  - `list` - List supported platforms

## Project Templates

### Counter (default)
A simple counter app demonstrating state management with hooks.

```bash
flui create my_counter --template counter
```

### Basic
A minimal FLUI application with just a "Hello, FLUI!" message.

```bash
flui create my_app --template basic
```

### Todo (planned)
A todo list app demonstrating lists, state management, and user input.

### Dashboard (planned)
A dashboard UI with multiple widgets and complex layouts.

## Environment Setup

### Check your environment

```bash
flui doctor
```

This will check for:
- ✓ Rust installation
- ✓ Cargo package manager
- ✓ FLUI CLI version
- ✓ Platform-specific tools (Android SDK, Xcode, etc.)
- ✓ wgpu support

### Platform-specific requirements

**Android:**
- Android SDK
- Android NDK (r25+)
- Java Development Kit (JDK 11+)

**iOS (macOS only):**
- Xcode 14+
- iOS deployment target 13.0+

**Web:**
- wasm-pack (optional but recommended)
- Modern browser with WebGPU support

**Desktop:**
- Platform build tools (MSVC/GCC/Xcode)

## Configuration

### Project configuration (flui.toml)

```toml
[app]
name = "my_app"
version = "0.1.0"
organization = "com.example"

[build]
target_platforms = ["windows", "linux", "macos"]

[assets]
directories = ["assets"]

[fonts]
# Custom fonts configuration
```

### Global configuration (~/.flui/config.toml)

```toml
[sdk]
channel = "stable"

[build]
jobs = 4

[devtools]
port = 9100
auto_launch = true
```

## Examples

### Development workflow

```bash
# Create project
flui create my_app --template counter

# Navigate to project
cd my_app

# Run with hot reload
flui run

# Make changes to src/main.rs (automatically reloads)

# Test
flui test

# Analyze code
flui analyze

# Format code
flui format
```

### Release workflow

```bash
# Clean previous builds
flui clean

# Run tests
flui test

# Analyze code
flui analyze

# Build for multiple platforms
flui build android --release
flui build web --release --optimize-wasm
flui build desktop --release

# Verify builds
ls target/flui-out/
```

## Troubleshooting

### "flui: command not found"

```bash
# Ensure cargo bin is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Reinstall
cargo install --path crates/flui_cli --force
```

### "Android SDK not found"

```bash
# Set ANDROID_HOME
export ANDROID_HOME="$HOME/Android/Sdk"

# Verify
flui doctor --android
```

### "Not a FLUI project"

Make sure you're in a directory with `Cargo.toml` and FLUI dependencies.

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Resources

- [FLUI Documentation](../../docs/)
- [Examples](../../examples/)
- [GitHub Repository](https://github.com/vanyastaff/flui)

---

**Built with ❤️ in Rust**

*Making cross-platform Rust UI development delightful*
