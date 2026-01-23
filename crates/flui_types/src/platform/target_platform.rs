//! Target platform enum

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TargetPlatform {
    /// Android mobile platform
    Android,

    #[allow(non_camel_case_types)]
    iOS,

    /// macOS desktop platform
    MacOS,

    /// Linux desktop platform
    Linux,

    /// Windows desktop platform
    Windows,

    /// Fuchsia platform
    Fuchsia,

    /// Web platform
    Web,
}

impl TargetPlatform {
    #[must_use]
    pub const fn is_mobile(&self) -> bool {
        matches!(self, Self::Android | Self::iOS | Self::Fuchsia)
    }

    #[must_use]
    pub const fn is_desktop(&self) -> bool {
        matches!(self, Self::MacOS | Self::Linux | Self::Windows)
    }

    #[must_use]
    pub const fn is_web(&self) -> bool {
        matches!(self, Self::Web)
    }

    #[must_use]
    pub const fn is_apple(&self) -> bool {
        matches!(self, Self::iOS | Self::MacOS)
    }

    #[must_use]
    pub const fn is_touch_primary(&self) -> bool {
        self.is_mobile() || matches!(self, Self::Web)
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::iOS => "ios",
            Self::MacOS => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Fuchsia => "fuchsia",
            Self::Web => "web",
        }
    }
}

impl Default for TargetPlatform {
    fn default() -> Self {
        // Detect the current platform at compile time
        #[cfg(target_os = "android")]
        return Self::Android;

        #[cfg(target_os = "ios")]
        return Self::iOS;

        #[cfg(target_os = "macos")]
        return Self::MacOS;

        #[cfg(target_os = "linux")]
        return Self::Linux;

        #[cfg(target_os = "windows")]
        return Self::Windows;

        #[cfg(target_os = "fuchsia")]
        return Self::Fuchsia;

        #[cfg(target_arch = "wasm32")]
        return Self::Web;

        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "linux",
            target_os = "windows",
            target_os = "fuchsia",
            target_arch = "wasm32"
        )))]
        return Self::Linux; // fallback
    }
}

