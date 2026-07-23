//! `RenderMouseRegion` — mouse hover/cursor hit-test proxy.
//!
//! Flutter parity: `rendering/proxy_box.dart` `RenderMouseRegion`.

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxLayoutContext},
    hit_testing::{
        CursorIcon, DeviceId, HitTestBehavior, MouseRegionTarget, MouseTrackerAnnotation,
        PointerTarget,
    },
    parent_data::BoxParentData,
    traits::RenderBox,
};
use flui_tree::Single;
use flui_types::{Offset, Size};

/// A callback used by [`RenderMouseRegion`] widgets for enter, hover, and exit
/// events.
///
/// The callback itself must not be stored in `RenderMouseRegion`; widgets keep
/// it in the owner-local interaction lane and store only data-plane target IDs
/// in render state.
pub type MouseRegionCallback = std::rc::Rc<dyn Fn(DeviceId, Offset)>;

/// A single-child proxy that contributes mouse-tracker annotation and cursor
/// metadata to its hit-test entry.
#[derive(Clone)]
pub struct RenderMouseRegion {
    /// Owner-local pointer target delivering hover `Move` events; the
    /// executable hover adapter lives in the interaction lane (ADR-0027).
    hover_target: Option<PointerTarget>,
    /// Owner-local mouse target delivering enter/exit callbacks; the render
    /// object stores only this data-plane identity.
    mouse_target: Option<MouseRegionTarget>,
    cursor: CursorIcon,
    valid_for_mouse_tracker: bool,
    opaque: bool,
    behavior: HitTestBehavior,
    has_child: bool,
}

impl Default for RenderMouseRegion {
    fn default() -> Self {
        Self {
            hover_target: None,
            mouse_target: None,
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

    /// The pointer target advertised for hover delivery, if any.
    #[must_use]
    pub const fn hover_target(&self) -> Option<PointerTarget> {
        self.hover_target
    }

    /// Sets the owner-local pointer target that delivers hover events.
    pub fn set_hover_target(&mut self, target: Option<PointerTarget>) {
        self.hover_target = target;
    }

    /// The mouse-region target advertised for tracker enter/exit delivery, if
    /// any.
    #[must_use]
    pub const fn mouse_region_target(&self) -> Option<MouseRegionTarget> {
        self.mouse_target
    }

    /// Sets the owner-local mouse-region target.
    pub fn set_mouse_region_target(&mut self, target: Option<MouseRegionTarget>) {
        self.mouse_target = target;
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
}

impl std::fmt::Debug for RenderMouseRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderMouseRegion")
            .field("has_mouse_target", &self.mouse_target.is_some())
            .field("has_hover_target", &self.hover_target.is_some())
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
        builder.add_flag(
            "has_mouse_target",
            self.mouse_target.is_some(),
            "has_mouse_target",
        );
        builder.add_flag(
            "has_hover_target",
            self.hover_target.is_some(),
            "has_hover_target",
        );
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

    fn pointer_target(&self) -> Option<PointerTarget> {
        self.hover_target
    }

    fn mouse_cursor(&self) -> CursorIcon {
        self.cursor
    }

    fn mouse_tracker_annotation(
        &self,
        id: flui_foundation::RenderId,
    ) -> Option<MouseTrackerAnnotation> {
        self.valid_for_mouse_tracker
            .then_some(self.mouse_target)
            .flatten()
            .map(|target| MouseTrackerAnnotation::new(id, target))
    }
}
