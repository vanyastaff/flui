//! Empty widget - a placeholder that renders nothing

use flui_core::view::{BuildContext, IntoElement, View};
use flui_rendering::objects::RenderEmpty;

/// A widget that renders nothing but takes up space
///
/// This is useful as a placeholder or spacer in layouts.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::Empty;
///
/// // Use as a placeholder
/// let placeholder = Empty;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Empty;

impl View for Empty {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Returns a leaf render that does nothing
        (RenderEmpty, ())
    }
}
