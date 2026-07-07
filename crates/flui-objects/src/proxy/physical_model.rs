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
//! derived from `size` differs (`BoxShape` + `BorderRadius` vs. a mandatory
//! [`CustomClipper<Path>`]). That is collapsed to one generic body,
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

use std::{fmt, sync::Arc};

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
    parent_data::BoxParentData,
    traits::RenderBox,
};

use super::clip::CustomClipper;

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

/// [`RenderPhysicalShape`]'s clip source: an arbitrary [`CustomClipper<Path>`].
///
/// Stored as `Option` (matching the oracle's nullable base-class `clipper`
/// field) even though [`RenderPhysicalModelBase::new`] on the `PathClip`
/// variant always supplies one — the oracle's own inherited setter allows
/// clearing it back to the whole-box rectangle default.
#[derive(Clone)]
pub struct PathClip {
    /// The active clipper, or `None` to fall back to the whole-box rect.
    pub clipper: Option<CustomClipper<Path>>,
}

impl fmt::Debug for PathClip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PathClip")
            .field("has_custom_clipper", &self.clipper.is_some())
            .finish()
    }
}

impl PhysicalClipSource for PathClip {
    type Shape = Path;
    const DIAGNOSTIC_NAME: &'static str = "RenderPhysicalShape";

    fn compute_clip(&self, size: Size) -> Path {
        if let Some(clipper) = &self.clipper {
            clipper(size)
        } else {
            // Oracle `:2296`'s `_defaultClip` fallback — only reachable
            // once a clipper is cleared via `set_clipper(None)`.
            let mut path = Path::new();
            path.add_rect(Rect::from_origin_size(Point::ZERO, size));
            path
        }
    }

    fn debug_fill_extra(&self, builder: &mut DiagnosticsBuilder) {
        builder.add_flag(
            "custom_clipper",
            self.clipper.is_some(),
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
/// * [`RenderPhysicalShape`] — arbitrary [`CustomClipper<Path>`] clip source.
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

/// Arbitrary-path-clipper variant — Flutter's `RenderPhysicalShape`.
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
    /// Creates a `RenderPhysicalShape` with a mandatory clipper — Flutter
    /// parity: `RenderPhysicalShape`'s `clipper` constructor argument is
    /// `required`, unlike `RenderPhysicalModel`'s total absence of one.
    pub fn new<F>(clipper: F, color: Color) -> Self
    where
        F: Fn(Size) -> Path + Send + Sync + 'static,
    {
        Self::with_clip_source(
            PathClip {
                clipper: Some(Arc::new(clipper)),
            },
            color,
        )
    }

    /// Whether a clipper is currently installed.
    #[inline]
    pub fn has_custom_clipper(&self) -> bool {
        self.clip_source.clipper.is_some()
    }

    /// Replaces the clipper; returns `true` if presence changed. `None`
    /// falls back to the whole-box rectangle default clip (oracle `:2296`).
    pub fn set_clipper<F>(&mut self, clipper: Option<F>) -> bool
    where
        F: Fn(Size) -> Path + Send + Sync + 'static,
    {
        let had_clipper = self.clip_source.clipper.is_some();
        let has_clipper = clipper.is_some();
        self.clip_source.clipper = clipper.map(|c| Arc::new(c) as CustomClipper<Path>);
        had_clipper != has_clipper
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
        // trap §4.2) for the full citation. `RenderPhysicalShape` always
        // has a clipper (mandatory constructor arg) so the oracle and this
        // convention already agree there.
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
        let source = PathClip { clipper: None };
        let path = source.compute_clip(Size::new(px(60.0), px(30.0)));
        assert!(path.contains(Point::new(px(30.0), px(15.0))));
        assert!(!path.contains(Point::new(px(200.0), px(200.0))));
    }

    #[test]
    fn path_clip_uses_installed_custom_clipper() {
        let source = PathClip {
            clipper: Some(Arc::new(|size: Size| {
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(
                    Point::new(px(10.0), px(10.0)),
                    Size::new(size.width - px(20.0), size.height - px(20.0)),
                ));
                p
            })),
        };
        let path = source.compute_clip(Size::new(px(100.0), px(100.0)));
        // Inset by 10px on each side: (5, 5) is outside, (50, 50) is inside.
        assert!(!path.contains(Point::new(px(5.0), px(5.0))));
        assert!(path.contains(Point::new(px(50.0), px(50.0))));
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
        let node = RenderPhysicalShape::new(
            |size: Size| {
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                p
            },
            Color::BLUE,
        );
        assert_eq!(node.elevation(), 0.0);
        assert_eq!(node.shadow_color(), Color::BLACK);
        assert_eq!(node.clip_behavior(), Clip::None);
        assert!(node.has_custom_clipper());
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
    fn set_clipper_reports_presence_change() {
        let mut node = RenderPhysicalShape::new(
            |size: Size| {
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                p
            },
            Color::BLUE,
        );
        assert!(node.set_clipper::<fn(Size) -> Path>(None));
        assert!(!node.has_custom_clipper());
        assert!(node.set_clipper(Some(|size: Size| {
            let mut p = Path::new();
            p.add_rect(Rect::from_origin_size(Point::ZERO, size));
            p
        })));
        assert!(node.has_custom_clipper());
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
        let shape_node = RenderPhysicalShape::new(
            |size: Size| {
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                p
            },
            Color::BLUE,
        );
        assert_eq!(
            shape_node.to_diagnostics_node().name(),
            Some("RenderPhysicalShape")
        );
    }
}
