//! `RenderPhysicalModel` / `RenderPhysicalShape` — a clipped, shadow-casting,
//! filled surface around a single child.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderPhysicalModel`](https://api.flutter.dev/flutter/rendering/RenderPhysicalModel-class.html)
//! and
//! [`RenderPhysicalShape`](https://api.flutter.dev/flutter/rendering/RenderPhysicalShape-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`,
//! `_RenderPhysicalModelBase<T>` `:2062-2126`, `RenderPhysicalModel`
//! `:2132-2269`, `RenderPhysicalShape` `:2280-2373`).
//!
//! # Rust-native shape
//!
//! The two oracle classes share their entire paint recipe, hit-test
//! recipe, and four field-level setters — only how the clip shape is
//! derived from `size` differs (`BoxShape` + `BorderRadius` vs. an owner-lane
//! [`PathClipTarget`]). That is collapsed to one generic body,
//! [`RenderPhysicalModelBase<C>`], monomorphised via [`RenderPhysicalModel`]
//! and [`RenderPhysicalShape`].
//!
//! Deliberately **not** built on [`super::clip::ClipGeometry`]: that trait's
//! `default_for_size(size: Size) -> Self` is a pure function of size alone,
//! with no room for `RenderPhysicalModel`'s extra `shape`/`border_radius`
//! per-instance config, and it carries no shadow/fill vocabulary (plain
//! clips never draw a shape, only clip). [`PhysicalClipSource`] and
//! [`PhysicalClipShape`] are a small, local trait pair scoped to exactly
//! this family instead.
//!
//! # Divergences from a literal transcription (all backed by the design
//! research doc, `docs/research/2026-07-01-render-physical-model-plan.md`)
//!
//! - **Hit-test always tests the clip shape for both variants.** The oracle
//!   gates this on `_clipper != null`, which for `RenderPhysicalModel`
//!   (which never exposes a public clipper) means the gate never engages —
//!   a circular `RenderPhysicalModel` hit-tests as its full bounding box in
//!   real Flutter. This port applies the already-shipped
//!   [`super::clip::RenderClip`] convention (always test the shape) to both
//!   variants for FLUI-wide consistency. See [`RenderBox::hit_test`] below.
//! - **`debugFillProperties` surfaces the real `shadow_color`.** The oracle
//!   has a confirmed bug (`proxy_box.dart:2124`) that passes `color` twice
//!   instead of `shadowColor`. Not reproduced here.
//! - **`clip_behavior` defaults to `Clip::None`**, not `Clip::AntiAlias` —
//!   the opposite of `RenderClip<S>`'s own default. Physical-model surfaces
//!   don't clip by default (oracle `:2071`).

use std::fmt;

use flui_painting::{Canvas, Paint};
use flui_tree::Single;
use flui_types::{
    Color, Offset, Pixels, Point, Rect, Size,
    geometry::RRect,
    layout::BoxShape,
    painting::{Clip, Path},
    styling::{BorderRadius, BorderRadiusExt},
};

use flui_foundation::DiagnosticsBuilder;
use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    hit_testing::{PathClipTarget, resolve_path_clip_target},
    parent_data::BoxParentData,
    traits::RenderBox,
};

// =============================================================================
// PhysicalClipShape — shape-level operations (RRect, Path)
// =============================================================================

/// Shape-level operations shared by the two clip carriers physical-model
/// surfaces use ([`RRect`], [`Path`]).
///
/// Deliberately **not** [`super::clip::ClipGeometry`] — see the module doc
/// for why (extra per-instance config on the rectangle source, plus a
/// shadow/fill vocabulary `ClipGeometry` has no need for).
pub trait PhysicalClipShape: Clone + fmt::Debug + Send + Sync + 'static {
    /// Returns `true` if the local-space `position` falls inside the shape.
    fn contains(&self, position: Point<Pixels>) -> bool;

    /// The path [`Canvas::draw_shadow`] casts against.
    fn shadow_path(&self) -> Path;

    /// Fills the shape directly on the *current* canvas — used for the
    /// non-save-layer branch, drawn before the clip scope is entered.
    fn fill(&self, canvas: &mut Canvas, paint: &Paint);

    /// Opens this shape as a clip-layer scope covering everything recorded
    /// inside `f`, child subtree included.
    fn with_clip_scope(
        &self,
        ctx: &mut PaintCx<'_, Single>,
        clip_behavior: Clip,
        f: impl FnOnce(&mut PaintCx<'_, Single>),
    );
}

