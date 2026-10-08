//! [`MouseRegion`] — hover/cursor interaction widget.

use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_objects::RenderMouseRegion;
use flui_platform_api::pointer::PointerInfo;
use flui_rendering::hit_testing::{
    CursorIcon, CursorRequest, HitTestBehavior, MouseEnterCallback, MouseExitCallback,
    MouseHoverCallback, MouseRegionCallbacks,
};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    Child, EventCx, EventOutcome, IntoView, RenderView, WriterSource, impl_render_view,
};

/// An enter, hover or exit callback, stored already adapted to report its
/// outcome.
type MouseCallback = Rc<dyn Fn(&mut EventCx<'_>, PointerInfo, Offset)>;

/// Store a mouse callback, adapted to report its outcome.
fn mouse_callback<F, R>(callback: F) -> MouseCallback
where
    F: Fn(&mut EventCx<'_>, PointerInfo, Offset) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, pointer, position| {
        callback(cx, pointer, position).report();
    })
}

/// Wrap a stored callback into the lane's shape: one write per event.
fn in_write(writer: &WriterSource, callback: &MouseCallback) -> Rc<dyn Fn(PointerInfo, Offset)> {
    let writer = writer.clone();
    let callback = Rc::clone(callback);
    Rc::new(move |pointer, position| writer.write(|cx| callback(cx, pointer, position)))
}

/// Calls callbacks when the mouse enters, hovers within, or exits its bounds.
///
/// The widget over `RenderMouseRegion`. Layout and paint are pass-through when a child exists;
/// without a child the region grows to the incoming biggest constraint.
///
/// Each callback receives the dispatch's `&mut EventCx<'_>` first, so it
/// writes a signal directly (ADR-0086):
/// `.on_enter(move |cx, _pointer, _position| hovered.set(cx, true))`. The
/// pointer metadata preserves kind, role and the optional hardware device identity.
/// region has no `init_state`; it takes the owner's [`WriterSource`] from the
/// render-object context that registers its callbacks. A stationary device's
/// re-hit-test after layout also delivers enter and exit, so those writes
/// land between layout and the next frame's build, which the guard accepts.
#[derive(Clone)]
pub struct MouseRegion {
    on_enter: Option<MouseCallback>,
    on_hover: Option<MouseCallback>,
    on_exit: Option<MouseCallback>,
    cursor: CursorRequest,
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
            cursor: CursorRequest::Defer,
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
    /// Creates an opaque mouse region with no callbacks and a deferring cursor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when the mouse enters this region.
    #[must_use]
    pub fn on_enter<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerInfo, Offset) -> R + 'static,
        R: EventOutcome,
    {
        self.on_enter = Some(mouse_callback(callback));
        self
    }

    /// Called when the mouse moves within this region.
    #[must_use]
    pub fn on_hover<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerInfo, Offset) -> R + 'static,
        R: EventOutcome,
    {
        self.on_hover = Some(mouse_callback(callback));
        self
    }

    /// Called when the mouse exits this region.
    #[must_use]
    pub fn on_exit<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerInfo, Offset) -> R + 'static,
        R: EventOutcome,
    {
        self.on_exit = Some(mouse_callback(callback));
        self
    }

    /// Sets the mouse cursor reported while this region is active.
    #[must_use]
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = CursorRequest::Icon(cursor);
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

    /// The lane's callback set, each opening its write from `writer`.
    fn mouse_callbacks(&self, writer: &WriterSource) -> MouseRegionCallbacks {
        let on_enter: Option<MouseEnterCallback> = self
            .on_enter
            .as_ref()
            .map(|callback| in_write(writer, callback));
        let on_exit: Option<MouseExitCallback> = self
            .on_exit
            .as_ref()
            .map(|callback| in_write(writer, callback));
        let on_hover: Option<MouseHoverCallback> = self
            .on_hover
            .as_ref()
            .map(|callback| in_write(writer, callback));
        MouseRegionCallbacks {
            on_enter,
            on_exit,
            on_hover,
        }
    }

    fn has_callbacks(&self) -> bool {
        self.on_enter.is_some() || self.on_hover.is_some() || self.on_exit.is_some()
    }

    /// Keep the render object's one mouse-region target in sync with the
    /// complete enter/hover/exit callback set.
    ///
    /// A target, once registered, stays registered for as long as the render
    /// object stays mounted — including across a rebuild that empties the
    /// callback set down to none. A `RenderMouseRegion` stays a valid mouse
    /// tracker annotation with empty callback fields for as long as it is
    /// attached; there is no "unregister while still attached" state.
    /// Dropping the lane registration here instead would make
    /// `RenderMouseRegion::mouse_tracker_annotation`
    /// (`crates/flui-objects/src/interaction/mouse_region.rs`) stop
    /// contributing an annotation to hit-test results the moment callbacks
    /// are emptied (it requires `Some(target)`), which a stationary
    /// device's postframe recheck reads as "the region departed" and
    /// resolves a stale exit against the callback set that was active
    /// before the rebuild — see
    /// [`did_unmount_render_object`](Self::did_unmount_render_object) below
    /// for the call that DOES need to end registration, because there the
    /// render object is truly leaving the tree.
    fn sync_mouse_region_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderMouseRegion,
    ) {
        let Some(writer) = ctx.writer_source() else {
            if self.has_callbacks() {
                tracing::debug!(
                    "MouseRegion mounted without an owner graph; \
                     enter/exit events will not be delivered"
                );
            }
            return;
        };
        match render_object.mouse_region_target() {
            Some(target) => {
                if let Err(error) = ctx.replace_mouse_region(target, self.mouse_callbacks(&writer))
                {
                    tracing::warn!(?error, "MouseRegion callback replacement failed");
                }
            }
            None if self.has_callbacks() => {
                match ctx.register_mouse_region(self.mouse_callbacks(&writer)) {
                    Ok(target) => render_object.set_mouse_region_target(Some(target)),
                    Err(error) => tracing::debug!(
                        ?error,
                        "MouseRegion mounted without an active interaction lane; \
                     enter/exit events will not be delivered"
                    ),
                }
            }
            None => {}
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
    ) -> flui_rendering::RenderUpdateImpact {
        self.configure(render_object);
        self.sync_mouse_region_target(ctx, render_object);
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.mouse_region_target() {
            // `detach_mouse_region`, not `unregister_mouse_region`: this
            // render object is being permanently removed from the tree, so
            // any pending exit a stationary device's postframe recheck might
            // otherwise resolve against it must not fire. See
            // `RenderObjectContext::detach_mouse_region`'s doc for why the
            // two calls differ.
            if let Err(error) = ctx.detach_mouse_region(target) {
                tracing::debug!(?error, "MouseRegion target detach failed");
            }
            render_object.set_mouse_region_target(None);
        }
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(MouseRegion);
