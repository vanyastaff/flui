//! [`FirstClick`] — lets the click that brings an inactive window forward act on its child.

use flui_objects::RenderInputRegion;
use flui_rendering::input_regions::InputRegionKind;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Lets the click that brings an inactive window forward also act on its child: a toolbar
/// button, a drag source, a control the user reaches for in a window behind.
///
/// Where the system leaves that click to the application (AppKit), it only activates the
/// window by default, as AppKit's own controls do; inside this widget's box it is
/// delivered too, as a press marked
/// [`is_first_click`](flui_platform::pointer::PointerButtonEvent::is_first_click), and
/// the gestures under it see it as any other press. Outside every such box the press, and
/// the rest of its contact, reach nothing. Where the system delivers or swallows that click
/// itself (Windows, X11, Wayland), the widget changes nothing.
///
/// The box is mapped into the window through every transform and clip above it, as it is
/// painted; a rotated child accepts the click over its bounding box.
///
/// ```ignore
/// FirstClick::new().child(toolbar_button)
/// ```
#[derive(Clone, Debug)]
pub struct FirstClick {
    accepting: bool,
    child: Child,
}

impl Default for FirstClick {
    fn default() -> Self {
        Self::new()
    }
}

impl FirstClick {
    /// Accepts the first click over the child.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accepting: true,
            child: Child::empty(),
        }
    }

    /// Whether the first click acts on the child; `false` keeps the child mounted and lets
    /// that click only activate the window there.
    #[must_use]
    pub fn accepting(mut self, accepting: bool) -> Self {
        self.accepting = accepting;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for FirstClick {
    type Protocol = BoxProtocol;
    type RenderObject = RenderInputRegion;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut region = RenderInputRegion::new(InputRegionKind::FirstClick);
        let _ = region.set_enabled(self.accepting);
        region
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_enabled(self.accepting)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(FirstClick);
