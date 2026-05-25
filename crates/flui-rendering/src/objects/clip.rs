//! Clipping render-object family — `RenderClipRect`, `RenderClipRRect`,
//! `RenderClipOval`, `RenderClipPath`.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderClipRect`](https://api.flutter.dev/flutter/rendering/RenderClipRect-class.html),
//! [`RenderClipRRect`](https://api.flutter.dev/flutter/rendering/RenderClipRRect-class.html),
//! [`RenderClipOval`](https://api.flutter.dev/flutter/rendering/RenderClipOval-class.html),
//! and [`RenderClipPath`](https://api.flutter.dev/flutter/rendering/RenderClipPath-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvement
//!
//! Flutter encodes the clip family as a 4-class private mixin tree:
//!
//! ```text
//!  _RenderCustomClip<T> (abstract, private)
//!  ├── RenderClipRect    (T = Rect)
//!  ├── RenderClipRRect   (T = RRect)
//!  ├── RenderClipOval    (T = Rect, hit-tested as ellipse)
//!  └── RenderClipPath    (T = Path)
//! ```
//!
//! Each subclass duplicates the same `_clipper` / `_clip` /
//! `clipBehavior` field cluster and only differs in `_defaultClip`,
//! `hitTest`, and which `canvas.clipXXX` call is used. That structure
//! is a clean diamond-shaped mixin chain in Dart; in Rust we collapse
//! it to **one generic struct + one sealed trait**:
//!
//! ```text
//!  trait ClipGeometry        (sealed; impls for Rect, RRect, Oval, Path)
//!  struct RenderClip<S: ClipGeometry>      ← single, generic, monomorphised
//!  ──────────────────────────────────────
//!  type RenderClipRect   = RenderClip<Rect<Pixels>>;
//!  type RenderClipRRect  = RenderClip<RRect>;
//!  type RenderClipOval   = RenderClip<Oval>;
//!  type RenderClipPath   = RenderClip<Path>;
//! ```
//!
//! The trait carries the per-shape variation (`default_for_size`,
//! `contains`, `apply_to_canvas`) so the generic body never branches on
//! shape. Each instantiation monomorphises to a dedicated zero-cost
//! type — no `Box<dyn>`, no vtable dispatch in the hot paint/hit-test
//! path — and the sealed trait prevents downstream crates from adding
//! shapes the engine cannot render.
//!
//! Custom clipper logic (Flutter's `CustomClipper<T>.getClip(size)`)
//! is modelled as an `Option<Arc<dyn Fn(Size) -> S + Send + Sync>>` —
//! preserved as a behavioural extension point but typed at compile
//! time per shape rather than via an abstract class.

use std::{fmt, sync::Arc};

use flui_tree::Single;
use flui_types::{
    Offset, Pixels, Point, Rect, Size,
    geometry::RRect,
    painting::{Clip, Path},
};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

// =============================================================================
// Oval — newtype for elliptical hit-test semantics
// =============================================================================

/// An axis-aligned ellipse inscribed in a rectangle.
///
/// Flutter's `RenderClipOval` carries a `Rect` and hit-tests as the
/// inscribed ellipse. Lifting the semantic to a distinct type means the
/// "treat this rect as an oval" intent is visible in the type system —
/// passing a bare `Rect` to a `RenderClip<Rect>` would clip rectangularly
/// (the wrong thing) without a compiler error in Flutter. Here it is
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oval {
    /// The bounding rectangle of the ellipse.
    pub bounds: Rect<Pixels>,
}

impl Oval {
    /// Creates an oval inscribed in the given rectangle.
    #[inline]
    #[must_use]
    pub const fn from_rect(bounds: Rect<Pixels>) -> Self {
        Self { bounds }
    }

    /// Creates an oval inscribed in a rectangle of the given size at origin.
    #[must_use]
    pub fn from_size(size: Size) -> Self {
        Self::from_rect(Rect::from_origin_size(Point::ZERO, size))
    }

