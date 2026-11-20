//! The macOS release the lane runs on, and the calls it gates.
//!
//! FLUI runs on macOS 10.13 on Intel (`platform-support.toml`), and the lane's newer calls are
//! gates, never a floor: a symbol a release lacks is never called there. The privacy permissions
//! the lane asks the system about (`CGPreflightPostEventAccess` and its kin) came with 10.15, and
//! before it posting events needed only the accessibility trust and listening needed nothing.
//! ScreenCaptureKit's one-shot window capture came with 14.0.

/// A macOS release, by its major and minor version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacosVersion {
    /// The major version: `10`, `11`, `14`.
    pub major: u32,
    /// The minor version.
    pub minor: u32,
}

impl MacosVersion {
    /// The release `major.minor`.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// The release `text` names, as `sw_vers` and the `kern.osproductversion` sysctl spell it:
    /// `14.5`, `10.15.7`, `26.0`; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = match parts.next() {
            Some(minor) => minor.parse().ok()?,
            None => 0,
        };
        if let Some(patch) = parts.next() {
            patch.parse::<u32>().ok()?;
        }
        parts.next().is_none().then_some(Self { major, minor })
    }

    /// Whether the system answers the privacy-permission preflights here.
    #[must_use]
    pub fn has_privacy_preflight(self) -> bool {
        self >= PRIVACY_PREFLIGHT
    }

    /// Whether ScreenCaptureKit captures one window's image here.
    #[must_use]
    pub fn has_window_screenshot(self) -> bool {
        self >= WINDOW_SCREENSHOT
    }
}

/// The release that brought the privacy-permission preflights.
pub const PRIVACY_PREFLIGHT: MacosVersion = MacosVersion::new(10, 15);

/// The release that brought `SCScreenshotManager`.
pub const WINDOW_SCREENSHOT: MacosVersion = MacosVersion::new(14, 0);
