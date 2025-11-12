# FLUI Mobile Deployment

Complete guide for deploying FLUI applications to multiple platforms using the FLUI CLI.

## 📱 Supported Platforms

- ✅ **Desktop** - Windows, Linux, macOS
- ✅ **Android** - ARM64, ARMv7
- 🚧 **iOS** - Device (ARM64), Simulator (ARM64 + x86_64) *(coming soon)*
- ✅ **Web** - WebAssembly with WebGPU

---

## 🚀 Quick Start

### Using FLUI CLI (Recommended)

The FLUI CLI provides a unified interface for building across all platforms.

#### Desktop

```bash
# Run directly
cargo run -p flui_app --example counter_demo

# Or build with flui_cli
flui build --platform desktop --example counter_demo --release
flui run --platform desktop
```

#### Android

```bash
# Build APK
flui build --platform android --example counter_demo --release

# Install to device
flui install --platform android

# Run on device
flui run --platform android
```

#### Web

```bash
# Build WASM package
flui build --platform web --example counter_demo --release

# Serve and open browser
flui run --platform web
```

---

## 📂 Project Structure

```
flui/
├── crates/
│   ├── flui_build/              # Build system library
│   │   ├── src/
│   │   │   ├── android.rs       # Android builder
│   │   │   ├── web.rs           # Web builder
│   │   │   ├── desktop.rs       # Desktop builder
│   │   │   └── platform.rs      # Common trait
│   │   └── Cargo.toml
│   │
│   ├── flui_cli/                # CLI tool
│   │   ├── src/
│   │   │   ├── commands/
│   │   │   │   ├── build.rs
│   │   │   │   ├── run.rs
│   │   │   │   └── install.rs
│   │   │   └── main.rs
│   │   └── Cargo.toml
│   │
│   └── flui_app/
│       └── examples/
│           └── counter_demo.rs  # ✨ Universal example
│
└── platforms/                   # Platform configurations
    ├── android/
    │   ├── app/
    │   │   ├── src/main/
    │   │   │   ├── AndroidManifest.xml
    │   │   │   └── jniLibs/     # Native libraries (.so)
    │   │   └── build.gradle.kts
    │   ├── build.gradle.kts
    │   ├── settings.gradle.kts
    │   ├── gradlew              # Gradle wrapper (Unix)
    │   └── gradlew.bat          # Gradle wrapper (Windows)
    │
    └── web/
        ├── index.html
        └── pkg/                 # Generated WASM files
```

---

## 🔧 Prerequisites

### All Platforms

