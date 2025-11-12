# FLUI CLI - Platform Support Status

## 🎯 Overview

FLUI CLI provides unified commands for building across multiple platforms. Some platforms use **xtask** integration, others use direct **cargo** builds.

---

## ✅ Fully Supported Platforms

### 1. **Android**
**Status**: ✅ **Production Ready** (via xtask)

```bash
# Debug build
flui build android

# Release build
flui build android --release

# Output
target/flui-out/android/flui-debug.apk
target/flui-out/android/flui-release.apk
```

**Integration**:
```
flui build android --release
    ↓
cargo xtask build android --release
    ↓
Android NDK + Gradle toolchain
    ↓
target/flui-out/android/flui-release.apk
```

**Requirements**:
- Android SDK
- Android NDK (r25+)
- Java JDK 11+
- Cargo NDK: `cargo install cargo-ndk`
- Rust target: `rustup target add aarch64-linux-android`

---

### 2. **Web (WASM)**
**Status**: ✅ **Production Ready** (via xtask)

```bash
# Debug build
flui build web

# Release build with optimizations
flui build web --release --optimize-wasm

# Output
target/flui-out/web/
```

**Integration**:
```
flui build web --release
    ↓
cargo xtask build web --release
    ↓
wasm-pack + wasm-opt
    ↓
target/flui-out/web/pkg/
```

**Requirements**:
- wasm-pack: `cargo install wasm-pack`
- Rust target: `rustup target add wasm32-unknown-unknown`
- Modern browser with WebGPU support

---

### 3. **Desktop**
**Status**: ✅ **Production Ready** (via xtask)

```bash
# Build for current platform
flui build desktop --release

# Output
target/flui-out/desktop/flui_app.exe (Windows)
target/flui-out/desktop/flui_app (Linux/macOS)
```

**Integration**:
```
flui build desktop --release
    ↓
cargo xtask build desktop --release
    ↓
Platform-specific cargo build
    ↓
target/flui-out/desktop/
```

**Auto-detects**:
- Windows → x86_64-pc-windows-msvc
- Linux → x86_64-unknown-linux-gnu
- macOS → x86_64-apple-darwin

---

### 4. **Windows (Specific)**
**Status**: ✅ **Supported** (direct cargo)

```bash
flui build windows --release
```

**Integration**:
```
flui build windows --release
    ↓
cargo build --target x86_64-pc-windows-msvc --release
```

---

### 5. **Linux (Specific)**
**Status**: ✅ **Supported** (direct cargo)

```bash
flui build linux --release
```

**Integration**:
```
flui build linux --release
    ↓
cargo build --target x86_64-unknown-linux-gnu --release
```

---

### 6. **macOS (Specific)**
**Status**: ✅ **Supported** (direct cargo)

```bash
flui build macos --release
```

**Integration**:
```
flui build macos --release
    ↓
cargo build --target x86_64-apple-darwin --release
```

---

## ⚠️ Partially Supported Platforms

### 7. **iOS**
**Status**: 🚧 **Not Yet Implemented**

```bash
flui build ios --release
# Error: iOS build via flui CLI not yet implemented
```

**Current workaround**:
```bash
# Option 1: Direct cargo build
cargo build --target aarch64-apple-ios --release

# Option 2: Use Xcode
# Open platforms/ios/ in Xcode and build
```

**Why not supported yet?**
- xtask doesn't have iOS builder yet
- iOS builds require complex Xcode integration
- Need signing and provisioning profiles

**Planned**:
- Add iOS support to xtask
- Integrate with `xcodebuild`
- Support for simulators and devices
- Automatic provisioning

---

## 📊 Platform Support Matrix

| Platform | CLI Command | Integration | Status | Output Location |
|----------|-------------|-------------|--------|-----------------|
| Android | `flui build android` | xtask | ✅ Ready | `target/flui-out/android/*.apk` |
| iOS | `flui build ios` | xtask | 🚧 Not Yet | N/A |
| Web | `flui build web` | xtask | ✅ Ready | `target/flui-out/web/` |
| Desktop | `flui build desktop` | xtask | ✅ Ready | `target/flui-out/desktop/` |
| Windows | `flui build windows` | cargo | ✅ Ready | `target/x86_64-pc-windows-msvc/release/` |
| Linux | `flui build linux` | cargo | ✅ Ready | `target/x86_64-unknown-linux-gnu/release/` |
| macOS | `flui build macos` | cargo | ✅ Ready | `target/x86_64-apple-darwin/release/` |

---

## 🔧 xtask Integration Details

### What platforms use xtask?

✅ **Android** - Complex NDK + Gradle setup
✅ **Web** - WASM packaging and optimization
✅ **Desktop** - Unified desktop builds
❌ **iOS** - Coming soon

### Why xtask for some platforms?

**xtask** is used when:
1. Multiple toolchains needed (NDK, wasm-pack, etc.)
2. Complex build steps (Gradle, wasm-opt, etc.)
3. Platform-specific configurations
4. Output packaging required

**Direct cargo** is used when:
1. Simple `cargo build --target` sufficient
2. No extra tooling required
3. Standard Rust cross-compilation works

---

## 🚀 Quickstart by Platform

### Android Development
```bash
# Setup
flui doctor --android

# Build & Run
flui build android --release
adb install target/flui-out/android/flui-release.apk
```

### Web Development
```bash
# Setup
flui doctor --web

# Build & Serve
flui build web --release --optimize-wasm
cd target/flui-out/web
python -m http.server 8080
```

### Desktop Development
```bash
# Build & Run
flui build desktop --release
./target/flui-out/desktop/flui_app
```

---

## 📝 Adding New Platform Support

To add a new platform:

1. **Add to xtask** (if complex build):
   - `xtask/src/builder/<platform>.rs`
   - Implement `PlatformBuilder` trait

2. **Add to flui CLI**:
   - Add enum variant to `BuildTarget`
   - Add build function in `commands/build.rs`
   - Update help text and documentation

3. **Test**:
   ```bash
   flui build <platform> --release
   ```

---

## 🔍 Checking Your Setup

```bash
# Check all platforms
flui doctor

# Check specific platform
flui doctor --android
flui doctor --web

# Show available devices
flui devices
```

---

## ❓ FAQ

**Q: Why can't I build for iOS?**
A: iOS support via xtask is not yet implemented. Use direct cargo or Xcode.

**Q: Can I cross-compile from Windows to Linux?**
A: Yes, use `flui build linux` with proper cross-compilation toolchain setup.

**Q: How do I build for multiple ABIs on Android?**
A: Use `flui build android --release --split-per-abi` (coming soon in xtask).

**Q: Can I customize xtask build options?**
A: Not yet directly via flui CLI. Run `cargo xtask build --help` for all options.

---

## 🛠 Troubleshooting

### Android build fails
```bash
# Check setup
flui doctor --android

# Ensure NDK is installed
echo $ANDROID_NDK_HOME

# Try direct xtask
cargo xtask build android --verbose
```

### Web build fails
```bash
# Check wasm-pack
wasm-pack --version

# Install if missing
cargo install wasm-pack

# Add target
rustup target add wasm32-unknown-unknown
```

---

**Last Updated**: 2025-11-11
**Version**: 0.1.0