/// Point-in-rounded-rect test for [`PhysicalClipShape::contains`] on
/// [`RRect`]. A fresh implementation — not shared with
/// `proxy::clip::ClipGeometry`'s own `RRect` impl, matching this module's
/// deliberate choice not to depend on `clip.rs` (see module doc).
fn rrect_contains(rrect: &RRect, position: Point<Pixels>) -> bool {
    let bounds = rrect.bounding_rect();
    if !bounds.contains(position) {
        return false;
    }

    let px = position.x.get();
    let py = position.y.get();
    let left = bounds.left().get();
    let top = bounds.top().get();
    let right = bounds.right().get();
    let bottom = bounds.bottom().get();

    // For each corner, a point inside the corner's square sub-region but
    // outside its inscribed ellipse is outside the rounded rect.
    let excluded_by_corner = |cx: f32, cy: f32, rx: f32, ry: f32, in_corner: bool| -> bool {
        if !in_corner || rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let dx = (px - cx) / rx;
        let dy = (py - cy) / ry;
        dx * dx + dy * dy > 1.0
    };

    let tl_rx = rrect.top_left.x.get();
    let tl_ry = rrect.top_left.y.get();
    let in_tl = px < left + tl_rx && py < top + tl_ry;
    if excluded_by_corner(left + tl_rx, top + tl_ry, tl_rx, tl_ry, in_tl) {
        return false;
    }

    let tr_rx = rrect.top_right.x.get();
    let tr_ry = rrect.top_right.y.get();
    let in_tr = px > right - tr_rx && py < top + tr_ry;
    if excluded_by_corner(right - tr_rx, top + tr_ry, tr_rx, tr_ry, in_tr) {
        return false;
    }

    let br_rx = rrect.bottom_right.x.get();
    let br_ry = rrect.bottom_right.y.get();
    let in_br = px > right - br_rx && py > bottom - br_ry;
    if excluded_by_corner(right - br_rx, bottom - br_ry, br_rx, br_ry, in_br) {
        return false;
    }

    let bl_rx = rrect.bottom_left.x.get();
    let bl_ry = rrect.bottom_left.y.get();
    let in_bl = px < left + bl_rx && py > bottom - bl_ry;
    if excluded_by_corner(left + bl_rx, bottom - bl_ry, bl_rx, bl_ry, in_bl) {
        return false;
    }

    true
}

impl PhysicalClipShape for RRect {
    fn contains(&self, position: Point<Pixels>) -> bool {
        rrect_contains(self, position)
    }

    fn shadow_path(&self) -> Path {
        Path::from_rrect(*self)
    }

    fn fill(&self, canvas: &mut Canvas, paint: &Paint) {
        canvas.draw_rrect(*self, paint);
    }

    fn with_clip_scope(
        &self,
        ctx: &mut PaintCx<'_, Single>,
        clip_behavior: Clip,
        f: impl FnOnce(&mut PaintCx<'_, Single>),
    ) {
        ctx.with_clip_rrect(*self, clip_behavior, f);
    }
}

impl PhysicalClipShape for Path {
    fn contains(&self, position: Point<Pixels>) -> bool {
        // Resolves to the inherent `Path::contains` (fill-type-aware
        // ray-casting/winding test), not infinite recursion — inherent
        // methods take priority over trait methods in method resolution.
        self.contains(position)
    }

    fn shadow_path(&self) -> Path {
        self.clone()
    }

    fn fill(&self, canvas: &mut Canvas, paint: &Paint) {
        canvas.draw_path(self, paint);
    }

    fn with_clip_scope(
        &self,
        ctx: &mut PaintCx<'_, Single>,
        clip_behavior: Clip,
        f: impl FnOnce(&mut PaintCx<'_, Single>),
    ) {
        ctx.with_clip_path(self.clone(), clip_behavior, f);
    }
}

// =============================================================================
// PhysicalClipSource — per-variant "size -> clip shape" derivation
// =============================================================================

/// Per-variant "how do I derive the clip shape from `size`" source.
pub trait PhysicalClipSource: Clone + fmt::Debug + Send + Sync + 'static {
    /// The clip-shape type this source produces.
    type Shape: PhysicalClipShape;

    /// Flutter-parity diagnostics label (`RenderPhysicalModel`,
    /// `RenderPhysicalShape`) — generic `RenderPhysicalModelBase<C>` would
    /// otherwise surface an unreadable monomorphised type name.
    const DIAGNOSTIC_NAME: &'static str;

