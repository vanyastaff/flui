//! Platform-specific types
//!
//! This module provides types for platform detection and configuration.

pub mod brightness;
pub mod locale;
pub mod orientation;
pub mod target_platform;

pub use brightness::Brightness;
pub use locale::Locale;
pub use orientation::DeviceOrientation;
pub use target_platform::TargetPlatform;

