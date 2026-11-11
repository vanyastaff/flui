//! `RenderInteractive`: the one render object of a control's behaviours.
//!
//! A control answers pointer contacts, tracks the mouse, describes itself to
//! assistive technology and marks keyboard focus. Composed from wrappers,
//! each of those is a render object of its own ([`RenderListener`],
//! [`RenderMouseRegion`], `RenderSemanticsAnnotations` and two decorated
//! boxes for the ring). This object does the four for one child, and a part
//! that is absent costs nothing: it holds no target, describes nothing and
//! paints nothing (ADR-0322 §1).
//!
//! Like the objects it stands for, it stores only data-plane identities
//! (ADR-0027): the handlers live with the element that owns it.
//!
//! [`RenderListener`]: super::RenderListener
//! [`RenderMouseRegion`]: super::RenderMouseRegion

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxLayoutContext, PaintCx},
    hit_testing::{
        CursorRequest, HitTestBehavior, MouseRegionTarget, MouseTrackerAnnotation, PointerTarget,
    },
    parent_data::BoxParentData,
    semantics::SemanticsConfiguration,
    traits::{RenderBox, TextBaseline},
};

use super::FocusRingStyle;

/// A single-child box of its child's size that answers the hit test for a
/// control, carries its pointer and mouse targets, describes its one
/// semantics configuration and paints its focus ring over the child.
#[derive(Clone, Debug)]
pub struct RenderInteractive {
    behavior: HitTestBehavior,
    pointer_target: Option<PointerTarget>,
    claims_presses: bool,
    mouse_target: Option<MouseRegionTarget>,
    cursor: CursorRequest,
    semantics: SemanticsConfiguration,
    focus_ring: Option<FocusRingStyle>,
    has_child: bool,
}

impl RenderInteractive {
    /// An object with no part: it lays out and paints its child, is hit
    /// where `behavior` says and delivers nothing.
    #[must_use]
    pub fn new(behavior: HitTestBehavior) -> Self {
        Self {
            behavior,
            pointer_target: None,
            claims_presses: false,
            mouse_target: None,
            cursor: CursorRequest::Defer,
            semantics: SemanticsConfiguration::new(),
            focus_ring: None,
            has_child: false,
        }
    }

    /// Where the object is hit: over its whole box (`Opaque`), where its
    /// child is (`DeferToChild`), or where its child is while contacts on
    /// the rest of the box also reach what is behind (`Translucent`).
    #[must_use]
    pub const fn behavior(&self) -> HitTestBehavior {
        self.behavior
    }

    /// Replaces the hit-test behavior. Hit testing reads it at the next
    /// contact; nothing is laid out or painted again.
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) {
        self.behavior = behavior;
    }

    /// The pointer target the object's hit entries carry.
    #[must_use]
    pub const fn pointer_target(&self) -> Option<PointerTarget> {
        self.pointer_target
    }

    /// Replaces the pointer target. `claims_presses` says whether the
    /// control takes presses (it has a press handler or adds recognizers to
    /// the arena): under an application's title bar such a control is cut
    /// out of the move area. The input regions are collected on composited
    /// frames, so a change of the claim marks paint.
    pub fn set_pointer_target(
        &mut self,
        target: Option<PointerTarget>,
        claims_presses: bool,
    ) -> RenderUpdateImpact {
        self.pointer_target = target;
        if self.claims_presses == claims_presses {
            return RenderUpdateImpact::NONE;
        }
        self.claims_presses = claims_presses;
        RenderUpdateImpact::PAINT
    }

    /// The mouse target that receives enter, hover and exit.
    #[must_use]
    pub const fn mouse_region_target(&self) -> Option<MouseRegionTarget> {
        self.mouse_target
    }

    /// Replaces the mouse target.
    pub fn set_mouse_region_target(&mut self, target: Option<MouseRegionTarget>) {
        self.mouse_target = target;
    }

    /// The cursor shown over the object.
    #[must_use]
    pub const fn cursor(&self) -> CursorRequest {
        self.cursor
    }

    /// Replaces the cursor. The mouse tracker reads it at its next pass.
    pub fn set_cursor(&mut self, cursor: CursorRequest) {
        self.cursor = cursor;
    }

    /// The semantics the object describes.
    #[must_use]
    pub const fn semantics(&self) -> &SemanticsConfiguration {
        &self.semantics
    }

    /// Replaces the semantics configuration, whole: the boundary, the role,
    /// the flags and the actions of the control are one value.
    pub fn set_semantics(&mut self, configuration: SemanticsConfiguration) -> RenderUpdateImpact {
        if self.semantics == configuration {
            return RenderUpdateImpact::NONE;
        }
        self.semantics = configuration;
        RenderUpdateImpact::SEMANTICS
    }

    /// The focus ring painted over the child, while one is shown.
    #[must_use]
    pub const fn focus_ring(&self) -> Option<FocusRingStyle> {
        self.focus_ring
    }

    /// Shows the focus ring in `style`, or hides it with `None`.
    pub fn set_focus_ring(&mut self, style: Option<FocusRingStyle>) -> RenderUpdateImpact {
        if self.focus_ring == style {
            return RenderUpdateImpact::NONE;
        }
        self.focus_ring = style;
        RenderUpdateImpact::PAINT
    }
}

impl flui_foundation::inspect::Inspect for RenderInteractive {
    fn kind(&self) -> &'static str {
        "RenderInteractive"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("behavior", &self.behavior);
        out.field("has_pointer_target", &self.pointer_target.is_some())
            .default(&false);
        out.field("claims_presses", &self.claims_presses)
            .default(&false);
        out.field("has_mouse_target", &self.mouse_target.is_some())
            .default(&false);
        out.field("has_semantics", &self.semantics.has_been_annotated())
            .default(&false);
        out.field("focus_ring", &self.focus_ring.is_some())
            .default(&false);
    }
}

impl RenderBox for RenderInteractive {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn forwards_baseline_to_only_child(&self) -> bool {
        true
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints)?;
            ctx.position_child(0, Offset::ZERO);
            Ok(child_size)
        } else {
            self.has_child = false;
            Ok(constraints.biggest())
        }
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        if ctx.child_count() == 0 {
            Ok(constraints.biggest())
        } else {
            ctx.child_dry_layout(0, constraints)
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        flui_rendering::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
    }

    /// The ring is painted after the child and inside the box, so a
    /// neighbour painted later cannot cover it.
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if self.has_child {
            ctx.paint_child();
        }
        if let Some(style) = &self.focus_ring {
            let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            style.paint_inside(ctx.canvas(), rect);
        }
    }

    fn describe_input_regions(
        &self,
        _size: Size,
        regions: &mut flui_rendering::input_regions::InputRegionSink,
    ) {
        if self.claims_presses {
            regions.claim_presses();
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // The child first, so what is under the control registers before it.
        let child_hit = self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO);
        let hit_target = match self.behavior {
            HitTestBehavior::Opaque => true,
            HitTestBehavior::DeferToChild | HitTestBehavior::Translucent => child_hit,
        };
        if !hit_target && self.behavior == HitTestBehavior::Translucent {
            ctx.register_self_hit_entry();
        }
        hit_target
    }

    fn pointer_target(&self) -> Option<PointerTarget> {
        self.pointer_target
    }

    fn mouse_cursor(&self) -> CursorRequest {
        self.cursor
    }

    fn mouse_tracker_annotation(
        &self,
        id: flui_foundation::RenderId,
    ) -> Option<MouseTrackerAnnotation> {
        self.mouse_target
            .map(|target| MouseTrackerAnnotation::new(id, target))
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        *config = self.semantics.clone();
    }
}
