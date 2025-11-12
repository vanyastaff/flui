# FLUI CLI Documentation

> **Command-line interface for FLUI - Project creation, building, and deployment automation**

Version: 0.1.0 (Planned)  
Status: In Development

---

## Table of Contents

1. [Overview](#overview)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Commands](#commands)
5. [Project Configuration](#project-configuration)
6. [Platform Support](#platform-support)
7. [Workflows](#workflows)
8. [Advanced Usage](#advanced-usage)
9. [Troubleshooting](#troubleshooting)
10. [Development](#development)

---

## Overview

### What is flui_cli?

`flui_cli` is a command-line tool for FLUI that provides a Flutter-like developer experience:

```bash
# Create a new project
flui create my_app

# Run on desktop
flui run

# Build for Android
flui build android --release

# Check your environment
flui doctor
```

### Key Features

✅ **Project scaffolding** - Quick project creation with templates  
✅ **Cross-platform builds** - One command for all platforms  
✅ **Environment diagnostics** - `flui doctor` checks your setup  
✅ **Device management** - List and launch emulators  
✅ **Hot reload** - Fast development iteration  
✅ **Template system** - Pre-built app templates

### Design Philosophy

Inspired by Flutter CLI, but designed for Rust:

- **Cargo integration** - Works seamlessly with cargo
- **Zero magic** - Transparent build process
- **Platform native** - Uses native toolchains
- **Extensible** - Plugin system for custom commands

---

## Installation

### Prerequisites

Before installing `flui_cli`, ensure you have:

```bash
# Rust toolchain (1.70+)
rustup --version

# Cargo package manager
cargo --version
```

### Install from Source

```bash
# Clone the repository
git clone https://github.com/vanyastaff/flui.git
cd flui

# Install CLI globally
cargo install --path crates/flui_cli

# Verify installation
flui --version
```

### Install from crates.io (Future)

```bash
# When published
cargo install flui_cli

# Check installation
flui --version
```

### Update

```bash
# Update to latest version
flui upgrade

# Or via cargo
cargo install flui_cli --force
```

---

## Quick Start

### Create Your First App

```bash
# Create a new project
flui create hello_flui

# Navigate to project
cd hello_flui

# Run on desktop
flui run

# Build for release
flui build --release
```

### Create with Template

```bash
# Counter app
flui create my_counter --template counter

# Todo list app
flui create my_todos --template todo

# Dashboard UI
flui create my_dashboard --template dashboard
```

### Multi-Platform Setup

```bash
# Create with specific platforms
flui create my_app --platforms android,ios,web

# Or add platforms later
cd my_app
flui platform add android
flui platform add ios
flui platform add web
```

---

## Commands

### `flui create`

Create a new FLUI project.

```bash
flui create <PROJECT_NAME> [OPTIONS]
```

**Arguments:**
- `PROJECT_NAME` - Name of the project (snake_case recommended)

**Options:**
- `--template <TEMPLATE>` - Use a template (basic, counter, todo, dashboard)
- `--platforms <PLATFORMS>` - Comma-separated list of platforms
- `--org <ORGANIZATION>` - Organization identifier (e.g., com.example)
- `--lib` - Create a library instead of an application
- `--path <PATH>` - Custom output directory

**Examples:**

```bash
# Basic app
flui create my_app

# With counter template
flui create my_counter --template counter

# Multi-platform
flui create my_app --platforms android,ios,web

# Custom organization
flui create my_app --org com.mycompany

# Library crate
flui create my_lib --lib
```

**Generated Structure:**

```
my_app/
├── src/
│   └── main.rs              # Application entry point
├── platforms/
│   ├── android/             # Android configuration
│   ├── ios/                 # iOS configuration
│   └── web/                 # Web configuration
├── assets/                  # Application assets
├── Cargo.toml              # Rust dependencies
├── flui.toml               # FLUI configuration
└── README.md
```

---

### `flui run`

Run the application on a device or simulator.

```bash
flui run [OPTIONS]
```

**Options:**
- `-d, --device <DEVICE>` - Target device (auto-detected if omitted)
- `-r, --release` - Build in release mode
- `--hot-reload` - Enable hot reload (development mode)
- `--profile <PROFILE>` - Build profile (dev, release, bench)
- `--verbose` - Verbose output

**Examples:**

```bash
# Run on desktop (auto-detected)
flui run

# Run on specific device
flui run --device android

# Run on Genymotion emulator
flui run --device genymotion-pixel6

# Run on web browser
flui run --device web

# Release mode
flui run --release

# With hot reload
flui run --hot-reload
```

**Device Selection:**

```bash
# List available devices
flui devices

# Run on specific device by ID
flui run -d emulator-5554

# Run on Chrome
flui run -d chrome
```

---

### `flui build`

Build the application for a target platform.

```bash
flui build <PLATFORM> [OPTIONS]
```

**Platforms:**
- `android` - Android APK/AAB
- `ios` - iOS IPA
- `web` - WebAssembly
- `windows` - Windows executable
- `linux` - Linux executable
- `macos` - macOS app bundle

**Options:**
- `-r, --release` - Build in release mode (optimized)
- `-o, --output <PATH>` - Output directory
- `--split-per-abi` - Android: Create separate APKs per ABI
- `--optimize-wasm` - Web: Optimize WASM size
- `--universal` - iOS: Build universal binary (arm64 + simulator)

**Examples:**

```bash
# Build for Android (debug)
flui build android

# Build for Android (release)
flui build android --release

# Build for iOS with universal binary
flui build ios --release --universal

# Build for Web with optimizations
flui build web --release --optimize-wasm

# Build all platforms
flui build android ios web --release
```

**Output Locations:**

```
target/
├── android/
│   └── app-release.apk
├── ios/
│   └── Runner.ipa
├── web/
│   └── pkg/
└── release/
    └── my_app(.exe)
```

---

### `flui doctor`

Check your development environment.

```bash
flui doctor [OPTIONS]
```

**Options:**
- `-v, --verbose` - Show detailed information
- `--android` - Check only Android toolchain
- `--ios` - Check only iOS toolchain
- `--web` - Check only Web toolchain

**Example Output:**

```
🔍 Checking FLUI environment...

[✓] FLUI (version 0.1.0)
    • flui_cli installed at: ~/.cargo/bin/flui

[✓] Rust Toolchain (version 1.75.0)
    • rustc: 1.75.0 (82e1608df 2023-12-21)
    • cargo: 1.75.0 (82e1608df 2023-12-21)

[✓] Cargo Tools
    • cargo-ndk: 3.4.0
    • wasm-pack: 0.12.1

[✓] Android Toolchain
    • Android SDK: C:\Users\Vanya\AppData\Local\Android\Sdk
    • NDK: 27.0.12077973
    • Platform: android-34
    • Build tools: 34.0.0
    • Emulators: 2 available

[!] iOS Toolchain (macOS only)
    ✗ Not available on Windows

[✓] Web Toolchain
    • wasm32-unknown-unknown: installed
    • wasm-pack: 0.12.1
    • Chrome: 120.0.6099.109

• No issues found!
```

---

### `flui devices`

List connected devices and emulators.

```bash
flui devices [OPTIONS]
```

**Options:**
- `--details` - Show detailed device information
- `--platform <PLATFORM>` - Filter by platform

**Example Output:**

```
4 connected devices:

Desktop (desktop)
• Windows 11 (version 10.0.22631)

Pixel 6 (android)
• emulator-5554 • Android 13 (API 33)
• Running • genymotion

iPhone 14 Pro (ios)
• 12345678-ABCD-1234-5678-1234567890AB • iOS 17.0
• Connected • Xcode simulator

Chrome (web)
• chrome • WebGPU supported
• Running • localhost:8080
```

**Device Management:**

```bash
# List emulators
flui emulators

# Launch emulator
flui emulators --launch Pixel_6_API_33

# Launch iOS simulator
flui emulators --launch "iPhone 14 Pro"
```

---

### `flui clean`

Remove build artifacts and caches.

```bash
flui clean [OPTIONS]
```

**Options:**
- `--deep` - Deep clean (including cargo caches)
- `--platform <PLATFORM>` - Clean specific platform only

**Examples:**

```bash
# Standard clean
flui clean

# Deep clean (removes all build artifacts)
flui clean --deep

# Clean only Android builds
flui clean --platform android
```

**What Gets Cleaned:**

- `target/` directory
- Platform-specific build outputs
- Generated code
- Cache files
- (with `--deep`) Cargo registry caches

---

### `flui upgrade`

Update flui_cli and project dependencies.

```bash
flui upgrade [OPTIONS]
```

**Options:**
- `--self` - Update flui_cli only
- `--dependencies` - Update project dependencies only

**Examples:**

```bash
# Update CLI
flui upgrade --self

# Update project dependencies
flui upgrade --dependencies

# Update both
flui upgrade
```

---

### `flui platform`

Manage platform support for your project.

```bash
flui platform <SUBCOMMAND>
```

**Subcommands:**
- `add <PLATFORM>` - Add platform support
- `remove <PLATFORM>` - Remove platform support
- `list` - List supported platforms

**Examples:**

```bash
# Add Android support
flui platform add android

# Add multiple platforms
flui platform add ios web

# List current platforms
flui platform list

# Remove platform
flui platform remove android
```

---

### `flui test`

Run tests for your project.

```bash
flui test [OPTIONS]
```

**Options:**
- `--unit` - Run unit tests only
- `--integration` - Run integration tests only
- `--platform <PLATFORM>` - Test on specific platform

**Examples:**

```bash
# Run all tests
flui test

# Unit tests only
flui test --unit

# Test on Android emulator
flui test --platform android
```

---

### `flui format`

Format source code.

```bash
flui format [OPTIONS]
```

**Options:**
- `--check` - Check formatting without modifying files

**Examples:**

```bash
# Format all files
flui format

# Check formatting
flui format --check
```

---

### `flui analyze`

Analyze code for issues.

```bash
flui analyze [OPTIONS]
```

**Options:**
- `--fix` - Automatically fix issues
- `--pedantic` - Enable pedantic lints

**Examples:**

```bash
# Analyze code
flui analyze

# Auto-fix issues
flui analyze --fix
```

---

## Project Configuration

### flui.toml

Configuration file for FLUI projects.

```toml
# flui.toml

[package]
name = "my_app"
version = "0.1.0"
authors = ["Vanya <vanya@example.com>"]
edition = "2021"

# Platform configuration
[platforms]
android = { enabled = true, min_sdk = 26, target_sdk = 34 }
ios = { enabled = true, deployment_target = "13.0" }
web = { enabled = true, optimize = true }
desktop = { enabled = true }

# Android-specific configuration
[android]
package = "com.example.myapp"
permissions = ["INTERNET", "CAMERA", "ACCESS_FINE_LOCATION"]
features = ["webgpu"]

# Build-time permissions check
validate_permissions = true

# Gradle configuration
[android.gradle]
compile_sdk = 34
build_tools = "34.0.0"

# iOS-specific configuration
[ios]
bundle_id = "com.example.myapp"
team_id = "ABCDEF1234"
deployment_target = "13.0"

# Capabilities
capabilities = ["camera", "location"]

# Web-specific configuration
[web]
optimize_wasm = true
target = "web"  # or "bundler"
features = ["webgpu"]

# Build configuration
[build]
default_release = false
hot_reload = true
parallel = true

# Build profiles
[build.profiles.dev]
opt_level = 0
debug = true

[build.profiles.release]
opt_level = 3
debug = false
lto = true

# Dependencies
[dependencies]
flui_core = "0.7"
flui_widgets = "0.7"
flui_app = "0.7"

# Assets
[assets]
# Directories to include
dirs = ["assets/images", "assets/fonts"]

# Individual files
files = ["config.json"]

# Dev dependencies
[dev-dependencies]
flui_test = "0.7"
```

### Environment Variables

Configure behavior through environment variables:

```bash
# Android
export ANDROID_HOME="/path/to/android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"

# iOS
export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"

# Build
export FLUI_LOG=debug
export FLUI_HOT_RELOAD=1
```

---

## Platform Support

### Android

#### Prerequisites

```bash
# Check Android setup
flui doctor --android
```

**Requirements:**
- Android SDK (API 26+)
- Android NDK (r25+)
- Java Development Kit (JDK 17+)
- Gradle

#### Building

```bash
# Debug build
flui build android

# Release build
flui build android --release

# Split APKs by ABI (smaller size)
flui build android --release --split-per-abi
```

#### Running

```bash
# List devices
flui devices

# Run on connected device
flui run --device android

# Run on emulator
flui emulators --launch Pixel_6_API_33
flui run
```

#### Signing (Release)

```bash
# Generate keystore
keytool -genkey -v -keystore release.keystore \
  -keyalg RSA -keysize 2048 -validity 10000

# Configure in flui.toml
[android.signing]
keystore = "release.keystore"
key_alias = "release"
```

---

### iOS

#### Prerequisites

```bash
# Check iOS setup (macOS only)
flui doctor --ios
```

**Requirements:**
- macOS 10.15+
- Xcode 14+
- iOS deployment target 13.0+
- CocoaPods (optional)

#### Building

```bash
# Debug build
flui build ios

# Release build
flui build ios --release

# Universal binary (device + simulator)
flui build ios --release --universal
```

#### Running

```bash
# List simulators
flui devices --platform ios

# Run on simulator
flui run --device "iPhone 14 Pro"

# Run on physical device
flui run --device <device-id>
```

#### Code Signing

```bash
# Configure in flui.toml
[ios]
team_id = "ABCDEF1234"
provisioning_profile = "path/to/profile.mobileprovision"
```

---

### Web

#### Prerequisites

```bash
# Check web setup
flui doctor --web
```

**Requirements:**
- wasm-pack
- Browser with WebGPU support (Chrome 113+)

#### Building

```bash
# Debug build
flui build web

# Release build (optimized)
flui build web --release --optimize-wasm
```

#### Running

```bash
# Development server
flui run --device web

# Or serve manually
cd platforms/web
python -m http.server 8080
```

#### Deployment

```bash
# Build for production
flui build web --release --optimize-wasm

# Deploy to hosting
# platforms/web/pkg/ contains all files
```

---

### Desktop

#### Windows

```bash
# Build
flui build windows --release

# Output: target/release/my_app.exe
```

#### Linux

```bash
# Build
flui build linux --release

# Output: target/release/my_app
```

#### macOS

```bash
# Build
flui build macos --release

# Create app bundle
flui build macos --release --bundle

# Output: target/release/MyApp.app
```

---

## Workflows

### Development Workflow

```bash
# 1. Create project
flui create my_app --template counter

# 2. Navigate to project
cd my_app

# 3. Run with hot reload
flui run --hot-reload

# 4. Make changes (automatically reloads)
# Edit src/main.rs

# 5. Test on multiple platforms
flui run --device android
flui run --device web
```

### Release Workflow

```bash
# 1. Clean previous builds
flui clean

# 2. Run tests
flui test

# 3. Analyze code
flui analyze

# 4. Build for all platforms
flui build android --release
flui build ios --release
flui build web --release --optimize-wasm

# 5. Verify builds
ls -lh target/android/app-release.apk
ls -lh target/ios/Runner.ipa
ls -lh platforms/web/pkg/
```

### CI/CD Integration

#### GitHub Actions

```yaml
# .github/workflows/build.yml
name: Build

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Install flui_cli
        run: cargo install --path crates/flui_cli
      
      - name: Check environment
        run: flui doctor
      
      - name: Build
        run: flui build android --release
      
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: android-apk
          path: target/android/app-release.apk
```

---

## Advanced Usage

### Custom Templates

Create your own project templates:

```bash
# Template structure
~/.flui/templates/
└── my_template/
    ├── template.toml
    ├── src/
    │   └── main.rs.template
    └── Cargo.toml.template
```

**template.toml:**

```toml
[template]
name = "my_template"
description = "My custom template"
version = "1.0.0"

[variables]
project_name = { type = "string", prompt = "Project name?" }
author = { type = "string", prompt = "Author name?" }
```

**Usage:**

```bash
flui create my_app --template my_template
```

### Plugins

Extend flui_cli with plugins:

```bash
# Install plugin
flui plugin install flui-deploy

# List plugins
flui plugin list

# Use plugin command
flui deploy firebase
```

### Custom Build Scripts

Add custom build steps in `flui.toml`:

```toml
[build.scripts]
pre_build = "python scripts/generate_assets.py"
post_build = "python scripts/sign_apk.py"
```

### Multi-Configuration

Support different configurations:

```bash
# Development
flui run --config dev

# Staging
flui run --config staging

# Production
flui run --config production
```

**flui.toml:**

```toml
[config.dev]
api_url = "http://localhost:3000"
debug = true

[config.staging]
api_url = "https://staging.example.com"
debug = true

[config.production]
api_url = "https://api.example.com"
debug = false
```

---

## Troubleshooting

### Common Issues

#### "flui: command not found"

**Solution:**

```bash
# Ensure cargo bin is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Reinstall
cargo install --path crates/flui_cli --force
```

#### "Android SDK not found"

**Solution:**

```bash
# Set ANDROID_HOME
export ANDROID_HOME="$HOME/Android/Sdk"

# Verify
flui doctor --android
```

#### "NDK not found"

**Solution:**

```bash
# Install NDK via Android Studio SDK Manager
# Or set manually
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
```

#### "wasm-pack not found"

**Solution:**

```bash
# Install wasm-pack
cargo install wasm-pack

# Verify
wasm-pack --version
```

#### "Device not found"

**Solution:**

```bash
# Check connected devices
flui devices

# For Android: Enable USB debugging
# For iOS: Trust computer in device settings
```

#### "Build failed: linker error"

**Solution:**

```bash
# Clean and rebuild
flui clean --deep
flui build android --release

# Check NDK version matches
rustup target list --installed
```

### Getting Help

```bash
# General help
flui --help

# Command-specific help
flui create --help
flui build --help
flui run --help

# Verbose output
flui build android --verbose

# Check environment
flui doctor -v
```

---

## Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/vanyastaff/flui.git
cd flui

# Build CLI
cargo build -p flui_cli --release

# Install locally
cargo install --path crates/flui_cli
```

### Running Tests

```bash
# Unit tests
cargo test -p flui_cli

# Integration tests
cargo test -p flui_cli --test integration

# All tests
cargo test --workspace
```

### Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

**Areas for contribution:**

- New templates
- Platform support improvements
- Documentation
- Bug fixes
- Feature requests

---

## Roadmap

### v0.1.0 (MVP) - Q1 2024
- ✅ `flui create` - Project scaffolding
- ✅ `flui run` - Desktop support
- ✅ `flui build` - Desktop builds
- ✅ `flui doctor` - Basic diagnostics

### v0.2.0 - Q2 2024
- 🔨 Android support
- 🔨 `flui devices` command
- 🔨 Template system
- 🔨 Hot reload (desktop)

### v0.3.0 - Q3 2024
- 🔨 iOS support
- 🔨 Web support
- 🔨 Multi-platform builds
- 🔨 CI/CD templates

### v0.4.0 - Q4 2024
- 🔨 Hot reload (mobile)
- 🔨 Plugin system
- 🔨 Advanced templates
- 🔨 Performance profiling

### v1.0.0 - 2025
- 🔨 Stable API
- 🔨 Complete platform support
- 🔨 Production-ready tooling

---

## FAQ

### Q: How does flui_cli compare to Flutter CLI?

**A:** Similar philosophy, Rust implementation:

| Feature | Flutter CLI | flui_cli |
|---------|-------------|----------|
| Project creation | ✅ | ✅ |
| Hot reload | ✅ | 🔨 |
| Multi-platform | ✅ | ✅ |
| Language | Dart | Rust |
| Build system | Gradle/Xcode | Cargo/Native |

### Q: Can I use flui_cli with existing Rust projects?

**A:** Yes! Add flui dependencies to `Cargo.toml` and create `flui.toml`.

### Q: Does flui_cli require Flutter?

**A:** No! FLUI is completely independent.

### Q: What platforms are supported?

**A:** Desktop (Windows/Linux/macOS), Mobile (Android/iOS), Web (WebAssembly).

### Q: How do I report bugs?

**A:** Open an issue on [GitHub](https://github.com/vanyastaff/flui/issues).

---

## Resources

### Documentation
- [FLUI App Documentation](./FLUI_APP_DOCUMENTATION.md)
- [FLUI Core Documentation](../flui_core/README.md)
- [Widget Library](../flui_widgets/README.md)

### Examples
- [Counter App](../../examples/counter_demo.rs)
- [Todo List](../../examples/todo_app.rs)
- [Dashboard](../../examples/dashboard.rs)

### Community
- [GitHub Discussions](https://github.com/vanyastaff/flui/discussions)
- [Discord Server](#) (Coming soon)

---

## License

MIT OR Apache-2.0

---

## Acknowledgments

Inspired by:
- **Flutter CLI** - Developer experience patterns
- **Cargo** - Rust toolchain integration
- **Create React App** - Project scaffolding
- **Tauri CLI** - Multi-platform tooling

---

**Built with ❤️ in Rust**

*"Making cross-platform Rust development delightful"*
