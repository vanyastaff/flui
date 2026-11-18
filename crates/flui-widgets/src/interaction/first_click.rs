//! [`FirstClick`] — lets the click that brings an inactive window forward act on its child.

use flui_objects::RenderInputRegion;
use flui_rendering::input_regions::InputRegionKind;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Lets the click that brings an inactive window forward also act on its child: a toolbar
/// button, a drag source, a control the user reaches for in a window behind.
///
/// On macOS that click only activates the window by default, as AppKit's own controls do.
/// A press marked
/// [`is_first_click`](flui_platform::pointer::PointerButtonEvent::is_first_click) reaches
/// the widgets only when its hit path runs through this widget: the press lands in the
/// child, and no widget painted over the child took the hit first. The gestures on that
/// path then see it as any other press. Anywhere else the press, and the rest of its
/// contact, reach nothing. A widget painted over the child that lets presses through
/// (`HitTestBehavior::Translucent`) shares the press with the child.
///
/// The host reads the box, mapped into the window through every transform and clip above
/// it as it is painted (a rotated child counts as its bounding box), only as a rough
/// pre-filter for which presses AppKit delivers; the hit path decides. On Windows, X11 and
/// Wayland FLUI delivers the activating click unmarked, as applications there do (Win32's
/// `WM_MOUSEACTIVATE` could swallow it with `MA_ACTIVATEANDEAT`; FLUI does not), so the
/// widget changes nothing there.
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
