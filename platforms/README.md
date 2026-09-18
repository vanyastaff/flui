# FLUI Platform-Specific Code

This directory contains platform-specific build configurations and native wrapper code for FLUI applications across different operating systems and environments.

## Overview

FLUI is a cross-platform UI framework built in Rust with **wgpu** for GPU-accelerated rendering. Native windowing lives in `crates/flui-platform` (Win32, AppKit, Android, winit fallback); this directory holds only the host build projects (Xcode, Gradle, CMake) that link the Rust library into a platform app. Build and test commands are the `justfile` recipes — run `just --list`.

## Supported Platforms

| Platform | Status | Rendering Backend | Build System |
|----------|--------|-------------------|--------------|
| **Android** | ✅ Production | Vulkan | Gradle + NDK |
| **iOS** | 🚧 Native backend in progress | Metal | Xcode |
| **Web** | ✅ Production | WebGPU | wasm-pack |
| **Windows** | 🚧 Win32 backend, lint-only in CI | DirectX 12 / Vulkan | CMake + MSVC |
| **Linux** | 🪟 winit fallback | Vulkan | cargo + `just` |
| **macOS** | ✅ Native AppKit backend | Metal | Xcode |

## Directory Structure

```
platforms/
├── android/          Android (NativeActivity + Rust)
│   ├── app/
│   │   ├── src/main/AndroidManifest.xml
│   │   └── build.gradle
│   ├── build.gradle
│   ├── settings.gradle
│   └── README.md
│
├── ios/              iOS (UIKit + Rust)
│   ├── FluiCounter/
│   │   ├── AppDelegate.swift
│   │   └── Info.plist
│   └── README.md
│
├── web/              WebAssembly + WebGPU
│   ├── index.html
│   ├── manifest.json
│   └── README.md
│
├── windows/          Windows Desktop (Win32 + Rust)
│   ├── CMakeLists.txt
│   ├── runner/
│   └── README.md
│
├── linux/            Linux Desktop (GTK3 + Rust)
│   ├── CMakeLists.txt
│   ├── runner/
│   └── README.md
│
└── macos/            macOS Desktop (Cocoa + Rust)
    ├── Runner/
    ├── Runner.xcodeproj/
    └── README.md
```

## Quick Start

### Prerequisites

**All platforms:**
- Rust toolchain (version pinned in `rust-toolchain.toml`)
- Platform-specific targets via rustup

### Building for Specific Platform

Build the Rust library with cargo, then the host project with its own tool:

```bash
# Android
cargo ndk -t arm64-v8a build -p flui-android-demo

# Web
cd examples/web_demo && wasm-pack build --target web --out-dir pkg

# iOS (simulator, Apple Silicon)
cargo build --target aarch64-apple-ios-sim -p flui-app

# macOS / desktop
cargo build --release
```

The macOS native backend's bundled probes are `just macos-frame-pump`,
`just macos-close-path`, `just macos-ime`, and `just macos-resize-jitter`;
run `just --list` for the full recipe set.

## wgpu Rendering Backends

FLUI uses [wgpu](https://wgpu.rs/) for cross-platform GPU rendering. The backend selected depends on the platform:

| Platform | Primary Backend | Fallback |
|----------|----------------|----------|
| Android | Vulkan | OpenGL ES 3.0 |
| iOS | Metal | - |
| Web | WebGPU | - |
| Windows | DirectX 12 | Vulkan → DX11 |
| Linux | Vulkan | OpenGL 3.3+ |
| macOS | Metal | - |

## Platform-Specific Features

### Mobile (Android/iOS)
- Touch input support
- Orientation changes
- Mobile GPU optimization
- App lifecycle management

*(Android is the implemented backend; the iOS half is Cross.P / P5. The bullet
list describes the target contract, not shipped iOS behavior.)*

### Web (WASM)
- WebGPU rendering
- Progressive Web App (PWA) support
- Responsive design
- Browser compatibility layer

### Desktop (Windows/Linux/macOS)
- Mouse and keyboard input
- Window management
- Multi-monitor support
- Native file dialogs
- System tray integration

## Development Workflow

### 1. Develop Core Logic in Rust
All business logic and UI code lives in `crates/flui_*`. Platform directories only contain thin wrappers.

### 2. Build Rust Library
```bash
# Android
cargo ndk -t arm64-v8a build --release

# iOS
cargo build --release --target aarch64-apple-ios

# Web
wasm-pack build --target web --release

# Desktop
cargo build --release --target <platform-target>
```

### 3. Build Platform Wrapper
Use platform-specific build tool (Gradle, Xcode, CMake, etc.)

### 4. Run Application
Deploy to device/emulator or run locally

## Common Issues

### "Library not found" errors
- Ensure Rust library is built for correct target architecture
- Check library search paths in platform build configuration
- Verify library naming (`.a`, `.so`, `.dll`, `.dylib`)

### Rendering issues
- Update GPU drivers
- Check wgpu backend support on device
- Enable fallback rendering backends

### Build errors
- Ensure all platform prerequisites are installed
- Check Rust target is installed: `rustup target list --installed`
- Clean build directories and rebuild

## Contributing

When adding platform support or features:

1. Keep platform-specific code minimal
2. Use Rust FFI for communication
3. Follow existing project structure
4. Update platform README with new features
5. Test on real devices when possible

## Platform Maintainers

- **Android**: Primary development platform
- **Web**: Production ready
- **macOS**: Native AppKit backend complete; lint-only in CI plus local bundled probes
- **iOS**: Native UIKit backend in progress (Cross.P / P5)
- **Windows**: Win32 backend present; needs a Windows session to close H9
- **Linux**: winit fallback in production use; native Wayland/X11 still open

## Resources

- [wgpu Documentation](https://wgpu.rs/)
- [Rust FFI Guide](https://doc.rust-lang.org/nomicon/ffi.html)
- [Android NDK Guide](https://developer.android.com/ndk/guides)
- [WebAssembly Guide](https://rustwasm.github.io/docs/book/)

## License

See root LICENSE file.