    /// Computes the clip shape for a box of the given laid-out `size`,
    /// origin at `(0, 0)` in local coordinates.
    fn compute_clip(&self, size: Size) -> Self::Shape;

    /// Appends variant-specific diagnostics (`shape`/`border_radius`, or
    /// `custom_clipper`) on top of the shared elevation/color/shadow/clip
    /// properties.
    fn debug_fill_extra(&self, builder: &mut DiagnosticsBuilder);
}

/// [`RenderPhysicalModel`]'s clip source: a [`BoxShape`] plus an optional
/// [`BorderRadius`] (ignored unless `shape` is `Rectangle`).
#[derive(Debug, Clone)]
pub struct RectangleClip {
    /// The box shape (`Rectangle` or `Circle`).
    pub shape: BoxShape,
    /// The border radius, applied only when `shape == BoxShape::Rectangle`.
    /// `None` behaves like `BorderRadius::ZERO` (oracle `:2169-2174`).
    pub border_radius: Option<BorderRadius>,
}

impl PhysicalClipSource for RectangleClip {
    type Shape = RRect;
    const DIAGNOSTIC_NAME: &'static str = "RenderPhysicalModel";

    fn compute_clip(&self, size: Size) -> RRect {
        let rect = Rect::from_origin_size(Point::ZERO, size);
        match self.shape {
            BoxShape::Rectangle => {
                let br = self.border_radius.unwrap_or(BorderRadius::ZERO);
                // Mirrors `flui-painting/src/decoration.rs`'s `decoration_rrect`
                // exactly — same field destructure, same lack of
                // `clamp_radii()` (see module doc / research plan trap §4.8).
                RRect::from_rect_and_corners(
                    rect,
                    br.top_left,
                    br.top_right,
                    br.bottom_right,
                    br.bottom_left,
                )
            }
            // Oracle `proxy_box.dart:2188` — `width/2, height/2` as TWO
            // INDEPENDENT radii (an ellipse inscribed in the bounding box),
            // NOT a true circle for non-square boxes. This deliberately
            // contradicts `BoxShape::Circle`'s own doc comment; follow the
            // oracle formula, not the doc comment (research plan trap §4.4).
            BoxShape::Circle => RRect::from_rect_xy(rect, rect.width() * 0.5, rect.height() * 0.5),
        }
    }

    fn debug_fill_extra(&self, builder: &mut DiagnosticsBuilder) {
        builder.add_enum("shape", self.shape);
        builder.add_optional(
            "border_radius",
            self.border_radius.map(|br| format!("{br:?}")),
        );
    }
}

/// [`RenderPhysicalShape`]'s clip source: an owner-local path clip target.
///
/// Stored as `Option` (matching the oracle's nullable base-class `clipper`
/// field) because clearing it falls back to the whole-box rectangle default.
#[derive(Clone, Copy)]
pub struct PathClip {
    /// The active owner-lane path target, or `None` to fall back to the
    /// whole-box rect.
    pub target: Option<PathClipTarget>,
}

impl fmt::Debug for PathClip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PathClip")
            .field("has_custom_clipper", &self.target.is_some())
            .finish()
    }
}

impl PhysicalClipSource for PathClip {
    type Shape = Path;
    const DIAGNOSTIC_NAME: &'static str = "RenderPhysicalShape";

    fn compute_clip(&self, size: Size) -> Path {
        if let Some(target) = self.target {
            match resolve_path_clip_target(target, size) {
                Ok(path) => return path,
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "RenderPhysicalShape path target resolution failed; using fallback clip"
                    );
                }
            }
        }

        // Oracle `:2296`'s `_defaultClip` fallback — reachable once a path
        // target is cleared or cannot be resolved by the active owner lane.
        let mut path = Path::new();
        path.add_rect(Rect::from_origin_size(Point::ZERO, size));
        path
    }

    fn debug_fill_extra(&self, builder: &mut DiagnosticsBuilder) {
        builder.add_flag(
            "custom_clipper",
            self.target.is_some(),
            "has custom clipper",
        );
    }
}

// =============================================================================
// RenderPhysicalModelBase<C> — generic render object
// =============================================================================

