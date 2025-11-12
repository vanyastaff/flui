//! FLUI Build System Library
//!
//! This library provides the core build infrastructure for the FLUI framework,
//! supporting cross-platform builds for Android, Web (WASM), iOS, and Desktop platforms.
//!
//! # Architecture
//!
//! The build system uses a trait-based architecture with platform-specific builders:
//!
//! - `PlatformBuilder` trait: Common interface for all platforms
//! - `AndroidBuilder`: Builds APKs using cargo-ndk and Gradle
//! - `WebBuilder`: Builds WASM packages using wasm-pack
//! - `DesktopBuilder`: Builds native desktop applications
//!
//! # Usage
//!
//! ```rust,no_run
//! use flui_build::*;
//! use std::path::PathBuf;
//!
//! # fn main() -> anyhow::Result<()> {
//! // Create build context
//! let ctx = BuilderContext {
//!     workspace_root: PathBuf::from("."),
//!     platform: Platform::Android {
//!         targets: vec!["arm64-v8a".to_string()],
//!     },
//!     profile: Profile::Release,
//!     features: vec![],
//!     output_dir: PathBuf::from("target/flui-out/android"),
//! };
//!
//! // Create Android builder
//! let builder = AndroidBuilder::new(&ctx.workspace_root)?;
//!
//! // Validate environment
//! builder.validate_environment()?;
//!
//! // Build Rust libraries
//! let artifacts = builder.build_rust(&ctx)?;
//!
//! // Build final APK
//! let final_artifacts = builder.build_platform(&ctx, &artifacts)?;
//!
//! println!("Built: {:?}", final_artifacts.app_binary);
//! # Ok(())
//! # }
//! ```

pub mod platform;
pub mod android;
pub mod web;
pub mod desktop;
pub mod util;

pub use platform::*;
pub use android::AndroidBuilder;
pub use web::WebBuilder;
pub use desktop::DesktopBuilder;
