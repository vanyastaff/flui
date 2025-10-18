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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Test that all exports are accessible
        let _ = TargetPlatform::Android;
        let _ = Brightness::Light;
        let _ = DeviceOrientation::PortraitUp;
        let locale = Locale::new("en", Some("US"));
        assert_eq!(locale.language(), "en");
    }
}