/// A render object that casts a drop shadow, fills, and clips its child to a
/// shape derived from `C`.
///
/// Pick the ergonomic alias:
/// * [`RenderPhysicalModel`] — `BoxShape` + `BorderRadius` clip source.
/// * [`RenderPhysicalShape`] — owner-lane [`PathClipTarget`] clip source.
#[derive(Debug, Clone)]
pub struct RenderPhysicalModelBase<C: PhysicalClipSource> {
    /// The per-variant clip-shape source.
    clip_source: C,
    /// Shadow elevation. `0.0` means no shadow is cast.
    elevation: f32,
    /// The fill color painted behind (or, under `AntiAliasWithSaveLayer`,
    /// inside) the clip.
    color: Color,
    /// The drop-shadow color, used only when `elevation != 0.0`.
    shadow_color: Color,
    /// The clip behavior. Defaults to `Clip::None` — see module doc.
    clip_behavior: Clip,
    /// Whether a child is attached (tracked for hit testing, mirroring
    /// `RenderClip<S>`'s own `has_child` field — there is no
    /// `child_count()` on `BoxHitTestContext`).
    has_child: bool,
}

/// `BoxShape` + `BorderRadius` variant — Flutter's `RenderPhysicalModel`.
pub type RenderPhysicalModel = RenderPhysicalModelBase<RectangleClip>;

/// Arbitrary-path-target variant — Flutter's `RenderPhysicalShape`.
pub type RenderPhysicalShape = RenderPhysicalModelBase<PathClip>;

impl<C: PhysicalClipSource> RenderPhysicalModelBase<C> {
    /// Shared field baseline: `elevation = 0.0`, `shadow_color` = opaque
    /// black (oracle `Color(0xFF000000)`), `clip_behavior = Clip::None`
    /// (oracle `:2071` — overridden down from `_RenderCustomClip`'s own
    /// `Clip::AntiAlias`).
    fn with_clip_source(clip_source: C, color: Color) -> Self {
        Self {
            clip_source,
            elevation: 0.0,
            color,
            shadow_color: Color::BLACK,
            clip_behavior: Clip::None,
            has_child: false,
        }
    }

    /// The current elevation. Zero means no shadow is cast.
    #[inline]
    pub fn elevation(&self) -> f32 {
        self.elevation
    }

    /// Builder: sets the elevation (debug-asserts non-negative, matching
    /// the oracle's own triple-asserted invariant).
    #[must_use]
    pub fn with_elevation(mut self, elevation: f32) -> Self {
        debug_assert!(
            elevation >= 0.0,
            "RenderPhysicalModelBase: elevation must be non-negative, got {elevation}"
        );
        self.elevation = elevation;
        self
    }

    /// Replaces the elevation; returns `true` if the value changed.
    /// Paint-only — Flutter parity: `markNeedsPaint()`, never a relayout.
    pub fn set_elevation(&mut self, elevation: f32) -> bool {
        debug_assert!(
            elevation >= 0.0,
            "RenderPhysicalModelBase: elevation must be non-negative, got {elevation}"
        );
        if self.elevation == elevation {
            return false;
        }
        self.elevation = elevation;
        true
    }

    /// The current fill color.
    #[inline]
    pub fn color(&self) -> Color {
        self.color
    }

    /// Builder: sets the fill color.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Replaces the fill color; returns `true` if the value changed.
    pub fn set_color(&mut self, color: Color) -> bool {
        if self.color == color {
            return false;
        }
        self.color = color;
        true
    }

    /// The current shadow color.
    #[inline]
    pub fn shadow_color(&self) -> Color {
        self.shadow_color
    }

    /// Builder: sets the shadow color.
    #[must_use]
    pub fn with_shadow_color(mut self, shadow_color: Color) -> Self {
        self.shadow_color = shadow_color;
        self
    }

    /// Replaces the shadow color; returns `true` if the value changed.
    pub fn set_shadow_color(&mut self, shadow_color: Color) -> bool {
        if self.shadow_color == shadow_color {
            return false;
        }
        self.shadow_color = shadow_color;
        true
    }

    /// The current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Builder: sets the clip behavior.
    #[must_use]
    pub fn with_clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Replaces the clip behavior; returns `true` if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }
}

impl RenderPhysicalModelBase<RectangleClip> {
    /// Creates a `RenderPhysicalModel`: `shape = BoxShape::Rectangle`,
    /// `border_radius = None`, `elevation = 0.0`, `shadow_color` = opaque
    /// black, `clip_behavior = Clip::None` (oracle defaults).
    pub fn new(color: Color) -> Self {
        Self::with_clip_source(
            RectangleClip {
                shape: BoxShape::Rectangle,
                border_radius: None,
            },
            color,
        )
    }

    /// Builder: sets the box shape.
    #[must_use]
    pub fn with_shape(mut self, shape: BoxShape) -> Self {
        self.clip_source.shape = shape;
        self
    }

