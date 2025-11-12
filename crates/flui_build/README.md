# flui_build

Build system library for the FLUI framework - handles cross-platform builds for Android, Web (WASM), iOS, and Desktop platforms.

## Overview

`flui_build` is the core build infrastructure library used by `flui_cli` to compile FLUI applications for multiple platforms. It provides a trait-based architecture with platform-specific builders.

## Architecture

```
┌─────────────────────┐
│  flui_cli           │  User-facing CLI
│  (uses flui_build)  │
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│  flui_build         │  Build library (this crate)
│  - PlatformBuilder  │  - Trait-based architecture
│  - AndroidBuilder   │  - Platform-specific logic
│  - WebBuilder       │  - Environment validation
│  - DesktopBuilder   │  - Artifact management
└─────────────────────┘
```

## Features

- **Trait-based architecture**: Common `PlatformBuilder` interface for all platforms
- **Android builds**: cargo-ndk + Gradle integration for APK generation
- **Web builds**: wasm-pack integration for WebAssembly
- **Desktop builds**: Native binary compilation for Windows/Linux/macOS
- **Environment validation**: Checks for required tools and SDKs
- **Clean artifact management**: Consistent output locations

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_build = "0.1"
```

### Building for Android

```rust
use flui_build::*;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    // Create build context
    let ctx = BuilderContext {
        workspace_root: PathBuf::from("."),
        platform: Platform::Android {
            targets: vec!["arm64-v8a".to_string()],
        },
        profile: Profile::Release,
        features: vec![],
        output_dir: PathBuf::from("target/flui-out/android"),
    };

    // Create Android builder
    let builder = AndroidBuilder::new(&ctx.workspace_root)?;

    // Validate environment
    builder.validate_environment()?;

    // Build Rust libraries
    let artifacts = builder.build_rust(&ctx)?;

    // Build final APK
    let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

    println!("Built APK: {:?}", final_artifacts.app_binary);
    println!("Size: {} bytes", final_artifacts.size_bytes);

    Ok(())
}
```

### Building for Web

```rust
use flui_build::*;

let ctx = BuilderContext {
    workspace_root: PathBuf::from("."),
    platform: Platform::Web {
        target: "web".to_string(),
    },
    profile: Profile::Release,
    features: vec![],
    output_dir: PathBuf::from("target/flui-out/web"),
};

let builder = WebBuilder::new(&ctx.workspace_root)?;
builder.validate_environment()?;
let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;
```

### Building for Desktop

```rust
use flui_build::*;

let ctx = BuilderContext {
    workspace_root: PathBuf::from("."),
    platform: Platform::Desktop {
        target: None, // Auto-detect host platform
    },
    profile: Profile::Release,
    features: vec![],
    output_dir: PathBuf::from("target/flui-out/desktop"),
};

let builder = DesktopBuilder::new(&ctx.workspace_root)?;
let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;
```

## Platform Requirements

### Android

- Android SDK (ANDROID_HOME environment variable)
- Android NDK (ANDROID_NDK_HOME or auto-detect from SDK)
- Java JDK 11+ (JAVA_HOME for Gradle APK builds)
- cargo-ndk: `cargo install cargo-ndk`
- Rust target: `rustup target add aarch64-linux-android`

### Web

- wasm-pack: `cargo install wasm-pack`
- Rust target: `rustup target add wasm32-unknown-unknown`
- Modern browser with WebGPU support

### Desktop

- Rust toolchain (automatically uses host target)
- Platform-specific build tools (MSVC/Xcode/GCC)

## Output Locations

All builders output to `target/flui-out/<platform>/`:

- **Android**: `target/flui-out/android/flui-{debug|release}.apk`
- **Web**: `target/flui-out/web/` (ready to serve)
- **Desktop**: `target/flui-out/desktop/flui_app[.exe]`

## API Reference

### PlatformBuilder Trait

```rust
pub trait PlatformBuilder: Send + Sync {
    fn platform_name(&self) -> &str;
    fn validate_environment(&self) -> Result<()>;
    fn build_rust(&self, ctx: &BuilderContext) -> Result<BuildArtifacts>;
    fn build_platform(&self, ctx: &BuilderContext, artifacts: &BuildArtifacts) -> Result<FinalArtifacts>;
    fn clean(&self, ctx: &BuilderContext) -> Result<()>;
}
```

### BuilderContext

Configuration for a build:

```rust
pub struct BuilderContext {
    pub workspace_root: PathBuf,
    pub platform: Platform,
    pub profile: Profile,
    pub features: Vec<String>,
    pub output_dir: PathBuf,
}
```

### Platform

Target platform to build for:

```rust
pub enum Platform {
    Android { targets: Vec<String> },
    Web { target: String },
    Desktop { target: Option<String> },
}
```

### Profile

Build profile:

```rust
pub enum Profile {
    Debug,
    Release,
}
```

## License

MIT OR Apache-2.0
