//! `RenderInteractive`: the one render object of a control's behaviours.
//!
//! A control answers pointer contacts, tracks the mouse, describes itself to
//! assistive technology and marks keyboard focus. Composed from wrappers,
//! each of those is a render object of its own around the child:
//! [`RenderListener`], [`RenderMouseRegion`], [`RenderSemanticsAnnotations`]
//! and two decorated boxes for the ring. This object lays out, paints and
//! hit-tests once for the four (ADR-0322 §1).
//!
//! It does not restate what those objects know. Each behaviour is a *part*:
//! the value of the object that carries it alone, configured by the same
//! code that configures it there, and asked the same questions. A part that
//! is absent registers nothing, describes nothing and paints nothing.
//!
//! Like its parts, it stores only data-plane identities (ADR-0027): the
//! handlers live with the element that owns it.

use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_foundation::{RenderId, Single};
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineContext, BoxDryLayoutContext, BoxHitTestContext, BoxLayoutContext,
        PaintContext,
    },
    hit_testing::{
        CursorRequest, HitTestBehavior, MouseTrackerAnnotation, PanZoomTarget, PointerTarget,
        ScrollTarget, SmartZoomTarget,
    },
    input_regions::InputRegionSink,
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    semantics::SemanticsConfiguration,
    traits::{RenderBox, TextBaseline},
};

use super::{FocusRingStyle, RenderListener, RenderMouseRegion};
use crate::{RenderSemanticsAnnotations, SubtreeAnchor};

/// A single-child box of its child's size that is hit as its pointer part
/// says, carries the targets of its pointer and mouse parts on one hit
/// entry, describes its semantics part and paints its focus ring over the
/// child.
#[derive(Clone, Debug)]
pub struct RenderInteractive {
    pointer: RenderListener,
    mouse: Option<RenderMouseRegion>,
    /// Out of line: a control mounted while its window collects no
    /// semantics has no part, and holds no room for one.
    semantics: Option<Box<RenderSemanticsAnnotations>>,
    focus_ring: Option<FocusRingStyle>,
    /// Where the object publishes its identity while mounted.
    anchor: Option<SubtreeAnchor>,
    /// Kept so a semantics part set after the object attached is attached
    /// too.
    invalidation: Option<LocalRenderInvalidationHandle>,
    has_child: bool,
}

impl RenderInteractive {
    /// An object with its pointer part alone: `pointer` decides where the
    /// object is hit and which contacts it delivers.
    #[must_use]
    pub fn new(pointer: RenderListener) -> Self {
        Self {
            pointer,
            mouse: None,
            semantics: None,
            focus_ring: None,
            anchor: None,
            invalidation: None,
            has_child: false,
        }
    }

    /// The pointer part: the hit-test behaviour and the pointer, scroll,
    /// pan-zoom and smart-zoom targets.
    #[must_use]
    pub const fn pointer(&self) -> &RenderListener {
        &self.pointer
    }

    /// The pointer part, to configure as a [`RenderListener`] is.
    pub fn pointer_mut(&mut self) -> &mut RenderListener {
        &mut self.pointer
    }

    /// The mouse part: the target of enter, hover and exit, and the cursor.
    /// Where the object is hit is the pointer part's alone; the mouse part's
    /// own behaviour and opacity are not read.
    #[must_use]
    pub const fn mouse(&self) -> Option<&RenderMouseRegion> {
        self.mouse.as_ref()
    }

    /// The mouse part, to configure as a [`RenderMouseRegion`] is.
    pub fn mouse_mut(&mut self) -> Option<&mut RenderMouseRegion> {
        self.mouse.as_mut()
    }

    /// Sets or removes the mouse part, returning the previous one so its
    /// target can be released.
    pub fn set_mouse(&mut self, mouse: Option<RenderMouseRegion>) -> Option<RenderMouseRegion> {
        std::mem::replace(&mut self.mouse, mouse)
    }

    /// The semantics part.
    #[must_use]
    pub const fn semantics(&self) -> Option<&RenderSemanticsAnnotations> {
        match &self.semantics {
            Some(part) => Some(&**part),
            None => None,
        }
    }

    /// The semantics part, to configure as a [`RenderSemanticsAnnotations`]
    /// is. The impact its setters return is the object's.
    pub fn semantics_mut(&mut self) -> Option<&mut RenderSemanticsAnnotations> {
        self.semantics.as_deref_mut()
    }