    /// Builder: sets the border radius (ignored unless `shape` is
    /// `BoxShape::Rectangle`).
    #[must_use]
    pub fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.clip_source.border_radius = Some(border_radius);
        self
    }

    /// The current box shape.
    #[inline]
    pub fn shape(&self) -> BoxShape {
        self.clip_source.shape
    }

    /// The current border radius, if any.
    #[inline]
    pub fn border_radius(&self) -> Option<BorderRadius> {
        self.clip_source.border_radius
    }

    /// Replaces the box shape; returns `true` if the value changed.
    /// Paint/hit-test only — Flutter parity: `_markNeedsClip()`, never a
    /// relayout (the clip shape never affects `size`).
    pub fn set_shape(&mut self, shape: BoxShape) -> bool {
        if self.clip_source.shape == shape {
            return false;
        }
        self.clip_source.shape = shape;
        true
    }

    /// Replaces the border radius; returns `true` if the value changed.
    pub fn set_border_radius(&mut self, border_radius: Option<BorderRadius>) -> bool {
        if self.clip_source.border_radius == border_radius {
            return false;
        }
        self.clip_source.border_radius = border_radius;
        true
    }
}

impl RenderPhysicalModelBase<PathClip> {
    /// Creates a `RenderPhysicalShape` with the whole-box fallback clip.
    ///
    /// Bounds-dependent path factories are registered in the owner runtime and
    /// connected with [`with_path_clip_target`](Self::with_path_clip_target);
    /// this render object never stores executable clipper callbacks.
    pub fn new(color: Color) -> Self {
        Self::with_clip_source(PathClip { target: None }, color)
    }

    /// Builder: sets the owner-lane path clip target.
    #[must_use]
    pub fn with_path_clip_target(mut self, target: PathClipTarget) -> Self {
        self.clip_source.target = Some(target);
        self
    }

    /// Returns the owner-lane path clip target, if one is installed.
    #[inline]
    #[must_use]
    pub const fn path_clip_target(&self) -> Option<PathClipTarget> {
        self.clip_source.target
    }

    /// Whether a clipper is currently installed.
    #[inline]
    pub fn has_custom_clipper(&self) -> bool {
        self.clip_source.target.is_some()
    }

    /// Replaces the path clip target; returns `true` if the value changed
    /// (`None` -> `Some`, `Some` -> `None`, or a swap between two distinct
    /// targets). `None` falls back to the whole-box rectangle default clip
    /// (oracle `:2296`).
    ///
    /// Comparing the full `Option<PathClipTarget>`, not just presence, is
    /// load-bearing: oracle `RenderPhysicalShape`'s `clipper` setter compares
    /// the new `CustomClipper` for equality and calls `markNeedsPaint()`
    /// whenever it differs (`_markNeedsClip()`), including a swap between two
    /// distinct non-null clippers — a presence-only check would silently miss
    /// that swap and never signal a repaint.
    pub fn set_path_clip_target(&mut self, target: Option<PathClipTarget>) -> bool {
        if self.clip_source.target == target {
            return false;
        }
        self.clip_source.target = target;
        true
    }
}

impl<C: PhysicalClipSource> flui_foundation::Diagnosticable for RenderPhysicalModelBase<C> {
    fn to_diagnostics_node(&self) -> flui_foundation::DiagnosticsNode {
        let mut node = flui_foundation::DiagnosticsNode::new(C::DIAGNOSTIC_NAME);
        let mut builder = flui_foundation::DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_double("elevation", self.elevation, None);
        builder.add_color("color", format!("{:?}", self.color));
        // Oracle bug (`proxy_box.dart:2124`) passes `color` a second time
        // here instead of `shadowColor` — not reproduced; this reads the
        // real field.
        builder.add_color("shadow_color", format!("{:?}", self.shadow_color));
        builder.add_enum("clip_behavior", self.clip_behavior);
        self.clip_source.debug_fill_extra(builder);
    }
}

impl<C: PhysicalClipSource> RenderBox for RenderPhysicalModelBase<C> {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle `:2206-2209`/`:2311-2314` — no child means nothing is
        // drawn at all, not even the shadow or fill.
        if ctx.child_count() == 0 {
            return;
        }

        let size = ctx.size();
        let shape = self.clip_source.compute_clip(size);

        if self.elevation != 0.0 {
            ctx.canvas()
                .draw_shadow(&shape.shadow_path(), self.shadow_color, self.elevation);
        }