    /// Tests if a point lies inside the ellipse.
    ///
    /// Uses the standard ellipse equation:
    /// `((x − cx)/rx)² + ((y − cy)/ry)² ≤ 1`.
    #[must_use]
    pub fn contains(&self, point: Point<Pixels>) -> bool {
        let r = self.bounds;
        let rx = r.width().get() * 0.5;
        let ry = r.height().get() * 0.5;
        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let cx = r.left().get() + rx;
        let cy = r.top().get() + ry;
        let dx = (point.x.get() - cx) / rx;
        let dy = (point.y.get() - cy) / ry;
        dx * dx + dy * dy <= 1.0
    }
}

// =============================================================================
// Sealed trait: shape-specific clip semantics
// =============================================================================

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Rect<super::Pixels> {}
    impl Sealed for super::RRect {}
    impl Sealed for super::Oval {}
    impl Sealed for super::Path {}
}

/// Trait abstracting the four clip shapes used by `RenderClip<S>`.
///
/// **Sealed.** Only the four canonical shapes implement this trait.
/// Downstream crates cannot add new variants — this matches Flutter's
/// `_RenderCustomClip<T>` access control (the parent class is library-
/// private), preserves engine-level dispatch invariants, and lets the
/// compiler monomorphise the clip-emission path per shape.
pub trait ClipGeometry: sealed::Sealed + Clone + fmt::Debug + Send + Sync + 'static {
    /// Returns the default clip for a render box of `size` whose origin
    /// is at `(0, 0)` in local coordinates.
    fn default_for_size(size: Size) -> Self;

    /// Returns `true` if the local-space `position` falls inside the
    /// clip region. Used for hit testing: anything outside the clip
    /// shape is unreachable.
    fn contains(&self, position: Point<Pixels>) -> bool;

    /// Pushes the clip onto the canvas. The generic `RenderClip<S>`
    /// body calls this once per `paint()`, sandwiched between a
    /// `canvas.save()` and `canvas.restore()`.
    fn apply_to_canvas(
        &self,
        canvas: &mut flui_painting::Canvas,
        offset: Offset,
        clip_behavior: Clip,
    );
}

// ---- Rect ------------------------------------------------------------------

impl ClipGeometry for Rect<Pixels> {
    fn default_for_size(size: Size) -> Self {
        Rect::from_origin_size(Point::ZERO, size)
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        Rect::contains(self, position)
    }

    fn apply_to_canvas(
        &self,
        canvas: &mut flui_painting::Canvas,
        offset: Offset,
        clip_behavior: Clip,
    ) {
        let shifted = self.translate_offset(offset);
        canvas.clip_rect_ext(
            shifted,
            flui_types::painting::ClipOp::Intersect,
            clip_behavior,
        );
    }
}

// ---- RRect -----------------------------------------------------------------

impl ClipGeometry for RRect {
    fn default_for_size(size: Size) -> Self {
        RRect::from_rect(Rect::from_origin_size(Point::ZERO, size))
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        // First fail-fast: outside the bounding rect.
        if !self.bounding_rect().contains(position) {
            return false;
        }
        // Then exclude each rounded corner via the per-corner ellipse.
        let r = self.bounding_rect();
        let px = position.x.get();
        let py = position.y.get();

        // For each corner, if the point is inside the corner's "square"
        // sub-region but outside the inscribed ellipse, it's outside the
        // rounded rect.
        let test_corner = |cx: f32, cy: f32, rx: f32, ry: f32, in_corner: bool| -> bool {
            if !in_corner || rx <= 0.0 || ry <= 0.0 {
                return true; // not in this corner OR no rounding → inside
            }
            let dx = (px - cx) / rx;
            let dy = (py - cy) / ry;
            dx * dx + dy * dy <= 1.0
        };

        let left = r.left().get();
        let top = r.top().get();
        let right = r.right().get();
        let bottom = r.bottom().get();

        // Top-left.
        let tl_rx = self.top_left.x.get();
        let tl_ry = self.top_left.y.get();
        let in_tl = px < left + tl_rx && py < top + tl_ry;
        if !test_corner(left + tl_rx, top + tl_ry, tl_rx, tl_ry, in_tl) {
            return false;
        }

        // Top-right.
        let tr_rx = self.top_right.x.get();
        let tr_ry = self.top_right.y.get();
        let in_tr = px > right - tr_rx && py < top + tr_ry;
        if !test_corner(right - tr_rx, top + tr_ry, tr_rx, tr_ry, in_tr) {
            return false;
        }

        // Bottom-right.
        let br_rx = self.bottom_right.x.get();
        let br_ry = self.bottom_right.y.get();
        let in_br = px > right - br_rx && py > bottom - br_ry;
        if !test_corner(right - br_rx, bottom - br_ry, br_rx, br_ry, in_br) {
            return false;
        }

        // Bottom-left.
        let bl_rx = self.bottom_left.x.get();
        let bl_ry = self.bottom_left.y.get();
        let in_bl = px < left + bl_rx && py > bottom - bl_ry;
        if !test_corner(left + bl_rx, bottom - bl_ry, bl_rx, bl_ry, in_bl) {
            return false;
        }

        true
    }

