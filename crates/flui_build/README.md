# flui_build

Build system library for the FLUI framework - handles cross-platform builds for Android, iOS, Web (WASM), and Desktop platforms.

## Overview

`flui_build` is the core build infrastructure library used by `flui_cli` to compile FLUI applications for multiple platforms. It provides a trait-based architecture with platform-specific builders and unified progress reporting.

## Features

### 🏗️ Platform Support
- **Android builds**: cargo-ndk + Gradle integration for APK generation
- **iOS builds**: cargo + Xcode integration for .app bundles
- **Web builds**: wasm-pack integration for WebAssembly
- **Desktop builds**: Native binary compilation for Windows/Linux/macOS

### 📊 Progress Reporting
- Beautiful progress bars with `indicatif`
- Real-time build status for each platform
- Phase tracking (Validate → Build Rust → Build Platform → Clean)
- Output parsing for cargo, gradle, wasm-pack, xcodebuild
- Verbose mode for detailed logging

### 🎯 Advanced Features
- **Trait-based architecture**: Common `PlatformBuilder` interface
- **Type-state builder pattern**: Compile-time validation
- **Sealed traits**: Future-proof API design
- **Custom error types**: Structured error handling with `thiserror`
- **Extension traits**: Convenient helper methods
- **Parallel builds**: MultiProgress coordination

## Architecture

```
┌─────────────────────┐
│  flui_cli           │  User-facing CLI
│  (uses flui_build)  │
└──────────┬──────────┘
           │
           ↓
┌─────────────────────────────────────────┐
│  flui_build                             │
│  ┌───────────────────────────────────┐  │
│  │  PlatformBuilder Trait (Sealed)   │  │
│  └───────────────────────────────────┘  │
│     ↑        ↑        ↑        ↑        │
│     │        │        │        │        │
│  Android   iOS     Web    Desktop       │
│  Builder  Builder Builder Builder       │
│                                          │
│  ┌─────────────────────────────────┐   │
│  │  Progress Tracking & Parsing    │   │
│  │  - BuildProgress                │   │
│  │  - OutputParser (cargo/gradle)  │   │
│  └─────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_build = "0.1"
```

## Quick Start

### Using Type-State Builder (Recommended)

```rust
use flui_build::*;
use std::path::PathBuf;

fn main() -> BuildResult<()> {
    // Type-safe builder with compile-time validation
    let ctx = BuilderContextBuilder::new(PathBuf::from("."))
        .with_platform(Platform::Android {
            targets: vec!["aarch64-linux-android".to_string()],
        })
        .with_profile(Profile::Release)
        .with_feature("audio".to_string())
        .build();

    // Create builder and build
    let builder = AndroidBuilder::new(&ctx.workspace_root)?;
    builder.validate_environment()?;

    let artifacts = builder.build_rust(&ctx)?;
    let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

    println!("Built: {:?} ({} bytes)",
        final_artifacts.app_binary,
        final_artifacts.size_bytes
    );

    Ok(())
}
```

### With Progress Reporting

```rust
use flui_build::{*, progress::*};

#[tokio::main]
async fn main() -> BuildResult<()> {
    let manager = ProgressManager::new();
    let mut progress = manager.create_build("Android");

    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking tools..."));
    // ... validation code ...
    progress.finish_phase("Environment validated");
    progress.set_progress(25);

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling..."));
    // ... build code ...
    progress.finish_phase("Rust libraries built");
    progress.set_progress(60);

    // Build Platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Building APK..."));
    // ... platform build ...
    progress.finish_phase("APK created");
    progress.set_progress(100);

    progress.finish("Build completed!");
    Ok(())
}
```

## Platform-Specific Examples

### Android

```rust
use flui_build::*;

let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    .with_platform(Platform::Android {
        targets: vec![
            "aarch64-linux-android".to_string(),
            "armv7-linux-androideabi".to_string(),
        ],
    })
    .with_profile(Profile::Release)
    .build();

let builder = AndroidBuilder::new(&ctx.workspace_root)?;
builder.validate_environment()?;

let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

println!("APK: {:?}", final_artifacts.app_binary);
```

### iOS

```rust
use flui_build::*;

let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    .with_platform(Platform::IOS {
        targets: vec![
            "aarch64-apple-ios".to_string(),      // Device
            "x86_64-apple-ios".to_string(),       // Simulator (Intel)
            "aarch64-apple-ios-sim".to_string(),  // Simulator (M1/M2)
        ],
    })
    .with_profile(Profile::Release)
    .build();

let builder = IOSBuilder::new(&ctx.workspace_root)?;
builder.validate_environment()?;

let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

println!(".app bundle: {:?}", final_artifacts.app_binary);
```

### Web

```rust
use flui_build::*;

let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    .with_platform(Platform::Web {
        target: "web".to_string(),
    })
    .with_profile(Profile::Release)
    .build();

let builder = WebBuilder::new(&ctx.workspace_root)?;
builder.validate_environment()?;

let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

println!("Web build: {:?}", final_artifacts.app_binary);
```

### Desktop

```rust
use flui_build::*;

let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    .with_platform(Platform::Desktop {
        target: None, // Auto-detect (x86_64-pc-windows-msvc, etc.)
    })
    .with_profile(Profile::Release)
    .build();

let builder = DesktopBuilder::new(&ctx.workspace_root)?;
let artifacts = builder.build_rust(&ctx)?;
let final_artifacts = builder.build_platform(&ctx, &artifacts)?;

println!("Native lib: {:?}", final_artifacts.app_binary);
```

