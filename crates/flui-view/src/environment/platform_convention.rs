//! [`PlatformConvention`]: the platform whose conventions a subtree follows.

use flui_platform::TargetPlatform;

use crate::impl_inherited_view;
use crate::prelude::*;

/// word, which keys refresh and zoom, how a long press selects, which
/// physics a scroll view settles with, which page transition a route runs.
///
/// The one channel for that choice (ADR-0028's capability seam): a widget
/// reads [`PlatformConvention::of`] and never branches on
/// `TargetPlatform::current()` or a theme field. The runtime installs it
/// above every root with the platform its presentation names, so a web
/// build follows the platform the host reports, not the compile target.
/// A plain wrapper overrides it for a subtree, which is how a test or a
/// preview shows the iOS conventions on any machine:
///
/// ```rust,ignore
/// PlatformConvention::new(TargetPlatform::iOS, app)
/// ```
#[derive(Clone)]
pub struct PlatformConvention {
    platform: TargetPlatform,
    child: BoxedView,
}

impl PlatformConvention {
    /// Wrap `child` in a subtree that follows `platform`'s conventions.
    #[must_use]
    pub fn new(platform: TargetPlatform, child: impl IntoView) -> Self {
        Self {
            platform,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// The platform the nearest ancestor names, registering a dependency so
    /// the element rebuilds when it changes. Without one,
    /// [`TargetPlatform::Unknown`], whose conventions are the
    /// platform-neutral ones: a tree mounted outside a runtime never takes
    /// them from the machine it runs on.
    pub fn of(ctx: &dyn BuildContext) -> TargetPlatform {
        ctx.depend_on::<Self, _>(|convention| convention.platform)
            .unwrap_or(TargetPlatform::Unknown)
    }
}

impl std::fmt::Debug for PlatformConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformConvention")
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

impl InheritedView for PlatformConvention {
    type Data = TargetPlatform;

    fn data(&self) -> &Self::Data {
        &self.platform
    }

    fn child(&self) -> &BoxedView {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.platform != old.platform
    }
}

impl_inherited_view!(PlatformConvention);