        // The `usesSaveLayer` fork controls WHERE the fill is drawn, not
        // just whether: `!uses_save_layer` fills OUTSIDE the clip (on the
        // current canvas, before the scope is entered); `uses_save_layer`
        // fills INSIDE the clip scope via `draw_paint` (oracle
        // `:2235-2249`, citing flutter/flutter#18057 — avoids double
        // anti-aliasing the same edge). Exactly one fill happens either way.
        let uses_save_layer = self.clip_behavior == Clip::AntiAliasWithSaveLayer;
        let fill_paint = Paint::fill(self.color);
        if !uses_save_layer {
            shape.fill(ctx.canvas(), &fill_paint);
        }

        shape.with_clip_scope(ctx, self.clip_behavior, |ctx| {
            if uses_save_layer {
                ctx.canvas().draw_paint(&fill_paint);
            }
            ctx.paint_child();
        });
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // FLUI-wide convention (`RenderClip<S>`, `clip.rs`): always test
        // the shape. This is a deliberate divergence from the oracle for
        // `RenderPhysicalModel` specifically — the oracle gates this test
        // on `_clipper != null`, which is always false for
        // `RenderPhysicalModel` (it never exposes a public clipper), so a
        // circular or rounded-corner `RenderPhysicalModel` hit-tests as its
        // full bounding box in real Flutter. See the module doc and the
        // design research plan (`docs/research/2026-07-01-render-physical-model-plan.md`,
        // trap §4.2) for the full citation. `RenderPhysicalShape` uses the
        // same shape gate when an owner-lane path target is installed, and
        // otherwise falls back to the whole-box default clip.
        let shape = self.clip_source.compute_clip(ctx.own_size());
        if !shape.contains(Point::new(ctx.x(), ctx.y())) {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use flui_foundation::Diagnosticable;
    use flui_interaction::InteractionLane;
    use flui_types::geometry::{Radius, px};

    use super::*;

    // ---------- RectangleClip::compute_clip -------------------------------

    #[test]
    fn rectangle_clip_plain_rect_has_zero_radii_without_border_radius() {
        let source = RectangleClip {
            shape: BoxShape::Rectangle,
            border_radius: None,
        };
        let rrect = source.compute_clip(Size::new(px(100.0), px(50.0)));
        assert_eq!(rrect.top_left.x, px(0.0));
        assert_eq!(rrect.bottom_right.y, px(0.0));
    }

    #[test]
    fn rectangle_clip_border_radius_maps_corners_field_for_field() {
        let br = BorderRadius::only(
            Radius::circular(px(10.0)),
            Radius::circular(px(20.0)),
            Radius::circular(px(30.0)),
            Radius::circular(px(40.0)),
        );
        let source = RectangleClip {
            shape: BoxShape::Rectangle,
            border_radius: Some(br),
        };
        let rrect = source.compute_clip(Size::new(px(200.0), px(200.0)));
        assert_eq!(rrect.top_left.x, px(10.0));
        assert_eq!(rrect.top_right.x, px(20.0));
        assert_eq!(rrect.bottom_right.x, px(30.0));
        assert_eq!(rrect.bottom_left.x, px(40.0));
    }

    // Trap §4.4 regression: `BoxShape::Circle` must be an ELLIPSE (two
    // independent radii), not a true circle (`min(width, height) / 2`),
    // for a non-square box — oracle `proxy_box.dart:2188`.
    #[test]
    fn rectangle_clip_circle_shape_is_ellipse_not_true_circle_for_non_square_box() {
        let source = RectangleClip {
            shape: BoxShape::Circle,
            border_radius: None,
        };
        let rrect = source.compute_clip(Size::new(px(100.0), px(40.0)));
        assert_ne!(
            rrect.top_left.x, rrect.top_left.y,
            "a true-circle mis-port would give equal x/y radii; the oracle \
             formula gives independent width/2, height/2 radii"
        );
        assert_eq!(rrect.top_left.x, px(50.0));
        assert_eq!(rrect.top_left.y, px(20.0));
    }

    // ---------- PathClip::compute_clip -------------------------------------

    #[test]
    fn path_clip_falls_back_to_whole_rect_without_clipper() {
        let source = PathClip { target: None };
        let path = source.compute_clip(Size::new(px(60.0), px(30.0)));
        assert!(path.contains(Point::new(px(30.0), px(15.0))));
        assert!(!path.contains(Point::new(px(200.0), px(200.0))));
    }

    #[test]
    fn path_clip_uses_owner_local_path_target() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let target = handle
                .register_path_clipper(|size: Size| {
                    let mut p = Path::new();
                    p.add_rect(Rect::from_origin_size(
                        Point::new(px(10.0), px(10.0)),
                        Size::new(size.width - px(20.0), size.height - px(20.0)),
                    ));
                    p
                })
                .expect("register path target");
            let source = PathClip {
                target: Some(target),
            };
            let path = source.compute_clip(Size::new(px(100.0), px(100.0)));
            // Inset by 10px on each side: (5, 5) is outside, (50, 50) is inside.
            assert!(!path.contains(Point::new(px(5.0), px(5.0))));
            assert!(path.contains(Point::new(px(50.0), px(50.0))));
        });
    }

    // ---------- RRect corner hit-test (fresh, non-`ClipGeometry` impl) -----

    #[test]
    fn rrect_contains_excludes_rounded_corner_cutout() {
        let rect = Rect::from_origin_size(Point::ZERO, Size::new(px(100.0), px(100.0)));
        let rrect = RRect::from_rect_circular(rect, px(20.0));
        assert!(!PhysicalClipShape::contains(
            &rrect,
            Point::new(px(0.0), px(0.0))
        ));
        assert!(PhysicalClipShape::contains(
            &rrect,
            Point::new(px(50.0), px(50.0))
        ));
    }

    // ---------- RenderPhysicalModel / RenderPhysicalShape construction -----

    #[test]
    fn render_physical_model_defaults_match_oracle() {
        let node = RenderPhysicalModel::new(Color::RED);
        assert_eq!(node.elevation(), 0.0);
        assert_eq!(node.shadow_color(), Color::BLACK);
        // Trap §4.5: opposite of `RenderClip<S>`'s own `AntiAlias` default.
        assert_eq!(node.clip_behavior(), Clip::None);
        assert_eq!(node.shape(), BoxShape::Rectangle);
        assert_eq!(node.border_radius(), None);
    }

    #[test]
    fn render_physical_shape_defaults_match_oracle() {
        let node = RenderPhysicalShape::new(Color::BLUE);
        assert_eq!(node.elevation(), 0.0);
        assert_eq!(node.shadow_color(), Color::BLACK);
        assert_eq!(node.clip_behavior(), Clip::None);
        assert!(!node.has_custom_clipper());
    }

    // Oracle parity (`proxy_box_test.dart`, `'RenderPhysicalModel compositing'`,
    // tag `3.44.0`): `needsCompositing` stays `false` across an elevation
    // 0.0 -> 1.0 -> 0.0 round trip — casting a shadow does not by itself
    // force a dedicated compositing layer (oracle comment: on Fuchsia the
    // system compositor draws elevation shadows, but even there the
    // per-object flag this test reads stays false). `RenderPhysicalModelBase`
    // never overrides `always_needs_compositing`, so it keeps the trait's
    // `false` default regardless of elevation — this test pins that the
    // generic body does not gain an override that would diverge from the
    // oracle.
    #[test]
    fn render_physical_model_compositing_stays_false_across_elevation_changes() {
        let mut node = RenderPhysicalModel::new(Color::from_argb(0xffff_00ff));
        assert!(!node.always_needs_compositing());

        node.set_elevation(1.0);
        assert!(!node.always_needs_compositing());

        node.set_elevation(0.0);
        assert!(!node.always_needs_compositing());
    }

    // Sibling port of the same oracle contract for `RenderPhysicalShape`
    // (`proxy_box_test.dart`, `RenderPhysicalShape` group, `'compositing'`,
    // tag `3.44.0`) — both aliases share the identical generic
    // `RenderPhysicalModelBase<C>` body, so the same assertion applies
    // unmodified to the path-clip variant.
    #[test]
    fn render_physical_shape_compositing_stays_false_across_elevation_changes() {
        let mut node = RenderPhysicalShape::new(Color::from_argb(0xffff_00ff));
        assert!(!node.always_needs_compositing());

        node.set_elevation(1.0);
        assert!(!node.always_needs_compositing());

        node.set_elevation(0.0);
        assert!(!node.always_needs_compositing());
    }

    #[test]
    fn set_elevation_returns_change_flag() {
        let mut node = RenderPhysicalModel::new(Color::RED);
        assert!(node.set_elevation(4.0));
        assert!(!node.set_elevation(4.0));
    }

    #[test]
    fn set_clip_behavior_returns_change_flag() {
        let mut node = RenderPhysicalModel::new(Color::RED);
        assert!(node.set_clip_behavior(Clip::AntiAlias));
        assert!(!node.set_clip_behavior(Clip::AntiAlias));
    }

    #[test]
    fn set_shape_and_border_radius_return_change_flags() {
        let mut node = RenderPhysicalModel::new(Color::RED);
        assert!(node.set_shape(BoxShape::Circle));
        assert!(!node.set_shape(BoxShape::Circle));
        assert!(node.set_border_radius(Some(BorderRadius::circular(px(4.0)))));
        assert!(!node.set_border_radius(Some(BorderRadius::circular(px(4.0)))));
    }

    #[test]
    fn set_path_clip_target_reports_presence_change() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let target = handle
                .register_path_clipper(|size: Size| {
                    let mut p = Path::new();
                    p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                    p
                })
                .expect("register path target");
            let mut node = RenderPhysicalShape::new(Color::BLUE).with_path_clip_target(target);
            assert!(node.has_custom_clipper());
            assert!(node.set_path_clip_target(None));
            assert!(!node.has_custom_clipper());
            assert!(node.set_path_clip_target(Some(target)));
            assert!(node.has_custom_clipper());
        });
    }

    // Oracle parity (`proxy_box_test.dart`, `RenderPhysicalShape` group,
    // `'shape change triggers repaint'`, tag `3.44.0`): setting the SAME
    // clipper again reports no change; swapping to a DIFFERENT clipper must
    // report a change even though both are `Some` — a presence-only
    // comparison (`had_clipper != has_clipper`) would wrongly miss this swap
    // since presence never toggles. Bug fixed in the same change as this
    // test: `set_path_clip_target` now compares the full
    // `Option<PathClipTarget>`, not just its `is_some()` presence.
    //
    // FLUI has no `RenderPhysicalShape`-consuming widget wired up yet (no
    // `PhysicalShape` view exists in `flui-widgets`), so this cannot be
    // ported at the oracle's own fidelity (`debugNeedsPaint` after a real
    // `layout()`/`pumpFrame()`) — there is no `mark_needs_paint()` consumer
    // of this setter's return value to observe yet. This tests the setter's
    // own change-detection contract instead, the precondition any future
    // widget-diff layer will rely on.
    #[test]
    fn set_path_clip_target_detects_target_swap_not_just_presence() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let target_a = handle
                .register_path_clipper(|size: Size| {
                    let mut p = Path::new();
                    p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                    p
                })
                .expect("register target_a");
            let target_b = handle
                .register_path_clipper(|size: Size| {
                    let mut p = Path::new();
                    p.add_rect(Rect::from_origin_size(
                        Point::ZERO,
                        Size::new(size.width * 0.5, size.height * 0.5),
                    ));
                    p
                })
                .expect("register target_b");

            let mut node = RenderPhysicalShape::new(Color::BLUE).with_path_clip_target(target_a);

            // "Same shape, no repaint" (oracle).
            assert!(
                !node.set_path_clip_target(Some(target_a)),
                "re-setting the identical target must report no change"
            );
            // "Different shape triggers repaint" (oracle) — both sides are
            // `Some`, so presence alone cannot distinguish this case.
            assert!(
                node.set_path_clip_target(Some(target_b)),
                "swapping to a distinct target must report a change even \
                 though presence (Some -> Some) never toggles"
            );
            assert_eq!(node.path_clip_target(), Some(target_b));
        });
    }

    // Trap §4.1 regression: diagnostics must surface the real
    // `shadow_color`, not `color` twice (confirmed oracle bug, not
    // reproduced).
    #[test]
    fn diagnostics_surface_real_shadow_color_not_color_bug() {
        let node = RenderPhysicalModel::new(Color::RED).with_shadow_color(Color::BLUE);
        let mut builder = flui_foundation::DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let properties = builder.build();
        let shadow_color_property = properties
            .iter()
            .find(|p| p.name() == "shadow_color")
            .expect("shadow_color property must be present");
        assert_eq!(shadow_color_property.value(), format!("{:?}", Color::BLUE));
    }

    #[test]
    fn to_diagnostics_node_uses_flutter_parity_alias_names() {
        assert_eq!(
            RenderPhysicalModel::new(Color::RED)
                .to_diagnostics_node()
                .name(),
            Some("RenderPhysicalModel")
        );
        let shape_node = RenderPhysicalShape::new(Color::BLUE);
        assert_eq!(
            shape_node.to_diagnostics_node().name(),
            Some("RenderPhysicalShape")
        );
    }
}