## Platform Requirements

### Android

- **Android SDK**: Set `ANDROID_HOME` environment variable
- **Android NDK**: Set `ANDROID_NDK_HOME` or auto-detect from SDK
- **Java JDK 11+**: Set `JAVA_HOME` (for Gradle APK builds)
- **cargo-ndk**: `cargo install cargo-ndk`
- **Rust targets**:
  ```bash
  rustup target add aarch64-linux-android
  rustup target add armv7-linux-androideabi
  rustup target add i686-linux-android
  rustup target add x86_64-linux-android
  ```

### iOS

- **Xcode**: Latest version with command-line tools
- **xcodebuild**: Automatically included with Xcode
- **Rust targets**:
  ```bash
  rustup target add aarch64-apple-ios          # Device
  rustup target add x86_64-apple-ios           # Simulator (Intel)
  rustup target add aarch64-apple-ios-sim      # Simulator (M1/M2)
  ```

### Web

- **wasm-pack**: `cargo install wasm-pack`
- **Rust target**: `rustup target add wasm32-unknown-unknown`
- **Modern browser**: With WebGPU support

### Desktop

- **Rust toolchain**: Automatically uses host target
- **Platform-specific tools**:
  - Windows: MSVC Build Tools
  - macOS: Xcode Command Line Tools
  - Linux: GCC/Clang

## Output Locations

All builders output to `target/flui-out/<platform>/`:

- **Android**: `target/flui-out/android/flui-{debug|release}.apk`
- **iOS**: `target/flui-out/ios/flui.app`
- **Web**: `target/flui-out/web/` (ready to serve with HTTP server)
- **Desktop**: `target/flui-out/desktop/libflui_app.{dll|dylib|so}`

## Extension Traits

`BuilderContextExt` provides convenient helper methods:

```rust
use flui_build::*;

let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    .with_platform(Platform::Android { targets: vec![] })
    .with_profile(Profile::Release)
    .build();

// Platform checks
assert!(ctx.is_android());
assert!(!ctx.is_ios());
assert!(!ctx.is_web());
assert!(!ctx.is_desktop());

// Profile checks
assert!(ctx.is_release());
assert!(!ctx.is_debug());

// Feature helpers
ctx.has_feature("audio");
ctx.feature_count();

// Path helpers
let output = ctx.platform_output_dir();
let cargo_args = ctx.cargo_args();
```

## Output Parsing

Automatically parse build tool output:

```rust
use flui_build::output_parser::*;

let parser = get_parser("cargo");

if let Some(event) = parser.parse_line("   Compiling flui_app v0.1.0") {
    match event {
        BuildEvent::Started { task } => println!("Started: {}", task),
        BuildEvent::Completed { task, duration_ms } => {
            println!("Completed: {} ({:?}ms)", task, duration_ms)
        }
        BuildEvent::Warning { message } => eprintln!("Warning: {}", message),
        BuildEvent::Error { message } => eprintln!("Error: {}", message),
        _ => {}
    }
}
```

Supported parsers:
- **CargoParser**: Rust compilation output
- **GradleParser**: Android Gradle builds
- **WasmPackParser**: WASM builds
- **XcodeParser**: iOS builds

## Error Handling

Structured errors with `thiserror`:

```rust
use flui_build::{BuildError, BuildResult};

match build() {
    Err(BuildError::ToolNotFound { tool, install_hint }) => {
        eprintln!("{} not found", tool);
        eprintln!("Install with: {}", install_hint);
    }
    Err(BuildError::TargetNotInstalled { target, install_cmd }) => {
        eprintln!("Target {} not installed", target);
        eprintln!("Install with: {}", install_cmd);
    }
    Err(BuildError::CommandFailed { command, exit_code, stderr }) => {
        eprintln!("Command '{}' failed with code {}", command, exit_code);
        eprintln!("Error: {}", stderr);
    }
    Err(BuildError::PathNotFound { path, context }) => {
        eprintln!("Path not found: {:?}", path);
        eprintln!("Context: {}", context);
    }
    _ => {}
}
```

## API Reference

### PlatformBuilder Trait (Sealed)

```rust
pub trait PlatformBuilder: Send + Sync {
    fn platform_name(&self) -> &'static str;
    fn validate_environment(&self) -> BuildResult<()>;
    fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts>;
    fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts
    ) -> BuildResult<FinalArtifacts>;
    fn clean(&self, ctx: &BuilderContext) -> BuildResult<()>;
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
    IOS { targets: Vec<String> },
    Web { target: String },
    Desktop { target: Option<String> },
}
```

### Profile

Build profile:

```rust
#[derive(Default)]
pub enum Profile {
    #[default]
    Debug,
    Release,
}
```

## Examples

See `examples/` directory:

- `progress_demo.rs` - Progress indicators for parallel builds
- More examples coming soon!

## Development

Run tests:
```bash
cargo test -p flui_build
```

Run examples:
```bash
cargo run --example progress_demo
```

Check code quality:
```bash
cargo clippy -p flui_build
cargo fmt --check
```

## License

MIT OR Apache-2.0
