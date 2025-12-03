//! Platform Abstraction and Detection - Simplified but Powerful
//!
//! This module provides compile-time platform detection and capabilities
//! with zero-cost abstractions while being simpler and more focused.

use std::marker::PhantomData;

/// Compile-time platform detection
pub const fn current_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    return "windows";

    #[cfg(target_os = "macos")]
    return "macos";

    #[cfg(target_os = "linux")]
    return "linux";

    #[cfg(target_os = "android")]
    return "android";

    #[cfg(target_os = "ios")]
    return "ios";

    #[cfg(target_arch = "wasm32")]
    return "web";

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )))]
    return "unknown";
}

/// Platform capabilities determined at compile time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub multiple_windows: bool,
    pub high_refresh_rate: bool,
    pub touch_input: bool,
    pub mouse_input: bool,
    pub keyboard_input: bool,
    pub compute_shaders: bool,
    pub hdr_displays: bool,
    pub file_system: bool,
    pub networking: bool,
}

impl PlatformCapabilities {
    pub const fn for_current() -> Self {
        match current_platform() {
            "windows" => Self::WINDOWS,
            "macos" => Self::MACOS,
            "linux" => Self::LINUX,
            "android" => Self::ANDROID,
            "ios" => Self::IOS,
            "web" => Self::WEB,
            _ => Self::UNKNOWN,
        }
    }

    const WINDOWS: Self = Self {
        multiple_windows: true,
        high_refresh_rate: true,
        touch_input: false,
        mouse_input: true,
        keyboard_input: true,
        compute_shaders: true,
        hdr_displays: true,
        file_system: true,
        networking: true,
    };

    const MACOS: Self = Self {
        multiple_windows: true,
        high_refresh_rate: true,
        touch_input: false,
        mouse_input: true,
        keyboard_input: true,
        compute_shaders: true,
        hdr_displays: true,
        file_system: true,
        networking: true,
    };

    const LINUX: Self = Self {
        multiple_windows: true,
        high_refresh_rate: true,
        touch_input: false,
        mouse_input: true,
        keyboard_input: true,
        compute_shaders: true,
        hdr_displays: false,
        file_system: true,
        networking: true,
    };

    const ANDROID: Self = Self {
        multiple_windows: false,
        high_refresh_rate: true,
        touch_input: true,
        mouse_input: false,
        keyboard_input: false,
        compute_shaders: false,
        hdr_displays: true,
        file_system: false,
        networking: true,
    };

    const IOS: Self = Self {
        multiple_windows: false,
        high_refresh_rate: true,
        touch_input: true,
        mouse_input: false,
        keyboard_input: false,
        compute_shaders: false,
        hdr_displays: true,
        file_system: false,
        networking: true,
    };

    const WEB: Self = Self {
        multiple_windows: false,
        high_refresh_rate: false,
        touch_input: true,
        mouse_input: true,
        keyboard_input: true,
        compute_shaders: false,
        hdr_displays: false,
        file_system: false,
        networking: true,
    };

    const UNKNOWN: Self = Self {
        multiple_windows: false,
        high_refresh_rate: false,
        touch_input: false,
        mouse_input: false,
        keyboard_input: false,
        compute_shaders: false,
        hdr_displays: false,
        file_system: false,
        networking: false,
    };
}

/// Platform family classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformFamily {
    Desktop,
    Mobile,
    Web,
    Unknown,
}

impl PlatformFamily {
    pub const fn current() -> Self {
        match current_platform() {
            "windows" | "macos" | "linux" => Self::Desktop,
            "android" | "ios" => Self::Mobile,
            "web" => Self::Web,
            _ => Self::Unknown,
        }
    }

    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::Mobile)
    }

    pub const fn is_desktop(self) -> bool {
        matches!(self, Self::Desktop)
    }

    pub const fn is_web(self) -> bool {
        matches!(self, Self::Web)
    }
}

/// Platform marker trait
pub trait Platform: Send + Sync + 'static {
    const NAME: &'static str;
    const CAPABILITIES: PlatformCapabilities;
    const FAMILY: PlatformFamily;
}

/// Current platform type (resolved at compile time)
#[derive(Debug, Clone, Copy)]
pub struct CurrentPlatform;

impl Platform for CurrentPlatform {
    const NAME: &'static str = current_platform();
    const CAPABILITIES: PlatformCapabilities = PlatformCapabilities::for_current();
    const FAMILY: PlatformFamily = PlatformFamily::current();
}

/// Platform information at runtime
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    pub name: &'static str,
    pub capabilities: PlatformCapabilities,
    pub family: PlatformFamily,
}

impl PlatformInfo {
    pub fn current() -> Self {
        Self {
            name: current_platform(),
            capabilities: PlatformCapabilities::for_current(),
            family: PlatformFamily::current(),
        }
    }
}

/// Execute platform-specific code
pub fn execute_on_current_platform<T>(app: T) -> !
where
    T: Send + Sync + 'static,
{
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        crate::platform::desktop::run(app)
    }

    #[cfg(target_os = "android")]
    {
        crate::platform::android::run(app)
    }

    #[cfg(target_os = "ios")]
    {
        crate::platform::ios::run(app)
    }

    #[cfg(target_arch = "wasm32")]
    {
        crate::platform::web::run(app)
    }

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )))]
    {
        compile_error!("Unsupported platform")
    }
}

// Platform-specific execution modules (stubs for now)
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub mod desktop {
    pub fn run<T>(_app: T) -> !
    where
        T: Send + Sync + 'static,
    {
        tracing::info!("Running on desktop platform");
        // Desktop execution logic would go here
        std::process::exit(0)
    }
}

#[cfg(target_os = "android")]
pub mod android {
    pub fn run<T>(_app: T) -> !
    where
        T: Send + Sync + 'static,
    {
        tracing::info!("Running on Android platform");
        // Android execution logic would go here
        std::process::exit(0)
    }
}

#[cfg(target_os = "ios")]
pub mod ios {
    pub fn run<T>(_app: T) -> !
    where
        T: Send + Sync + 'static,
    {
        tracing::info!("Running on iOS platform");
        // iOS execution logic would go here
        std::process::exit(0)
    }
}

#[cfg(target_arch = "wasm32")]
pub mod web {
    pub fn run<T>(_app: T) -> !
    where
        T: Send + Sync + 'static,
    {
        tracing::info!("Running on Web platform");
        // Web execution logic would go here
        std::process::abort()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = current_platform();
        assert!(!platform.is_empty());
        assert_ne!(platform, "unknown");
    }

    #[test]
    fn test_capabilities() {
        let caps = PlatformCapabilities::for_current();
        // All platforms should support networking
        assert!(caps.networking);
    }

    #[test]
    fn test_platform_family() {
        let family = PlatformFamily::current();

        match current_platform() {
            "windows" | "macos" | "linux" => {
                assert_eq!(family, PlatformFamily::Desktop);
                assert!(family.is_desktop());
            }
            "android" | "ios" => {
                assert_eq!(family, PlatformFamily::Mobile);
                assert!(family.is_mobile());
            }
            "web" => {
                assert_eq!(family, PlatformFamily::Web);
                assert!(family.is_web());
            }
            _ => {}
        }
    }

    #[test]
    fn test_compile_time_constants() {
        const PLATFORM: &str = current_platform();
        const CAPABILITIES: PlatformCapabilities = PlatformCapabilities::for_current();

        assert!(!PLATFORM.is_empty());
        // Capabilities exist (test passes if compiles)
        let _ = CAPABILITIES;
    }
}
