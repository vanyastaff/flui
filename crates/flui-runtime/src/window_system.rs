//! What a host's windows are drawn in: a platform's window system, or a
//! browser's page. A host states it once (a runner from the target it was
//! built for, a headless host from what its primary window plays), and every
//! policy that differs between them reads it: the built-in capabilities, the
//! closes the application may refuse and how the session ends. Nothing that
//! reads a [`WindowSystem`] reads the build target.

use flui_platform::{SessionEndSupport, TargetPlatform};

/// What a host's windows are drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowSystem {
    /// The window system of the platform named: a runner's own, or the one a
    /// headless window plays.
    Platform(TargetPlatform),
    /// A browser's page, which no [`TargetPlatform`] names.
    Browser,
}

impl WindowSystem {
    /// The platform whose window system this is; `None` for a browser's page,
    /// which follows no platform's.
    #[must_use]
    pub fn platform(self) -> Option<TargetPlatform> {
        match self {
            Self::Platform(platform) => Some(platform),
            Self::Browser => None,
        }
    }

    /// How the session ends in these windows, given what the host's platform
    /// `reported`: a browser's page only tells (`pagehide`), whatever was
    /// reported, so a page's session end can never be held; a platform's
    /// window system ends it as reported.
    #[must_use]
    pub fn session_end_support(
        self,
        reported: Option<SessionEndSupport>,
    ) -> Option<SessionEndSupport> {
        match self {
            Self::Platform(_) => reported,
            Self::Browser => Some(SessionEndSupport::NotifyOnly),
        }
    }
}
