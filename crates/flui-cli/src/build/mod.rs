//! The build pipeline behind `flui build` and `flui run`: cross-platform
//! builds for Android, Web (WASM), iOS and desktop, plus platform scaffolding.
//! Formerly the `flui-build` crate; it has no consumer but this binary, so it
//! lives here as a module.
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
//! use crate::build::*;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create build context via builder
//!     let ctx = BuilderContextBuilder::new(PathBuf::from("."))
//!         .with_platform(Platform::Android {
//!             targets: vec!["arm64-v8a".to_string()],
//!         })
//!         .with_profile(Profile::Release)
//!         .build();
//!
//!     // Create Android builder
//!     let builder = AndroidBuilder::new(&ctx.workspace_root)?;
//!
//!     // Validate environment
//!     builder.validate_environment()?;
//!
//!     // Build Rust libraries
//!     let artifacts = builder.build_rust(&ctx).await?;
//!
//!     // Build final APK
//!     let final_artifacts = builder.build_platform(&ctx, &artifacts).await?;
//!
//!     println!("Built: {:?}", final_artifacts.app_binary);
//!     Ok(())
//! }
//! ```

// Ship bar (wave 4): every public item is documented; keep it that way.

/// Android platform build support
pub mod android;
/// Type-state builder for `BuilderContext`
pub mod context_builder;
/// Desktop platform build support (Windows, macOS, Linux)
pub mod desktop;
/// Custom error types for build operations
pub mod error;
/// iOS platform build support
pub mod ios;
mod ios_package;
/// Platform abstractions and core types
pub mod platform;
/// Platform scaffolding for new projects
pub mod scaffold;
/// Utility functions and helpers
pub(crate) mod util;

#[cfg(test)]
mod tests;
/// Web/WASM platform build support
pub mod web;

pub use android::AndroidBuilder;
pub use context_builder::BuilderContextBuilder;
pub use desktop::DesktopBuilder;
pub use ios::IosBuilder;
pub use platform::*;
pub use web::WebBuilder;