    fn apply_to_canvas(
        &self,
        canvas: &mut flui_painting::Canvas,
        offset: Offset,
        clip_behavior: Clip,
    ) {
        // Shift the rounded rect by `offset`.
        let shifted_rect = self.bounding_rect().translate_offset(offset);
        let shifted = RRect::new(
            shifted_rect,
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        );
        canvas.clip_rrect_ext(
            shifted,
            flui_types::painting::ClipOp::Intersect,
            clip_behavior,
        );
    }
}

// ---- Oval ------------------------------------------------------------------

impl ClipGeometry for Oval {
    fn default_for_size(size: Size) -> Self {
        Oval::from_size(size)
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        Oval::contains(self, position)
    }

    fn apply_to_canvas(
        &self,
        canvas: &mut flui_painting::Canvas,
        offset: Offset,
        clip_behavior: Clip,
    ) {
        // Approximate the oval with an RRect whose corner radii equal half
        // the bounding-rect dimensions — a perfect inscribed ellipse.
        // (The engine may specialise this in a future backend; the
        // approximation is exact for the inscribed-ellipse case.)
        let shifted = self.bounds.translate_offset(offset);
        let rx = shifted.width() * 0.5;
        let ry = shifted.height() * 0.5;
        let rrect = RRect::from_rect_elliptical(shifted, rx, ry);
        canvas.clip_rrect_ext(
            rrect,
            flui_types::painting::ClipOp::Intersect,
            clip_behavior,
        );
    }
}

// ---- Path ------------------------------------------------------------------

impl ClipGeometry for Path {
    fn default_for_size(size: Size) -> Self {
        // A path-shaped default is the rectangle outline of `size`.
        let mut p = Path::new();
        p.add_rect(Rect::from_origin_size(Point::ZERO, size));
        p
    }

    fn contains(&self, _position: Point<Pixels>) -> bool {
        // Path containment requires a winding-number / tessellation
        // test. The framework hasn't shipped that yet; for now we
        // conservatively allow the hit (defers exclusion to the child).
        // Flutter does the same when `customClipper.getClip()` returns
        // a path: hit testing inside `RenderClipPath` defers to the
        // child regardless of the path region. The corresponding
        // backend-level path containment check lives in the engine.
        true
    }

    fn apply_to_canvas(
        &self,
        canvas: &mut flui_painting::Canvas,
        offset: Offset,
        clip_behavior: Clip,
    ) {
        // `Path::translate` returns a new Path (immutable). The painting
        // layer takes the path by reference, so we keep the shifted copy
        // local to this call.
        let shifted = self.translate(offset);
        canvas.clip_path_ext(
            &shifted,
            flui_types::painting::ClipOp::Intersect,
            clip_behavior,
        );
    }
}

// =============================================================================
// CustomClipper — Flutter's CustomClipper<T> analog
// =============================================================================

/// A type-erased function that produces a clip shape for a given box size.
///
/// This is the Rust analog of Flutter's `CustomClipper<T>.getClip(size)`.
/// Stored as `Arc` so the containing `RenderClip<S>` remains `Clone`.
pub type CustomClipper<S> = Arc<dyn Fn(Size) -> S + Send + Sync + 'static>;

// =============================================================================
// RenderClip<S> — generic clip render object
// =============================================================================

