//! [`SystemGestureExclusion`] — keeps the OS's edge gestures out of its child's box.

use flui_objects::RenderInputRegion;
use flui_rendering::input_regions::InputRegionKind;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Asks the OS not to start its own edge gestures — Android's back and home swipes, iOS
/// screen-edge gestures — inside its child's box, so a drag the application handles there
/// (a drawer handle, a slider at the screen edge) is not taken by the system.
///
/// The box is mapped into the window through every transform and clip above it, as it is
/// painted; a rotated child excludes its bounding box. Each composited frame hands the
/// window its whole set, and the backend updates the OS only when the device-pixel set,
/// the scale or the display changed. Where the OS honours no such region the widget has
/// no effect (`InputCapabilities::input_regions` reports which).
///
/// ```ignore
/// SystemGestureExclusion::new().child(drawer_edge_handle)
/// ```
#[derive(Clone, Debug)]
pub struct SystemGestureExclusion {
    excluding: bool,
    child: Child,
}

impl Default for SystemGestureExclusion {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemGestureExclusion {
    /// Excludes system gestures over the child.
    #[must_use]
    pub fn new() -> Self {
        Self {
            excluding: true,
            child: Child::empty(),
        }
    }

    /// Whether the exclusion is in force; `false` keeps the child mounted and hands the
    /// edge back to the OS.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SystemGestureExclusion {
    type Protocol = BoxProtocol;
    type RenderObject = RenderInputRegion;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut region = RenderInputRegion::new(InputRegionKind::SystemGestureExclusion);
        let _ = region.set_enabled(self.excluding);
        region
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_enabled(self.excluding)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SystemGestureExclusion);
