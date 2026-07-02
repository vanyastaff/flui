//! [`MouseRegion`] — hover/cursor interaction widget.

use std::sync::Arc;

use flui_objects::RenderMouseRegion;
use flui_rendering::hit_testing::{
    CursorIcon, DeviceId, HitTestBehavior, MouseEnterCallback, MouseExitCallback,
    MouseHoverCallback,
};
use flui_rendering::protocol::BoxProtocol;
use flui_types::Offset;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Calls callbacks when the mouse enters, hovers within, or exits its bounds.
///
/// Flutter parity: `widgets/basic.dart` `MouseRegion` over
/// `RenderMouseRegion`. Layout and paint are pass-through when a child exists;
/// without a child the region grows to the incoming biggest constraint.
#[derive(Clone)]
pub struct MouseRegion {
    on_enter: Option<MouseEnterCallback>,
    on_hover: Option<MouseHoverCallback>,
    on_exit: Option<MouseExitCallback>,
    cursor: CursorIcon,
    opaque: bool,
    behavior: HitTestBehavior,
    child: Child,
}

impl Default for MouseRegion {
    fn default() -> Self {
        Self {
            on_enter: None,
            on_hover: None,
            on_exit: None,
            cursor: CursorIcon::Default,
            opaque: true,
            behavior: HitTestBehavior::Opaque,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for MouseRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseRegion")
            .field("on_enter", &self.on_enter.is_some())
            .field("on_hover", &self.on_hover.is_some())
            .field("on_exit", &self.on_exit.is_some())
            .field("cursor", &self.cursor)
            .field("opaque", &self.opaque)
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl MouseRegion {
    /// Creates an opaque mouse region with no callbacks and the default cursor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when the mouse enters this region.
    #[must_use]
    pub fn on_enter(mut self, callback: impl Fn(DeviceId, Offset) + Send + Sync + 'static) -> Self {
        self.on_enter = Some(Arc::new(callback));
        self
    }

    /// Called when the mouse moves within this region.
    #[must_use]
    pub fn on_hover(mut self, callback: impl Fn(DeviceId, Offset) + Send + Sync + 'static) -> Self {
        self.on_hover = Some(Arc::new(callback));
        self
    }

    /// Called when the mouse exits this region.
    #[must_use]
    pub fn on_exit(mut self, callback: impl Fn(DeviceId, Offset) + Send + Sync + 'static) -> Self {
        self.on_exit = Some(Arc::new(callback));
        self
    }

    /// Sets the mouse cursor reported while this region is active.
    #[must_use]
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = cursor;
        self
    }

    /// Sets whether the region should block mouse regions visually behind it.
    #[must_use]
    pub fn opaque(mut self, opaque: bool) -> Self {
        self.opaque = opaque;
        self
    }

    /// Sets hit-test behavior. Defaults to [`HitTestBehavior::Opaque`].
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Sets the child whose hover bounds are observed.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn configure(&self, render_object: &mut RenderMouseRegion) {
        render_object.set_on_enter(self.on_enter.clone());
        render_object.set_on_hover(self.on_hover.clone());
        render_object.set_on_exit(self.on_exit.clone());
        render_object.set_cursor(self.cursor);
        render_object.set_opaque(self.opaque);
        render_object.set_behavior(self.behavior);
    }
}

impl RenderView for MouseRegion {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMouseRegion;

    fn create_render_object(&self) -> Self::RenderObject {
        let mut render_object = RenderMouseRegion::new();
        self.configure(&mut render_object);
        render_object
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        self.configure(render_object);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(MouseRegion);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_match_flutter_opaque_defaults() {
        let render_object = MouseRegion::new().create_render_object();
        assert_eq!(render_object.cursor(), CursorIcon::Default);
        assert!(render_object.opaque());
        assert_eq!(render_object.behavior(), HitTestBehavior::Opaque);
    }

    #[test]
    fn create_render_object_applies_overridden_cursor_opaque_and_behavior() {
        let render_object = MouseRegion::new()
            .cursor(CursorIcon::Pointer)
            .opaque(false)
            .behavior(HitTestBehavior::Translucent)
            .create_render_object();

        assert_eq!(render_object.cursor(), CursorIcon::Pointer);
        assert!(!render_object.opaque());
        assert_eq!(render_object.behavior(), HitTestBehavior::Translucent);
    }

    #[test]
    fn update_render_object_reconfigures_cursor_opaque_and_behavior() {
        let mut render_object = MouseRegion::new().create_render_object();

        MouseRegion::new()
            .cursor(CursorIcon::Text)
            .opaque(false)
            .behavior(HitTestBehavior::DeferToChild)
            .update_render_object(&mut render_object);

        assert_eq!(render_object.cursor(), CursorIcon::Text);
        assert!(!render_object.opaque());
        assert_eq!(render_object.behavior(), HitTestBehavior::DeferToChild);
    }

    #[test]
    fn debug_reports_callback_presence_and_configuration_without_child() {
        let widget = MouseRegion::new().on_hover(|_device, _offset| {});
        let debug = format!("{widget:?}");
        assert!(
            debug.contains("on_hover: true") && debug.contains("on_enter: false"),
            "Debug output must reflect which callbacks are set, got: {debug}",
        );
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!MouseRegion::new().has_children());
        assert!(MouseRegion::new().child(SizedBox::shrink()).has_children());
    }
}