- **Rust** 1.90+ ([rustup.rs](https://rustup.rs))
- **Cargo** (comes with Rust)
- **FLUI CLI** - `cargo install --path crates/flui_cli`

### Android

- **Android SDK** (Android Studio or command-line tools)
  - Set `ANDROID_HOME` environment variable
- **Android NDK** (will auto-detect from SDK)
- **Java JDK 11+** for Gradle
  - Set `JAVA_HOME` environment variable
- **cargo-ndk** - `cargo install cargo-ndk`
- **Rust target** - `rustup target add aarch64-linux-android`

### Web

- **wasm-pack** - `cargo install wasm-pack`
- **Rust target** - `rustup target add wasm32-unknown-unknown`
- Modern browser with WebGPU support (Chrome 113+, Edge 113+)

### Desktop

- Platform-specific build tools:
  - **Windows**: MSVC (Visual Studio Build Tools)
  - **Linux**: GCC/Clang
  - **macOS**: Xcode Command Line Tools

---

## 📦 Build Commands

### Build

Build for a specific platform:

```bash
# Debug build
flui build --platform android --example counter_demo

# Release build (optimized)
flui build --platform android --example counter_demo --release

# Specific Android targets
flui build --platform android --target arm64-v8a --release
flui build --platform android --target armeabi-v7a --release
```

### Install

Install built package to device:

```bash
# Android - installs APK to connected device
flui install --platform android

# Web - starts local server
flui install --platform web
```

### Run

Run the application:

```bash
# Android - install + launch on device
flui run --platform android

# Web - build + serve + open browser
flui run --platform web

# Desktop - build + run
flui run --platform desktop
```

### Clean

Clean build artifacts:

```bash
# Clean all platforms
flui clean

# Clean specific platform
flui clean --platform android
```

---

## 🔍 Environment Validation

Check if your environment is ready for building:

```bash
# Check all platforms
flui doctor

# Check specific platform
flui doctor --platform android
flui doctor --platform web
```

Output example:
```
✓ Rust toolchain (1.90.0)
✓ cargo-ndk (3.5.0)
✓ Android SDK (/Users/you/Library/Android/sdk)
✓ Android NDK (27.0.12077973)
✓ Java JDK (17.0.2)
✓ Gradle (8.5)
✗ iOS tools (Xcode not installed)
```

---

## 🛠 Platform-Specific Details

### Android

#### Architecture

```
Rust Code (counter_demo.rs)
    ↓ cargo-ndk
Native Library (.so)
    ↓ copied to jniLibs/
Android Project (Gradle)
    ↓ gradlew assembleRelease
APK (flui-release.apk)
```

#### Output Locations

- Native libraries: `platforms/android/app/src/main/jniLibs/arm64-v8a/libcounter_demo.so`
- APK: `target/flui-out/android/flui-release.apk`

#### Supported ABIs

- `arm64-v8a` - Modern 64-bit ARM devices (primary)
- `armeabi-v7a` - Older 32-bit ARM devices

#### Common Issues

**Error: ANDROID_HOME not set**
```bash
# Windows
set ANDROID_HOME=C:\Users\YourName\AppData\Local\Android\Sdk

# Linux/macOS
export ANDROID_HOME=$HOME/Library/Android/sdk
```

**Error: cargo-ndk not found**
```bash
cargo install cargo-ndk
```

**Error: Rust target not installed**
```bash
rustup target add aarch64-linux-android
```

### Web

#### Architecture

```
Rust Code (counter_demo.rs)
    ↓ wasm-pack
WASM Package (pkg/)
    ├── counter_demo_bg.wasm
    ├── counter_demo.js
    └── package.json
    ↓ flui run --platform web
Local Server (http://localhost:8080)
```

#### Output Locations

- WASM files: `platforms/web/pkg/`
- Static files: `platforms/web/index.html`

#### Browser Support

- Chrome 113+ (stable WebGPU)
- Edge 113+ (stable WebGPU)
- Firefox 118+ (experimental, enable `dom.webgpu.enabled`)
- Safari 18+ (experimental)

#### Common Issues

**Error: wasm-pack not found**
```bash
cargo install wasm-pack
```

**Error: WebGPU not supported**
- Use Chrome 113+ or Edge 113+
- Check `chrome://flags` - ensure WebGPU is enabled

### Desktop

#### Architecture

```
Rust Code (counter_demo.rs)
    ↓ cargo build
Native Binary
    └── target/release/flui_app[.exe]
```

#### Output Locations

- **Windows**: `target/flui-out/desktop/flui_app.exe`
- **Linux**: `target/flui-out/desktop/flui_app`
- **macOS**: `target/flui-out/desktop/flui_app`

---

## 🧪 Testing

### Manual Testing

```bash
# Build and run on Android device
flui run --platform android

# Build and serve Web locally
flui run --platform web

# Build and run Desktop
cargo run -p flui_app --example counter_demo
```

### Automated Testing

```bash
# Run unit tests
cargo test --workspace

# Run with logging
RUST_LOG=debug flui run --platform android
```

---

## 📊 Build Performance

Typical build times (Apple M1, Release mode):

| Platform | First Build | Incremental |
|----------|-------------|-------------|
| Desktop  | ~2 min      | ~10 sec     |
| Android  | ~5 min      | ~30 sec     |
| Web      | ~3 min      | ~20 sec     |

---

## 🔗 Resources

- **FLUI Documentation**: `README.md`
- **flui_build API**: `crates/flui_build/README.md`
- **flui_cli Guide**: `crates/flui_cli/FLUI_CLI_DOCUMENTATION.md`
- **Example Code**: `crates/flui_app/examples/counter_demo.rs`

---

## 📝 License

MIT OR Apache-2.0