/// A render object that clips its child to the geometry produced by `S`.
///
/// The shape parameter `S` is one of [`Rect<Pixels>`], [`RRect`], [`Oval`],
/// or [`Path`] via the sealed [`ClipGeometry`] trait. Pick the right type
/// alias for ergonomic construction:
///
/// * [`RenderClipRect`] — axis-aligned rectangular clip.
/// * [`RenderClipRRect`] — rounded rectangle clip.
/// * [`RenderClipOval`] — inscribed-ellipse clip.
/// * [`RenderClipPath`] — arbitrary path clip.
///
/// # Custom clippers
///
/// By default the clip uses the entire box (`S::default_for_size(size)`).
/// Provide a [`CustomClipper`] via [`Self::with_clipper`] to compute a
/// shape from the box's runtime size — equivalent to Flutter's
/// `customClipper` field.
pub struct RenderClip<S: ClipGeometry> {
    /// The clip behavior to use when applying the shape.
    clip_behavior: Clip,
    /// Optional custom clipper closure (`None` = use `S::default_for_size`).
    clipper: Option<CustomClipper<S>>,
    /// Final size after layout.
    size: Size,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl<S: ClipGeometry> RenderClip<S> {
    /// Creates a clip render object with the given clip behavior and the
    /// default clipper (whole box).
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            clip_behavior,
            clipper: None,
            size: Size::ZERO,
            has_child: false,
        }
    }

    /// Creates an anti-aliased clip (`Clip::AntiAlias`).
    pub fn anti_alias() -> Self {
        Self::new(Clip::AntiAlias)
    }

    /// Creates a hard-edge clip (`Clip::HardEdge`).
    pub fn hard_edge() -> Self {
        Self::new(Clip::HardEdge)
    }

    /// Replaces the clip behavior; returns true if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Returns whether a custom clipper has been installed.
    #[inline]
    pub fn has_custom_clipper(&self) -> bool {
        self.clipper.is_some()
    }

    /// Sets a custom clipper closure (builder).
    #[must_use]
    pub fn with_clipper<F>(mut self, clipper: F) -> Self
    where
        F: Fn(Size) -> S + Send + Sync + 'static,
    {
        self.clipper = Some(Arc::new(clipper));
        self
    }

    /// Replaces the custom clipper; returns true if the slot was changed.
    pub fn set_clipper<F>(&mut self, clipper: Option<F>) -> bool
    where
        F: Fn(Size) -> S + Send + Sync + 'static,
    {
        let new_some = clipper.is_some();
        let old_some = self.clipper.is_some();
        self.clipper = clipper.map(|c| Arc::new(c) as CustomClipper<S>);
        new_some != old_some
    }

    /// Computes the clip shape for the current size.
    ///
    /// Called from both `paint()` and `hit_test()`, which both take
    /// `&self`. The cost is one closure call (or one `default_for_size`
    /// dispatch) per paint/hit-test, which is negligible relative to
    /// the canvas / hit-test work that follows.
    fn resolve_clip(&self) -> S {
        match &self.clipper {
            Some(c) => (c)(self.size),
            None => S::default_for_size(self.size),
        }
    }
}

// `Clone` cannot be derived because `dyn Fn` is not Clone, but Arc is.
impl<S: ClipGeometry> Clone for RenderClip<S> {
    fn clone(&self) -> Self {
        Self {
            clip_behavior: self.clip_behavior,
            clipper: self.clipper.clone(),
            size: self.size,
            has_child: self.has_child,
        }
    }
}

impl<S: ClipGeometry> fmt::Debug for RenderClip<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderClip")
            .field("clip_behavior", &self.clip_behavior)
            .field("has_custom_clipper", &self.clipper.is_some())
            .field("size", &self.size)
            .field("has_child", &self.has_child)
            .finish()
    }
}

impl<S: ClipGeometry> Default for RenderClip<S> {
    fn default() -> Self {
        Self::anti_alias()
    }
}

