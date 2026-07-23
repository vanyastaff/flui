//! [`MouseRegion`] — hover/cursor interaction widget.

use std::rc::Rc;

use flui_objects::RenderMouseRegion;
use flui_rendering::hit_testing::{
    CursorIcon, DeviceId, HitTestBehavior, MouseEnterCallback, MouseExitCallback,
    MouseHoverCallback, MouseRegionCallbacks,
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
    pub fn on_enter(mut self, callback: impl Fn(DeviceId, Offset) + 'static) -> Self {
        self.on_enter = Some(Rc::new(callback));
        self
    }

    /// Called when the mouse moves within this region.
    #[must_use]
    pub fn on_hover(mut self, callback: impl Fn(DeviceId, Offset) + 'static) -> Self {
        self.on_hover = Some(Rc::new(callback));
        self
    }

    /// Called when the mouse exits this region.
    #[must_use]
    pub fn on_exit(mut self, callback: impl Fn(DeviceId, Offset) + 'static) -> Self {
        self.on_exit = Some(Rc::new(callback));
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
        render_object.set_cursor(self.cursor);
        render_object.set_opaque(self.opaque);
        render_object.set_behavior(self.behavior);
    }

    fn mouse_callbacks(&self) -> MouseRegionCallbacks {
        MouseRegionCallbacks {
            on_enter: self.on_enter.clone(),
            on_exit: self.on_exit.clone(),
            on_hover: self.on_hover.clone(),
        }
    }

    fn has_callbacks(&self) -> bool {
        self.on_enter.is_some() || self.on_hover.is_some() || self.on_exit.is_some()
    }

    /// Keep the render object's one mouse-region target in sync with the
    /// complete enter/hover/exit callback set.
    fn sync_mouse_region_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderMouseRegion,
    ) {
        match (self.has_callbacks(), render_object.mouse_region_target()) {
            (true, Some(target)) => {
                if let Err(error) = ctx.replace_mouse_region(target, self.mouse_callbacks()) {
                    tracing::warn!(?error, "MouseRegion callback replacement failed");
                }
            }
            (true, None) => match ctx.register_mouse_region(self.mouse_callbacks()) {
                Ok(target) => render_object.set_mouse_region_target(Some(target)),
                Err(error) => tracing::debug!(
                    ?error,
                    "MouseRegion mounted without an active interaction lane; \
                     enter/exit events will not be delivered"
                ),
            },
            (false, Some(target)) => {
                if let Err(error) = ctx.unregister_mouse_region(target) {
                    tracing::debug!(?error, "MouseRegion target unregistration failed");
                }
                render_object.set_mouse_region_target(None);
            }
            (false, None) => {}
        }
    }
}

impl RenderView for MouseRegion {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMouseRegion;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderMouseRegion::new();
        self.configure(&mut render_object);
        self.sync_mouse_region_target(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        self.configure(render_object);
        self.sync_mouse_region_target(ctx, render_object);
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.mouse_region_target() {
            if let Err(error) = ctx.unregister_mouse_region(target) {
                tracing::debug!(?error, "MouseRegion target unregistration failed");
            }
            render_object.set_mouse_region_target(None);
        }
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
        let render_object =
            MouseRegion::new().create_render_object(&flui_view::RenderObjectContext::detached());
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
            .create_render_object(&flui_view::RenderObjectContext::detached());

        assert_eq!(render_object.cursor(), CursorIcon::Pointer);
        assert!(!render_object.opaque());
        assert_eq!(render_object.behavior(), HitTestBehavior::Translucent);
    }

    #[test]
    fn update_render_object_reconfigures_cursor_opaque_and_behavior() {
        let mut render_object =
            MouseRegion::new().create_render_object(&flui_view::RenderObjectContext::detached());

        MouseRegion::new()
            .cursor(CursorIcon::Text)
            .opaque(false)
            .behavior(HitTestBehavior::DeferToChild)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );

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
