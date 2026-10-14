//! What diagnostics name a controller by: a debug label and the site that
//! created it. Both are display fields only; correlating events stays on the
//! controller span's identity (ADR-0202).

use std::borrow::Cow;
use std::panic::Location;

/// Where a controller was created: the caller of
/// [`AnimationControllerBuilder::build`](crate::AnimationControllerBuilder::build)
/// or [`build_on`](crate::AnimationControllerBuilder::build_on). Kept only
/// with debug assertions or the `diagnostics` feature; zero-sized otherwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CreationSite(
    #[cfg(any(debug_assertions, feature = "diagnostics"))] &'static Location<'static>,
);

impl CreationSite {
    #[track_caller]
    pub(crate) fn caller() -> Self {
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        {
            Self(Location::caller())
        }
        #[cfg(not(any(debug_assertions, feature = "diagnostics")))]
        {
            Self()
        }
    }

    /// The recorded location, when this build keeps one.
    #[cfg_attr(
        any(debug_assertions, feature = "diagnostics"),
        expect(
            clippy::unnecessary_wraps,
            reason = "`None` in builds that keep no location"
        )
    )]
    #[cfg_attr(
        not(any(debug_assertions, feature = "diagnostics")),
        expect(clippy::unused_self, reason = "the site is zero-sized in this build")
    )]
    pub(crate) fn location(self) -> Option<&'static Location<'static>> {
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        {
            Some(self.0)
        }
        #[cfg(not(any(debug_assertions, feature = "diagnostics")))]
        {
            None
        }
    }
}

/// A controller's debug label and creation site, fixed at construction.
#[derive(Clone, Debug)]
pub(crate) struct ControllerOrigin {
    pub(crate) label: Option<Cow<'static, str>>,
    pub(crate) site: CreationSite,
}