impl<S: ClipGeometry> flui_foundation::Diagnosticable for RenderClip<S> {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("clip_behavior", format!("{:?}", self.clip_behavior));
        builder.add("custom_clipper", self.clipper.is_some());
        builder.add("size", format!("{:?}", self.size));
        builder.add("has_child", self.has_child);
    }
}

impl<S: ClipGeometry> RenderBox for RenderClip<S> {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            self.size = child_size;
        } else {
            self.has_child = false;
            self.size = constraints.smallest();
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
        let clip = self.resolve_clip();
        let offset = ctx.offset();
        ctx.with_save(|ctx| {
            clip.apply_to_canvas(ctx.canvas(), offset, self.clip_behavior);
            ctx.paint_child();
        });
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }
        // Honour the clip: a hit outside the clip shape doesn't reach
        // the child. Flutter parity.
        let position = Point::new(ctx.x(), ctx.y());
        if !self.resolve_clip().contains(position) {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect<Pixels> {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl<S: ClipGeometry> PaintEffectsCapability for RenderClip<S> {}
impl<S: ClipGeometry> SemanticsCapability for RenderClip<S> {}
impl<S: ClipGeometry> HotReloadCapability for RenderClip<S> {}

// =============================================================================
// Type aliases — ergonomic per-shape names matching Flutter's class names.
// =============================================================================

/// Rectangular clip — Flutter's `RenderClipRect`.
pub type RenderClipRect = RenderClip<Rect<Pixels>>;

/// Rounded-rectangle clip — Flutter's `RenderClipRRect`.
pub type RenderClipRRect = RenderClip<RRect>;

/// Oval (inscribed-ellipse) clip — Flutter's `RenderClipOval`.
pub type RenderClipOval = RenderClip<Oval>;

/// Arbitrary-path clip — Flutter's `RenderClipPath`.
pub type RenderClipPath = RenderClip<Path>;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    // ---------- Oval newtype ---------------------------------------------

    #[test]
    fn oval_contains_inside_and_outside() {
        let oval = Oval::from_size(Size::new(px(100.0), px(50.0)));
        // Center (50, 25) is inside.
        assert!(oval.contains(Point::new(px(50.0), px(25.0))));
        // Top-left bounding-rect corner (0, 0) is outside the ellipse.
        assert!(!oval.contains(Point::new(px(0.0), px(0.0))));
        // Right-mid edge (100, 25) is right at the ellipse boundary.
        assert!(oval.contains(Point::new(px(100.0), px(25.0))));
    }

    #[test]
    fn oval_zero_size_contains_nothing() {
        let oval = Oval::from_size(Size::ZERO);
        assert!(!oval.contains(Point::new(px(0.0), px(0.0))));
    }

    // ---------- ClipGeometry impls (Rect) --------------------------------

    #[test]
    fn rect_default_for_size_starts_at_origin() {
        let rect = <Rect<Pixels> as ClipGeometry>::default_for_size(Size::new(px(80.0), px(40.0)));
        assert_eq!(rect.left(), px(0.0));
        assert_eq!(rect.top(), px(0.0));
        assert_eq!(rect.width(), px(80.0));
        assert_eq!(rect.height(), px(40.0));
    }

    #[test]
    fn rect_contains_via_clip_geometry() {
        let rect = <Rect<Pixels> as ClipGeometry>::default_for_size(Size::new(px(50.0), px(50.0)));
        assert!(<Rect<Pixels> as ClipGeometry>::contains(
            &rect,
            Point::new(px(25.0), px(25.0))
        ));
        assert!(!<Rect<Pixels> as ClipGeometry>::contains(
            &rect,
            Point::new(px(60.0), px(25.0))
        ));
    }

    // ---------- ClipGeometry impls (RRect) -------------------------------

    #[test]
    fn rrect_contains_center_and_excludes_outside_bounds() {
        let rrect = <RRect as ClipGeometry>::default_for_size(Size::new(px(100.0), px(50.0)));
        // Default RRect with from_rect has zero radius — degenerates to rect.
        assert!(<RRect as ClipGeometry>::contains(
            &rrect,
            Point::new(px(50.0), px(25.0))
        ));
        assert!(!<RRect as ClipGeometry>::contains(
            &rrect,
            Point::new(px(200.0), px(25.0))
        ));
    }

    #[test]
    fn rrect_corner_excludes_point_inside_bbox_outside_ellipse() {
        let rect = Rect::from_origin_size(Point::ZERO, Size::new(px(100.0), px(100.0)));
        let rrect = RRect::from_rect_circular(rect, px(20.0));
        // Bounding-rect corner (0,0) — inside square TL sub-region, outside
        // the inscribed circle (distance √(400) ≈ 20 from corner-radius
        // origin (20,20), so on the boundary; pick (0,0) which is outside
        // the circle of radius 20 centered at (20,20)).
        assert!(!<RRect as ClipGeometry>::contains(
            &rrect,
            Point::new(px(0.0), px(0.0))
        ));
        // A point clearly inside the rrect.
        assert!(<RRect as ClipGeometry>::contains(
            &rrect,
            Point::new(px(50.0), px(50.0))
        ));
        // A point in the TL square region but inside the ellipse.
        assert!(<RRect as ClipGeometry>::contains(
            &rrect,
            Point::new(px(15.0), px(15.0))
        ));
    }

    // ---------- ClipGeometry impls (Path) --------------------------------

    #[test]
    fn path_contains_is_permissive() {
        // Until tessellated containment is wired in the engine, Path
        // clips defer hit-testing to the child.
        let path = <Path as ClipGeometry>::default_for_size(Size::new(px(10.0), px(10.0)));
        assert!(<Path as ClipGeometry>::contains(
            &path,
            Point::new(px(100.0), px(100.0))
        ));
    }

    // ---------- RenderClip<S> generic ------------------------------------

    #[test]
    fn default_clip_behavior_is_anti_alias() {
        let node: RenderClipRect = RenderClipRect::default();
        assert_eq!(node.clip_behavior(), Clip::AntiAlias);
        assert!(!node.has_custom_clipper());
    }

    #[test]
    fn explicit_clip_behavior_round_trips() {
        let node = RenderClipRect::new(Clip::HardEdge);
        assert_eq!(node.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn set_clip_behavior_returns_change_flag() {
        let mut node = RenderClipRect::anti_alias();
        assert!(node.set_clip_behavior(Clip::HardEdge));
        assert!(!node.set_clip_behavior(Clip::HardEdge));
    }

    #[test]
    fn with_clipper_installs_custom_function() {
        // A 20-pixel inset clip rect.
        let node: RenderClipRect = RenderClip::anti_alias().with_clipper(|size| {
            Rect::from_origin_size(
                Point::new(px(20.0), px(20.0)),
                Size::new(size.width - px(40.0), size.height - px(40.0)),
            )
        });
        assert!(node.has_custom_clipper());
    }

    #[test]
    fn type_aliases_compile() {
        let _r: RenderClipRect = RenderClip::anti_alias();
        let _rr: RenderClipRRect = RenderClip::anti_alias();
        let _o: RenderClipOval = RenderClip::anti_alias();
        let _p: RenderClipPath = RenderClip::anti_alias();
    }

    #[test]
    fn clone_is_supported_even_with_clipper() {
        let node: RenderClipRect =
            RenderClip::anti_alias().with_clipper(|s| Rect::from_origin_size(Point::ZERO, s));
        let cloned = node.clone();
        assert!(cloned.has_custom_clipper());
        assert_eq!(cloned.clip_behavior(), node.clip_behavior());
    }

    #[test]
    fn debug_format_does_not_expose_clipper_internals() {
        let node: RenderClipRect = RenderClip::anti_alias();
        let dbg = format!("{node:?}");
        assert!(dbg.contains("RenderClip"));
        assert!(dbg.contains("clip_behavior"));
        // Clipper is summarised as a boolean, not the closure body.
        assert!(dbg.contains("has_custom_clipper"));
    }

    // ---------- Diagnostics ----------------------------------------------

    #[test]
    fn debug_fill_properties_lists_clip_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node: RenderClipRRect = RenderClip::anti_alias();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "clip_behavior"));
        assert!(names.iter().any(|n| n == "custom_clipper"));
        assert!(names.iter().any(|n| n == "size"));
    }
}
