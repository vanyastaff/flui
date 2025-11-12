# FLUI Build System

This document describes how to build FLUI applications for different platforms using the **xtask** build system.

## Quick Start

```bash
# Check your environment
cargo xtask info

# Build for Android (debug)
cargo xtask build android

# Build for Android (release)
cargo xtask build android --release

# Build for Web
cargo xtask build web --release

# Build for Desktop
cargo xtask build desktop --release

# Clean build artifacts
cargo xtask clean --all
```

## Installation & Setup

### Prerequisites

**All platforms:**
- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- Git

**Android:**
- [Android Studio](https://developer.android.com/studio) or Android SDK
- [Android NDK](https://developer.android.com/ndk) (version 23+)
- [Java JDK 11+](https://adoptium.net/)
- `cargo-ndk`: `cargo install cargo-ndk`
- Android Rust targets: `rustup target add aarch64-linux-android`

**Web:**
- `wasm-pack`: `cargo install wasm-pack`
- WASM target: `rustup target add wasm32-unknown-unknown`

**Desktop:**
- Platform-specific build tools (MSVC on Windows, Xcode on macOS, GCC on Linux)

### Environment Variables

**Android:**
```bash
# Windows
set ANDROID_HOME=C:\Users\<user>\AppData\Local\Android\Sdk
set ANDROID_NDK_HOME=C:\Users\<user>\AppData\Local\Android\Sdk\ndk\29.0.14206865
set JAVA_HOME=C:\Program Files\Java\jdk-17

# Linux/macOS
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/29.0.14206865
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk
```

The build system will attempt to auto-detect these if not set.

---

## Platform-Specific Guides

### Android

**Build Debug APK:**
```bash
cargo xtask build android
```

**Build Release APK:**
```bash
cargo xtask build android --release
```

**Build for Multiple Targets:**
```bash
cargo xtask build android --android-targets arm64-v8a,armeabi-v7a,x86_64 --release
```

**Output:**
- Debug: `target/flui-out/android/flui-debug.apk`
- Release: `target/flui-out/android/flui-release.apk`

**Install on Device:**
```bash
adb install target/flui-out/android/flui-release.apk
```

**Troubleshooting:**
- **"cargo-ndk not found"**: Run `cargo install cargo-ndk`
- **"ANDROID_NDK_HOME not set"**: Set the environment variable or install NDK via Android Studio
- **"No Android targets installed"**: Run `rustup target add aarch64-linux-android`

---

### Web (WebAssembly)

**Build for Web:**
```bash
cargo xtask build web --release
```

**Output:**
- `target/flui-out/web/`
  - `index.html`
  - `flui_app.js`
  - `flui_app_bg.wasm`
  - `manifest.json`
  - `icons/`

**Serve Locally:**
```bash
cd target/flui-out/web
python -m http.server 8080
# Or use: npx serve .
```

Open `http://localhost:8080` in your browser.

**Troubleshooting:**
- **"wasm-pack not found"**: Run `cargo install wasm-pack`
- **"wasm32-unknown-unknown target not installed"**: Run `rustup target add wasm32-unknown-unknown`

---

### Desktop (Windows/Linux/macOS)

**Build for Host Platform:**
```bash
cargo xtask build desktop --release
```

**Build for Specific Target:**
```bash
# Windows from Windows
cargo xtask build desktop --desktop-target x86_64-pc-windows-msvc --release

# macOS (Apple Silicon)
cargo xtask build desktop --desktop-target aarch64-apple-darwin --release

# Linux
cargo xtask build desktop --desktop-target x86_64-unknown-linux-gnu --release
```

**Output:**
- Windows: `target/flui-out/desktop/flui_app.exe`
- macOS/Linux: `target/flui-out/desktop/flui_app`

**Run:**
```bash
# Windows
.\target\flui-out\desktop\flui_app.exe

# macOS/Linux
./target/flui-out/desktop/flui_app
```

---

## Configuration

Configuration is loaded from `flui-build.toml` in the workspace root.

**Example `flui-build.toml`:**
```toml
[build]
out-dir = "target/flui-out"
log-level = "info"

[android]
min-sdk = 21
target-sdk = 35
targets = ["arm64-v8a"]

[web]
target = "web"
wasm-opt-level = "z"

[desktop]
targets = []
bundle-assets = true
```

**User-Specific Overrides:**

Copy `flui-build.toml` to `flui-build.local.toml` for local customizations (this file is .gitignored).

---

## Commands Reference

### `cargo xtask build <platform> [OPTIONS]`

Build for a specific platform.

**Platforms:** `android`, `web`, `desktop`

**Options:**
- `--release`, `-r` - Build in release mode with optimizations
- `--android-targets <TARGETS>` - Android ABIs (comma-separated): `arm64-v8a`, `armeabi-v7a`, `x86_64`, `x86`
- `--web-target <TARGET>` - Web target: `web` (default), `bundler`, `nodejs`
- `--desktop-target <TARGET>` - Desktop target triple (auto-detected if omitted)
- `--features <FEATURES>` - Additional Cargo features (comma-separated)
- `-v`, `--verbose` - Enable verbose logging

**Examples:**
```bash
# Android with multiple architectures
cargo xtask build android --android-targets arm64-v8a,x86_64 --release

# Web with dev profile
cargo xtask build web

# Desktop for specific target
cargo xtask build desktop --desktop-target x86_64-apple-darwin --release
```

---

### `cargo xtask clean [OPTIONS]`

Clean build artifacts.

**Options:**
- `--all` - Clean all platforms
- `<PLATFORM>` - Clean specific platform: `android`, `web`, `desktop`

**Examples:**
```bash
# Clean everything
cargo xtask clean --all

# Clean Android artifacts
cargo xtask clean android
```

---

### `cargo xtask info`

Show environment information and installed tools.

**Example Output:**
```
=== FLUI Build System Environment ===

RUST:
  Version: rustc 1.91.0 (stable)
  Cargo: cargo 1.91.0

ANDROID:
  ANDROID_HOME: /path/to/android/sdk ✓
  NDK: Found ✓
  JAVA_HOME: /path/to/jdk ✓
  cargo-ndk: Installed ✓
  Android targets:
    - aarch64-linux-android ✓
    - x86_64-linux-android ✓

WEB:
  wasm-pack: Installed ✓
  wasm32-unknown-unknown: Installed ✓

DESKTOP:
  Platform: windows ✓
  Architecture: x86_64 ✓
```

---

## Cargo Aliases

For convenience, add these aliases to your shell:

```bash
# Or use the built-in .cargo/config.toml aliases:
cargo build-android          # Build Android debug
cargo build-android-release  # Build Android release
cargo build-web-release      # Build Web release
cargo build-desktop-release  # Build Desktop release
cargo info                   # Show environment info
cargo clean-all              # Clean everything
```

---

## CI/CD Integration

The build system is designed for CI/CD pipelines.

**GitHub Actions Example:**
```yaml
name: Multi-Platform Build

on: [push, pull_request]

jobs:
  build-android:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android
      - uses: android-actions/setup-android@v3
      - run: cargo install cargo-ndk
      - run: cargo xtask build android --release
      - uses: actions/upload-artifact@v4
        with:
          name: android-apk
          path: target/flui-out/android/flui-release.apk

  build-web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
      - run: cargo install wasm-pack
      - run: cargo xtask build web --release
      - uses: actions/upload-artifact@v4
        with:
          name: web-dist
          path: target/flui-out/web/

  build-desktop:
    strategy:
      matrix:
        os: [windows-latest, ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo xtask build desktop --release
      - uses: actions/upload-artifact@v4
        with:
          name: desktop-${{ runner.os }}
          path: target/flui-out/desktop/
```

---

## Build Performance

Typical build times on modern hardware:

| Platform | Profile | Cold Build | Incremental |
|----------|---------|------------|-------------|
| Android | Debug | 3-5 min | 30-60 sec |
| Android | Release | 5-8 min | 1-2 min |
| Web | Debug | 2-3 min | 30 sec |
| Web | Release | 4-6 min | 1 min |
| Desktop | Debug | 1-2 min | 15-30 sec |
| Desktop | Release | 3-5 min | 45 sec |

**Tips for Faster Builds:**
- Use `--jobs` to control parallelism: `cargo build --jobs 8`
- Enable `sccache` for distributed caching
- Use `lld` linker on Linux: `rustup component add llvm-tools-preview`

---

## Troubleshooting

### General Issues

**"Command not found" errors:**
- Ensure Rust is installed: `rustup --version`
- Check PATH includes `~/.cargo/bin`

**Slow builds:**
- Increase parallel jobs in `flui-build.toml`: `jobs = 8`
- Use `cargo clean` if builds seem corrupted

### Android Issues

**"NDK not found":**
1. Install NDK via Android Studio → SDK Manager → SDK Tools → NDK
2. Set `ANDROID_NDK_HOME` environment variable
3. Or specify in `flui-build.local.toml`

**"Library not found" linker errors:**
- Ensure correct NDK version (23+)
- Check that Android targets are installed
- Verify `jniLibs` directory is being populated

**APK installation fails:**
- Check device is connected: `adb devices`
- Enable USB debugging on device
- Try uninstalling previous version: `adb uninstall com.vanya.flui.counter`

### Web Issues

**WASM file is huge:**
- Use `--release` for size optimization
- Adjust `wasm-opt-level` in `flui-build.toml` (try "z" for maximum compression)
- Run `wasm-opt` manually: `wasm-opt -Oz input.wasm -o output.wasm`

**Blank page in browser:**
- Check browser console for errors
- Ensure WebGPU is supported (Chrome/Edge 113+, Firefox 130+)
- Try serving over HTTPS (WebGPU requires secure context)

### Desktop Issues

**"Linker not found":**
- Windows: Install Visual Studio Build Tools with C++ workload
- macOS: Install Xcode Command Line Tools: `xcode-select --install`
- Linux: Install GCC: `sudo apt install build-essential`

**Runtime errors:**
- Check dependencies are installed (GPU drivers, system libraries)
- Try debug build to get better error messages

---

## Next Steps

- Read `CLAUDE.md` for developer documentation
- Explore `platforms/README.md` for platform-specific details
- Check `flui_app/examples/` for example applications
- Join the community at [GitHub Discussions](https://github.com/your-repo/discussions)

---

## License

This build system is part of the FLUI project and follows the same license (MIT OR Apache-2.0).
