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
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
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

/// Android platform build support
pub mod android;
/// Type-state builder for `BuilderContext`
pub mod context_builder;
/// Extension trait with utility methods for `BuilderContext`
pub mod context_ext;
/// Desktop platform build support (Windows, macOS, Linux)
pub mod desktop;
/// Custom error types for build operations
pub mod error;
/// iOS platform build support
pub mod ios;
/// Output parsers for build tools (cargo, gradle, wasm-pack)
pub mod output_parser;
/// Platform abstractions and core types
pub mod platform;
/// Build progress tracking and reporting
pub mod progress;
/// Utility functions and helpers
pub mod util;
/// Web/WASM platform build support
pub mod web;

pub use android::AndroidBuilder;
pub use context_builder::BuilderContextBuilder;
pub use context_ext::BuilderContextExt;
pub use desktop::DesktopBuilder;
pub use error::{BuildError, BuildResult};
pub use ios::IOSBuilder;
pub use output_parser::{get_parser, BuildEvent, OutputParser};
pub use platform::*;
pub use progress::{BuildPhase, BuildProgress, ProgressManager};
pub use web::WebBuilder;
