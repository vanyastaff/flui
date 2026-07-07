//! `RenderMouseRegion` — mouse hover/cursor hit-test proxy.
//!
//! Flutter parity: `rendering/proxy_box.dart` `RenderMouseRegion`.

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxLayoutContext},
    hit_testing::{
        CursorIcon, DeviceId, EventPropagation, HitTestBehavior, InputEvent, MouseEnterCallback,
        MouseExitCallback, MouseHoverCallback, MouseTrackerAnnotation, PointerEvent,
        PointerEventExt, PointerEventHandler,
    },
    parent_data::BoxParentData,
    traits::RenderBox,
};
use flui_tree::Single;
use flui_types::{Offset, Size};

/// A callback used by [`RenderMouseRegion`] for enter, hover, and exit events.
pub type MouseRegionCallback = std::sync::Arc<dyn Fn(DeviceId, Offset) + Send + Sync>;

/// A single-child proxy that contributes mouse-tracker annotation and cursor
/// metadata to its hit-test entry.
#[derive(Clone)]
pub struct RenderMouseRegion {
    on_enter: Option<MouseEnterCallback>,
    on_hover: Option<MouseHoverCallback>,
    on_exit: Option<MouseExitCallback>,
    cursor: CursorIcon,
    valid_for_mouse_tracker: bool,
    opaque: bool,
    behavior: HitTestBehavior,
    has_child: bool,
}

impl Default for RenderMouseRegion {
    fn default() -> Self {
        Self {
            on_enter: None,
            on_hover: None,
            on_exit: None,
            cursor: CursorIcon::Default,
            valid_for_mouse_tracker: true,
            opaque: true,
            behavior: HitTestBehavior::Opaque,
            has_child: false,
        }
    }
}

impl RenderMouseRegion {
    /// Creates an opaque mouse region with no callbacks and the default cursor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the enter callback.
    pub fn set_on_enter(&mut self, callback: Option<MouseEnterCallback>) {
        self.on_enter = callback;
    }

    /// Sets the hover callback.
    pub fn set_on_hover(&mut self, callback: Option<MouseHoverCallback>) {
        self.on_hover = callback;
    }

    /// Sets the exit callback.
    pub fn set_on_exit(&mut self, callback: Option<MouseExitCallback>) {
        self.on_exit = callback;
    }

    /// Returns the active cursor.
    #[must_use]
    pub const fn cursor(&self) -> CursorIcon {
        self.cursor
    }

    /// Updates the active cursor; returns true when hit-test state changed.
    pub fn set_cursor(&mut self, cursor: CursorIcon) -> bool {
        if self.cursor == cursor {
            return false;
        }
        self.cursor = cursor;
        true
    }

    /// Whether this object should be considered by mouse tracking.
    #[must_use]
    pub const fn valid_for_mouse_tracker(&self) -> bool {
        self.valid_for_mouse_tracker
    }

    /// Updates mouse-tracker validity.
    pub fn set_valid_for_mouse_tracker(&mut self, valid: bool) -> bool {
        if self.valid_for_mouse_tracker == valid {
            return false;
        }
        self.valid_for_mouse_tracker = valid;
        true
    }

    /// Whether this region should block regions visually behind it.
    #[must_use]
    pub const fn opaque(&self) -> bool {
        self.opaque
    }

    /// Updates the opacity flag.
    pub fn set_opaque(&mut self, opaque: bool) -> bool {
        if self.opaque == opaque {
            return false;
        }
        self.opaque = opaque;
        true
    }

    /// Returns the hit-test behavior.
    #[must_use]
    pub const fn behavior(&self) -> HitTestBehavior {
        self.behavior
    }

    /// Updates the hit-test behavior.
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) -> bool {
        if self.behavior == behavior {
            return false;
        }
        self.behavior = behavior;
        true
    }

    fn hover_handler(&self) -> Option<PointerEventHandler> {
        let on_hover = self.on_hover.clone()?;
        Some(std::sync::Arc::new(move |event: &PointerEvent| {
            if matches!(event, PointerEvent::Move(_)) {
                let device_id = InputEvent::Pointer(event.clone()).device_id().unwrap_or(0);
                on_hover(device_id, event.position());
            }
            EventPropagation::Continue
        }))
    }
}

impl std::fmt::Debug for RenderMouseRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderMouseRegion")
            .field("has_enter", &self.on_enter.is_some())
            .field("has_hover", &self.on_hover.is_some())
            .field("has_exit", &self.on_exit.is_some())
            .field("cursor", &self.cursor)
            .field("valid_for_mouse_tracker", &self.valid_for_mouse_tracker)
            .field("opaque", &self.opaque)
            .field("behavior", &self.behavior)
            .field("has_child", &self.has_child)
            .finish()
    }
}

impl flui_foundation::Diagnosticable for RenderMouseRegion {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("cursor", self.cursor);
        builder.add_flag("has_enter", self.on_enter.is_some(), "has_enter");
        builder.add_flag("has_hover", self.on_hover.is_some(), "has_hover");
        builder.add_flag("has_exit", self.on_exit.is_some(), "has_exit");
        builder.add_flag(
            "valid_for_mouse_tracker",
            self.valid_for_mouse_tracker,
            "valid_for_mouse_tracker",
        );
        builder.add_flag("opaque", self.opaque, "opaque");
        builder.add_enum("behavior", self.behavior);
    }
}

impl RenderBox for RenderMouseRegion {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.biggest()
        }
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() > 0 {
            ctx.child_dry_layout(0, constraints)
        } else {
            constraints.biggest()
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        let child_hit = self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO);
        let hit_target = match self.behavior {
            HitTestBehavior::Opaque => true,
            HitTestBehavior::DeferToChild | HitTestBehavior::Translucent => child_hit,
        };

        if hit_target || self.behavior == HitTestBehavior::Translucent {
            ctx.register_self_hit_entry();
        }

        hit_target && self.opaque
    }

    fn pointer_event_handler(&self) -> Option<PointerEventHandler> {
        self.hover_handler()
    }

    fn mouse_cursor(&self) -> CursorIcon {
        self.cursor
    }

    fn mouse_tracker_annotation(
        &self,
        id: flui_foundation::RenderId,
    ) -> Option<MouseTrackerAnnotation> {
        self.valid_for_mouse_tracker
            .then(|| MouseTrackerAnnotation {
                region_id: id,
                on_enter: self.on_enter.clone(),
                on_exit: self.on_exit.clone(),
                on_hover: self.on_hover.clone(),
            })
    }
}