    /// Sets or removes the semantics part, returning the previous one so its
    /// action route can be released. The object describes itself again.
    pub fn set_semantics(
        &mut self,
        semantics: Option<RenderSemanticsAnnotations>,
    ) -> (Option<RenderSemanticsAnnotations>, RenderUpdateImpact) {
        let mut previous = std::mem::replace(&mut self.semantics, semantics.map(Box::new));
        if let Some(previous) = &mut previous {
            RenderBox::detach(&mut **previous);
        }
        if let (Some(part), Some(handle)) = (&mut self.semantics, &self.invalidation) {
            RenderBox::attach(&mut **part, handle.clone());
        }
        let impact = if previous.is_none() && self.semantics.is_none() {
            RenderUpdateImpact::NONE
        } else {
            RenderUpdateImpact::SEMANTICS
        };
        (previous.map(|previous| *previous), impact)
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

    /// Publishes this object's identity into `anchor` while it is mounted,
    /// so its owner can measure it; `None` publishes nowhere. The previous
    /// anchor stops naming this object.
    pub fn set_anchor(&mut self, anchor: Option<SubtreeAnchor>) {
        let id = self
            .invalidation
            .as_ref()
            .map(LocalRenderInvalidationHandle::id);
        if let (Some(previous), Some(id)) = (&self.anchor, id) {
            previous.clear(id);
        }
        if let (Some(anchor), Some(id)) = (&anchor, id) {
            anchor.publish(id);
        }
        self.anchor = anchor;
    }
}

impl flui_foundation::inspect::Inspect for RenderInteractive {
    fn kind(&self) -> &'static str {
        "RenderInteractive"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("behavior", &self.pointer.behavior());
        out.field("has_pointer_target", &self.pointer.target().is_some())
            .default(&false);
        out.field("has_mouse_part", &self.mouse.is_some())
            .default(&false);
        out.field("has_semantics_part", &self.semantics.is_some())
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
        ctx: &mut BoxDryLayoutContext<'_>,
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
        ctx: &mut BoxDryBaselineContext<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        flui_rendering::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
    }

    /// The ring is painted after the child and inside the box, so a
    /// neighbour painted later cannot cover it.
    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        if self.has_child {
            ctx.paint_child();
        }
        if let Some(style) = &self.focus_ring {
            let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            style.paint_inside(ctx.canvas(), rect);
        }
    }

    fn describe_input_regions(&self, size: Size, regions: &mut InputRegionSink) {
        RenderBox::describe_input_regions(&self.pointer, size, regions);
    }

    /// The rule of [`RenderListener`], on this object's own child.
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // The child first, so what is under the control registers before it.
        let child_hit = self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO);
        let behavior = self.pointer.behavior();
        let hit_target = match behavior {
            HitTestBehavior::Opaque => true,
            HitTestBehavior::DeferToChild | HitTestBehavior::Translucent => child_hit,
        };
        if !hit_target && behavior == HitTestBehavior::Translucent {
            ctx.register_self_hit_entry();
        }
        hit_target
    }

    fn pointer_target(&self) -> Option<PointerTarget> {
        RenderBox::pointer_target(&self.pointer)
    }

    fn scroll_target(&self) -> Option<ScrollTarget> {
        RenderBox::scroll_target(&self.pointer)
    }

    fn pan_zoom_target(&self) -> Option<PanZoomTarget> {
        RenderBox::pan_zoom_target(&self.pointer)
    }

    fn smart_zoom_target(&self) -> Option<SmartZoomTarget> {
        RenderBox::smart_zoom_target(&self.pointer)
    }

    fn mouse_cursor(&self) -> CursorRequest {
        self.mouse
            .as_ref()
            .map_or(CursorRequest::Defer, RenderBox::mouse_cursor)
    }

    fn mouse_tracker_annotation(&self, id: RenderId) -> Option<MouseTrackerAnnotation> {
        self.mouse
            .as_ref()
            .and_then(|mouse| RenderBox::mouse_tracker_annotation(mouse, id))
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        if let Some(semantics) = &self.semantics {
            RenderBox::describe_semantics_configuration(&**semantics, config);
        }
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.semantics
            .as_deref()
            .is_some_and(RenderBox::excludes_semantics_subtree)
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        if let Some(semantics) = &mut self.semantics {
            RenderBox::attach(&mut **semantics, handle.clone());
        }
        if let Some(anchor) = &self.anchor {
            anchor.publish(handle.id());
        }
        self.invalidation = Some(handle);
    }

    fn detach(&mut self) {
        if let (Some(anchor), Some(handle)) = (&self.anchor, self.invalidation.take()) {
            anchor.clear(handle.id());
        }
        self.invalidation = None;
        if let Some(semantics) = &mut self.semantics {
            RenderBox::detach(&mut **semantics);
        }
    }
}
