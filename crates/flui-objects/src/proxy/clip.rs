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
//! Custom clipping is split at the ownership boundary. Render objects store
//! data-only shape configuration (`BorderRadius` for rounded rectangles) or an
//! owner-lane [`PathClipTarget`] for path factories; executable clipper
//! callbacks never live in render storage.

use std::{borrow::Borrow, fmt, marker::PhantomData, sync::Arc};

use flui_tree::Single;
use flui_types::{
    Offset, Pixels, Point, Rect, Size,
    geometry::RRect,
    painting::{Clip, Path},
    styling::BorderRadius,
};

use flui_rendering::{
    RenderUpdateImpact,
    context::BoxHitTestContext,
    hit_testing::PathClipTarget,
    parent_data::BoxParentData,
    traits::{PaintClip, PaintEffects, RenderBox, resolve_path_clip},
};

#[derive(Debug)]
struct ClipSourceMarker;

/// Stable identity for one widget-owned closure clip source.
///
/// A source token lets a render object distinguish a cloned widget
/// configuration from a separately constructed one without exposing callback
/// pointers or an integer identity protocol. Cloning the token preserves source
/// identity; call [`Self::fresh`] when constructing a new source.
///
/// # It answers a question Rust cannot
///
/// Two closures cannot be compared, so "did the clip change" has no structural
/// answer for a callback-shaped clip source, and this token is the substitute:
/// a new identity means a changed clip, and the render object repaints. That
/// makes minting one **not free** — under the ordinary pattern of building a
/// view fresh on every rebuild, a token minted in the constructor repaints the
/// clipped subtree every frame the surrounding tree rebuilds.
///
/// Prefer a clip that is DATA where the shape allows it: `ClipRect`,
/// `ClipOval` and `ClipRRect` take values compared with `==`, so they need no
/// token and cannot have this cost. This exists for `ClipPath`, where the
/// shape genuinely is a function the caller supplies.
///
/// The representation is deliberately private and has no accessor or raw-value
/// constructor.
///
/// ```
/// use flui_objects::ClipSourceToken;
///
/// let source = ClipSourceToken::fresh();
/// assert_eq!(source, source);
/// assert_ne!(source, ClipSourceToken::fresh());
/// ```
///
/// ```compile_fail
/// use flui_objects::ClipSourceToken;
///
/// let _ = ClipSourceToken(1);
/// ```
///
/// ```compile_fail
/// use flui_objects::ClipSourceToken;
///
/// let raw = ClipSourceToken::fresh().get();
/// # let _ = raw;
/// ```
#[derive(Clone)]
pub struct ClipSourceToken(Arc<ClipSourceMarker>);

impl fmt::Debug for ClipSourceToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClipSourceToken(..)")
    }
}

impl PartialEq for ClipSourceToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ClipSourceToken {}

impl ClipSourceToken {
    /// Allocates a new source identity.
    #[must_use]
    pub fn fresh() -> Self {
        Self(Arc::new(ClipSourceMarker))
    }
}

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
pub trait ClipGeometry:
    sealed::Sealed + Clone + fmt::Debug + PartialEq + Send + Sync + 'static
{
    /// Flutter-parity diagnostics label (`RenderClipRect`, `RenderClipRRect`, …).
    ///
    /// Generic `RenderClip<S>` would otherwise surface as
    /// `RenderClip<Rect<Pixels>>` via `type_name`, which breaks structured
    /// tree queries in the render harness.
    const DIAGNOSTIC_NAME: &'static str;

    /// Returns the default clip for a render box of `size` whose origin
    /// is at `(0, 0)` in local coordinates.
    fn default_for_size(size: Size) -> Self;

    /// Returns `true` if the local-space `position` falls inside the
    /// clip region. Used for hit testing: anything outside the clip
    /// shape is unreachable.
    fn contains(&self, position: Point<Pixels>) -> bool;

    /// An axis-aligned rect that CONTAINS this clip, in local coordinates.
    ///
    /// A conservative superset is allowed and expected — "approximate" is the
    /// reference's own word for it, and the semantics walk that consumes this
    /// only needs to know what is certainly outside. Returning something
    /// SMALLER than the true clip would drop content from the accessibility
    /// tree that is really on screen, which is the one direction that is a
    /// defect rather than a missed optimisation.
    fn approximate_bounds(&self, size: Size) -> Rect<Pixels>;

    /// Resolves an owner-local path clip target for this geometry, when the
    /// shape supports it.
    ///
    /// Only [`Path`] implements this; other clip geometries remain pure data
    /// and ignore owner-lane path targets.
    fn resolve_path_clip_target(_target: PathClipTarget, _size: Size) -> Option<Self> {
        None
    }

    /// Produces a node-local [`PaintClip`] descriptor for an owner-lane path
    /// clip target, when this shape supports one — data only, no user code.
    ///
    /// Only [`Path`] overrides this, and it returns the TOKEN as a
    /// [`PaintClip::PathTarget`] rather than a resolved path: the paint walk
    /// resolves it through [`resolve_path_clip`] inside the paint frame, so
    /// building this descriptor from [`RenderClip::paint_effects`] on a
    /// coordinate query (the default `apply_paint_transform`, outside any
    /// paint walk) runs no registered clipper. [`Self::resolve_path_clip_target`]
    /// — used by `RenderClip::resolve_clip` for hit-test and semantics —
    /// resolves the SAME target eagerly through the same helper, so all
    /// three agree on the shape. Other clip geometries ignore owner-lane
    /// path targets and keep the default `None`.
    fn path_target_descriptor(
        _target: PathClipTarget,
        _size: Size,
        _behavior: Clip,
    ) -> Option<PaintClip> {
        None
    }

    /// Resolves a data-only rounded-rect border-radius source for this
    /// geometry, when the shape supports it.
    ///
    /// Only [`RRect`] implements this; other clip geometries remain pure
    /// defaults or owner-lane path targets.
    fn resolve_rrect_border_radius(_border_radius: BorderRadius, _size: Size) -> Option<Self> {
        None
    }

    /// The representation `RenderClip<Self>` keeps once a shape is set: `Self`
    /// for the three value shapes, `Arc<Path>` for `Path`, so a stored path is
    /// shared by refcount on every `resolve_clip`/`to_paint_clip` call and
    /// never deep-cloned. `Self: Into<Self::Stored>` is std's own
    /// `From<T> for T` / `From<T> for Arc<T>` — no per-shape conversion code.
    type Stored: Borrow<Self> + Clone + fmt::Debug + Send + Sync + 'static + From<Self>;

    /// Produces the node-local [`PaintClip`] descriptor for this shape at the
    /// given `behavior`. [`RenderClip::paint_effects`] reports the result,
    /// and the pipeline opens a LAYER scope around the node's entire
    /// fragment from it — not a canvas clip, because canvas state is
    /// run-local in the fragment paint model: it never extends across child
    /// markers, and the entire point of `RenderClip` is clipping the child.
    ///
    /// By value: every caller holds a freshly-owned value from
    /// `resolve_clip`, so consuming `stored` is a move — for [`Path`], no
    /// second `Arc::clone`.
    fn to_paint_clip(stored: Self::Stored, behavior: Clip) -> PaintClip;
}

// ---- Rect ------------------------------------------------------------------

impl ClipGeometry for Rect<Pixels> {
    type Stored = Self;

    const DIAGNOSTIC_NAME: &'static str = "RenderClipRect";

    fn approximate_bounds(&self, _size: Size) -> Rect<Pixels> {
        *self
    }

    fn default_for_size(size: Size) -> Self {
        Rect::from_origin_size(Point::ZERO, size)
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        Rect::contains(self, position)
    }

    fn to_paint_clip(stored: Self::Stored, behavior: Clip) -> PaintClip {
        PaintClip::Rect {
            rect: stored,
            behavior,
        }
    }
}

// ---- RRect -----------------------------------------------------------------

impl ClipGeometry for RRect {
    type Stored = Self;

    const DIAGNOSTIC_NAME: &'static str = "RenderClipRRect";

    fn approximate_bounds(&self, _size: Size) -> Rect<Pixels> {
        // The corners round INWARD, so the bounding rect is a superset — the
        // allowed direction.
        self.bounding_rect()
    }

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

    fn resolve_rrect_border_radius(border_radius: BorderRadius, size: Size) -> Option<Self> {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        Some(RRect::from_rect_and_corners(
            bounds,
            border_radius.top_left,
            border_radius.top_right,
            border_radius.bottom_right,
            border_radius.bottom_left,
        ))
    }

    fn to_paint_clip(stored: Self::Stored, behavior: Clip) -> PaintClip {
        PaintClip::RRect {
            rrect: stored,
            behavior,
        }
    }
}

// ---- Oval ------------------------------------------------------------------

impl ClipGeometry for Oval {
    type Stored = Self;

    const DIAGNOSTIC_NAME: &'static str = "RenderClipOval";

    fn approximate_bounds(&self, _size: Size) -> Rect<Pixels> {
        // The inscribed ellipse is contained by its own bounding rect.
        self.bounds
    }

    fn default_for_size(size: Size) -> Self {
        Oval::from_size(size)
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        Oval::contains(self, position)
    }

    fn to_paint_clip(stored: Self::Stored, behavior: Clip) -> PaintClip {
        // Approximate the oval with an RRect whose corner radii equal half
        // the bounding-rect dimensions — a perfect inscribed ellipse.
        // (The engine may specialise this in a future backend; the
        // approximation is exact for the inscribed-ellipse case.)
        let rx = stored.bounds.width() * 0.5;
        let ry = stored.bounds.height() * 0.5;
        PaintClip::RRect {
            rrect: RRect::from_rect_elliptical(stored.bounds, rx, ry),
            behavior,
        }
    }
}

// ---- Path ------------------------------------------------------------------

impl ClipGeometry for Path {
    type Stored = Arc<Path>;

    const DIAGNOSTIC_NAME: &'static str = "RenderClipPath";

    fn approximate_bounds(&self, size: Size) -> Rect<Pixels> {
        // The whole box, not the path's own bounds. `Path::bounds` takes
        // `&mut self` because it memoizes, and this hook has `&self` — so the
        // honest options were a conservative superset or a lock. A superset is
        // what the contract asks for, and it is safe in the direction that
        // matters: a `ClipPath` narrows no accessibility rect it should have
        // narrowed, rather than dropping one it should have kept. Tightening
        // it means giving `Path` a `&self` bounds accessor first.
        Rect::from_origin_size(Point::ZERO, size)
    }

    fn default_for_size(size: Size) -> Self {
        // A path-shaped default is the rectangle outline of `size`.
        let mut p = Path::new();
        p.add_rect(Rect::from_origin_size(Point::ZERO, size));
        p
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        // Delegate to the fill-type-aware algorithm in flui_types::Path:
        // even-odd (ray-casting) or non-zero (winding number), selected
        // by the path's PathFillType. This matches Flutter's hit-test
        // semantics for RenderClipPath.
        self.contains(position)
    }

    fn resolve_path_clip_target(target: PathClipTarget, size: Size) -> Option<Self> {
        // The shared resolver owns the degrade (no lane, unregistered target
        // → the whole box), so paint, hit-test and semantics answer the same
        // path the paint walk builds for a `PaintClip::PathTarget`.
        Some(resolve_path_clip(target, size))
    }

    fn path_target_descriptor(
        target: PathClipTarget,
        size: Size,
        behavior: Clip,
    ) -> Option<PaintClip> {
        // The token, not a resolved path — see the trait doc for why.
        Some(PaintClip::PathTarget {
            target,
            size,
            behavior,
        })
    }

    fn to_paint_clip(stored: Self::Stored, behavior: Clip) -> PaintClip {
        PaintClip::Path {
            path: stored,
            behavior,
        }
    }
}

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
/// # Custom clip sources
///
/// By default the clip uses the entire box (`S::default_for_size(size)`).
/// `RenderClipRRect` can carry a data-only [`BorderRadius`], and
/// `RenderClipPath` can carry a data-only owner-lane [`PathClipTarget`].
pub struct RenderClip<S: ClipGeometry> {
    /// The clip behavior to use when applying the shape.
    clip_behavior: Clip,
    /// Optional rounded-rect border radius. Only meaningful for
    /// `RenderClipRRect`; other `RenderClip<S>` instantiations ignore it.
    rrect_border_radius: Option<BorderRadius>,
    /// Optional owner-local path clip target. Only meaningful for
    /// `RenderClipPath`; other `RenderClip<S>` instantiations ignore it.
    path_clip_target: Option<PathClipTarget>,
    /// Stable identity of the widget-owned path source currently registered.
    path_clip_source_token: Option<ClipSourceToken>,
    /// A caller-supplied clip shape that replaces `S::default_for_size`.
    ///
    /// Data, not a callback — see `set_clip_shape` for why that is the right
    /// shape here and what it does not cover.
    clip_shape: Option<S::Stored>,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
    /// Keeps the generic shape parameter part of the render object's type even
    /// when all runtime clip sources are data tokens.
    shape: PhantomData<S>,
}

impl<S: ClipGeometry> RenderClip<S> {
    /// Creates a clip render object with the given clip behavior and the
    /// default clipper (whole box).
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            clip_behavior,
            rrect_border_radius: None,
            path_clip_target: None,
            path_clip_source_token: None,
            clip_shape: None,
            has_child: false,
            shape: PhantomData,
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

    /// Replaces the clip behavior and reports a composited-layer update
    /// when changed.
    ///
    /// The clip is a layer property read from [`Self::paint_effects`]: a
    /// behavior change patches the retained clip layer in the enclosing
    /// repaint boundary's capture rather than repainting the subtree. There
    /// is no structural threshold to cross here the way opacity has one at
    /// alpha 0/255 — the clip layer exists at every `Clip` value, including
    /// `Clip::None` (see [`PaintClip`]'s docs) — so this setter never needs
    /// a second impact bit for that reason.
    ///
    /// Does not report [`RenderUpdateImpact::SEMANTICS`]: the accessibility
    /// clip gates on `Clip::None` at
    /// [`describe_approximate_paint_clip`](RenderBox::describe_approximate_paint_clip)'s
    /// read site rather than through this setter — a pre-existing gap, not
    /// fixed here.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Returns whether a custom clipper has been installed.
    #[inline]
    pub fn has_custom_clipper(&self) -> bool {
        self.rrect_border_radius.is_some() || self.path_clip_target.is_some()
    }

    /// Returns the caller-supplied clip shape, if one replaces the default
    /// whole box.
    #[must_use]
    pub fn clip_shape(&self) -> Option<&S> {
        self.clip_shape.as_ref().map(Borrow::borrow)
    }

    /// Replaces the default whole-box clip with a fixed shape.
    ///
    /// **Data, not a callback, and that is the whole design.** The reference's
    /// answer here is `CustomClipper<T>` — an abstract class with `getClip(size)`
    /// and a hand-written `shouldReclip(old)`. Its own test clipper
    /// (`ValueClipper` in `clip_test.dart`) holds a fixed `value`, ignores the
    /// `size` it is handed, and implements `shouldReclip` as
    /// `oldClipper.value != value`. In Rust that is a field and a `!=`.
    ///
    /// What that buys, concretely:
    ///
    /// * **No `shouldReclip` to get wrong.** The reference's version needs a
    ///   downcast that silently degrades to always- or never-reclip if written
    ///   badly. Here the comparison is the derived `PartialEq` the setter
    ///   already performs, so a wrong answer is not expressible.
    /// * **No identity token, so no repaint per rebuild.** A closure cannot be
    ///   compared, so a closure-based clipper needs an identity the caller has
    ///   to keep stable across rebuilds — the trap `ClipPath` carries today
    ///   (issue #856). A value has identity by construction.
    /// * **`Send + Sync`, so it lives in the render object** rather than in an
    ///   owner-lane side table resolved at paint time.
    ///
    /// **What it does not cover, stated rather than dropped:** a clip that is a
    /// FUNCTION of the box's size. The reference allows one and no case in its
    /// own suite uses it. If you need it, `ClipPath::new(|size| …)` takes a
    /// closure and a path can express any rect or oval, so the capability
    /// exists — it is these two convenience widgets that do not carry it.
    ///
    /// A changed shape reports a composited-layer update, plus semantics
    /// (the clipped accessibility rect moves with it): the shape is a layer
    /// property read from [`Self::paint_effects`], patched in place under a
    /// retained boundary rather than repainting the subtree.
    pub fn set_clip_shape(&mut self, shape: Option<S>) -> flui_rendering::RenderUpdateImpact {
        if self.clip_shape.as_ref().map(Borrow::borrow) == shape.as_ref() {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_shape = shape.map(Into::into);
        flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
            | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Builder form of [`set_clip_shape`](Self::set_clip_shape).
    #[must_use]
    pub fn with_clip_shape(mut self, shape: S) -> Self {
        self.clip_shape = Some(shape.into());
        self
    }

    /// Computes the clip shape for the given laid-out `size`, eagerly
    /// resolving an owner-lane path target (running the registered clipper)
    /// when one is installed.
    ///
    /// Called from `hit_test()` and `describe_approximate_paint_clip()`,
    /// both `&self`; the size is supplied by the driver (`ctx.own_size()` /
    /// the semantics walk). [`Self::clip_descriptor`] is the paint-side
    /// counterpart: same source precedence, but a path target stays an
    /// unresolved token there — see its doc for why.
    fn resolve_clip(&self, size: Size) -> S::Stored {
        if let Some(shape) = &self.clip_shape {
            return shape.clone();
        }
        self.rrect_border_radius
            .and_then(|border_radius| S::resolve_rrect_border_radius(border_radius, size))
            .or_else(|| {
                self.path_clip_target
                    .and_then(|target| S::resolve_path_clip_target(target, size))
            })
            .unwrap_or_else(|| S::default_for_size(size))
            .into()
    }

    /// Produces the node-local [`PaintClip`] descriptor for the laid-out
    /// `size`, following exactly [`Self::resolve_clip`]'s source precedence:
    /// a fixed `clip_shape`, then a data-only `rrect_border_radius`, then an
    /// owner-lane `path_clip_target`, then the shape's whole-box default.
    ///
    /// Unlike `resolve_clip`, an owner-lane path target here does NOT run
    /// the registered clipper: `S::path_target_descriptor` hands back the
    /// token as data, and the paint walk resolves it through
    /// [`resolve_path_clip`] inside the paint frame. That purity is what
    /// makes it safe to call from [`RenderBox::paint_effects`] below on a
    /// coordinate query (`transform_to`'s default `apply_paint_transform`)
    /// outside any paint walk — building this value never runs a caller's
    /// clipper. Paint, hit-test and semantics still resolve the same target
    /// eagerly through `resolve_clip`, so all three agree on the shape.
    fn clip_descriptor(&self, size: Size) -> PaintClip {
        if let Some(shape) = &self.clip_shape {
            return S::to_paint_clip(shape.clone(), self.clip_behavior);
        }
        if let Some(shape) = self
            .rrect_border_radius
            .and_then(|border_radius| S::resolve_rrect_border_radius(border_radius, size))
        {
            return S::to_paint_clip(shape.into(), self.clip_behavior);
        }
        if let Some(descriptor) = self
            .path_clip_target
            .and_then(|target| S::path_target_descriptor(target, size, self.clip_behavior))
        {
            return descriptor;
        }
        S::to_paint_clip(S::default_for_size(size).into(), self.clip_behavior)
    }
}

impl RenderClip<RRect> {
    /// Returns the data-only border radius used to compute the rounded-rect
    /// clip, if one is installed.
    #[must_use]
    pub const fn border_radius(&self) -> Option<BorderRadius> {
        self.rrect_border_radius
    }

    /// Builder: sets the data-only rounded-rect border radius.
    #[must_use]
    pub fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.rrect_border_radius = Some(border_radius);
        self
    }

    /// Replaces the rounded-rect source and reports a composited-layer
    /// update plus semantics when changed.
    ///
    /// The radius is a layer property read from [`Self::paint_effects`]: a
    /// changed radius patches the retained `ClipRRectLayer` in the
    /// enclosing repaint boundary's capture rather than repainting the
    /// subtree.
    pub fn set_border_radius(
        &mut self,
        border_radius: Option<BorderRadius>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.rrect_border_radius == border_radius {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.rrect_border_radius = border_radius;
        flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
            | flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl RenderClip<Path> {
    /// Builder form of [`Self::set_path_clip_source_token`].
    #[must_use]
    pub fn with_path_clip_source_token(mut self, source_token: ClipSourceToken) -> Self {
        self.path_clip_source_token = Some(source_token);
        self
    }

    /// Replaces the stable path-factory identity.
    ///
    /// A different source changes both the painted clip and clipped
    /// semantics geometry, reported as a composited-layer update plus
    /// semantics: the clip shape is a layer property read from
    /// [`Self::paint_effects`], patched in place under a retained boundary
    /// rather than repainting the subtree. An identical token is a no-op.
    pub fn set_path_clip_source_token(
        &mut self,
        source_token: &ClipSourceToken,
    ) -> RenderUpdateImpact {
        if self.path_clip_source_token.as_ref() == Some(source_token) {
            return RenderUpdateImpact::NONE;
        }
        self.path_clip_source_token = Some(source_token.clone());
        RenderUpdateImpact::COMPOSITED_LAYER_UPDATE | RenderUpdateImpact::SEMANTICS
    }

    /// Returns the owner-local path clip target, if one is installed.
    #[must_use]
    pub const fn path_clip_target(&self) -> Option<PathClipTarget> {
        self.path_clip_target
    }

    /// Replaces the owner-local path clip target.
    ///
    /// A changed target reports a composited-layer update plus semantics:
    /// the resolved clip is a layer property read from
    /// [`Self::paint_effects`], patched in place under a retained boundary
    /// rather than repainting the subtree.
    pub fn set_path_clip_target(&mut self, target: Option<PathClipTarget>) -> RenderUpdateImpact {
        if self.path_clip_target == target {
            return RenderUpdateImpact::NONE;
        }
        self.path_clip_target = target;
        RenderUpdateImpact::COMPOSITED_LAYER_UPDATE | RenderUpdateImpact::SEMANTICS
    }
}

// `Clone` cannot be derived because `dyn Fn` is not Clone, but Arc is.
impl<S: ClipGeometry> Clone for RenderClip<S> {
    fn clone(&self) -> Self {
        Self {
            clip_behavior: self.clip_behavior,
            rrect_border_radius: self.rrect_border_radius,
            path_clip_target: self.path_clip_target,
            path_clip_source_token: self.path_clip_source_token.clone(),
            clip_shape: self.clip_shape.clone(),
            has_child: self.has_child,
            shape: PhantomData,
        }
    }
}

impl<S: ClipGeometry> fmt::Debug for RenderClip<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderClip")
            .field("clip_behavior", &self.clip_behavior)
            .field(
                "has_custom_clipper",
                &(self.rrect_border_radius.is_some() || self.path_clip_target.is_some()),
            )
            .field(
                "has_rrect_border_radius",
                &self.rrect_border_radius.is_some(),
            )
            .field("has_path_clip_target", &self.path_clip_target.is_some())
            .field("path_clip_source_token", &self.path_clip_source_token)
            .field("has_clip_shape", &self.clip_shape.is_some())
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
    fn to_diagnostics_node(&self) -> flui_foundation::DiagnosticsNode {
        let mut node = flui_foundation::DiagnosticsNode::new(S::DIAGNOSTIC_NAME);
        let mut builder = flui_foundation::DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("clip_behavior", self.clip_behavior);
        builder.add_flag(
            "custom_clipper",
            self.rrect_border_radius.is_some()
                || self.path_clip_target.is_some()
                || self.clip_shape.is_some(),
            "has custom clipper",
        );
    }
}

impl<S: ClipGeometry> RenderBox for RenderClip<S> {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    /// Splices the child's fragment directly — no scope push here.
    ///
    /// The clip is reported through [`Self::paint_effects`] instead: the
    /// pipeline wraps the node's entire fragment (this splice) in the
    /// `PaintClip` layer `paint_effects` describes, BEFORE replaying it —
    /// the same split `RenderTransform::paint` makes between `paint` and
    /// `paint_effects` for its own layer (`crates/flui-objects/src/layout/transform.rs`).
    /// Pushing the clip again here would wrap the child in it twice.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    /// Reports this node's clip as a composited-layer-patchable effect.
    ///
    /// Unconditional, including at [`Clip::None`]: a [`PaintClip`] with
    /// `behavior: Clip::None` is still a layer scope the engine skips
    /// pushing, not the absence of one (see [`PaintClip`]'s own docs) — so
    /// `set_clip_behavior` crossing `Clip::None` is a property update on an
    /// existing layer, never a structural add/remove of one.
    fn paint_effects(&self, size: Size) -> PaintEffects {
        PaintEffects::NONE.with_clip(self.clip_descriptor(size))
    }

    /// Content this clip paints over carries no accessibility presence.
    ///
    /// Oracle: `_RenderCustomClip.describeApproximatePaintClip`
    /// (`rendering/proxy_box.dart`) returns the clip when the behaviour is not
    /// `none`. Without this the semantics walk saw the trait's `None` default,
    /// so a `ClipRect` published full-size rects for children it visibly cut
    /// in half — the clip was honoured by paint and by hit-test, and by
    /// nothing a screen reader could see.
    ///
    /// No `describe_semantics_clip` counterpart: a clip keeps no cache area
    /// the way a viewport does, so it has nothing wider than its paint clip to
    /// grant. Content it clips away is gone, not merely off-screen.
    fn describe_approximate_paint_clip(
        &self,
        _child_slot: usize,
        size: Size,
    ) -> Option<Rect<Pixels>> {
        if self.clip_behavior == Clip::None {
            return None;
        }
        let stored = self.resolve_clip(size);
        Some(S::approximate_bounds(
            <S::Stored as Borrow<S>>::borrow(&stored),
            size,
        ))
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // Honour the clip: a hit outside the clip shape doesn't reach
        // the child. Flutter parity.
        let position = Point::new(ctx.x(), ctx.y());
        let stored = self.resolve_clip(ctx.own_size());
        if !S::contains(<S::Stored as Borrow<S>>::borrow(&stored), position) {
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
    use flui_types::styling::BorderRadiusExt;

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

    // 1.4 guard tests (characterization — NOT red→green; these pass today).
    // Label: these confirm existing correct Oval behavior and lock it against
    // future regressions.
    #[test]
    fn oval_clip_geometry_center_is_inside() {
        let oval = <Oval as ClipGeometry>::default_for_size(Size::new(px(100.0), px(60.0)));
        assert!(
            <Oval as ClipGeometry>::contains(&oval, Point::new(px(50.0), px(30.0))),
            "center of oval must be inside (guard: existing correct behavior)"
        );
    }

    #[test]
    fn oval_clip_geometry_bbox_corner_is_outside_ellipse() {
        let oval = <Oval as ClipGeometry>::default_for_size(Size::new(px(100.0), px(60.0)));
        assert!(
            !<Oval as ClipGeometry>::contains(&oval, Point::new(px(1.0), px(1.0))),
            "bbox corner (near 0,0) must be outside the inscribed ellipse \
             (guard: existing correct behavior)"
        );
    }

    #[test]
    fn oval_clip_geometry_outside_bbox_is_outside() {
        let oval = <Oval as ClipGeometry>::default_for_size(Size::new(px(100.0), px(60.0)));
        assert!(
            !<Oval as ClipGeometry>::contains(&oval, Point::new(px(200.0), px(200.0))),
            "point outside bounding box must not be inside oval \
             (guard: existing correct behavior)"
        );
    }

    #[test]
    fn oval_clip_geometry_degenerate_contains_nothing() {
        let oval = <Oval as ClipGeometry>::default_for_size(Size::ZERO);
        assert!(
            !<Oval as ClipGeometry>::contains(&oval, Point::ZERO),
            "degenerate (zero-size) oval must contain nothing \
             (guard: existing correct behavior)"
        );
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

    // 1.4 RED test (behavior fix): Path::contains must delegate to the
    // fill-type-aware algorithm in flui_types::Path::contains, not return
    // a conservative true for all points.
    #[test]
    fn path_contains_delegates_to_fill_type_algorithm() {
        // Build a triangle: (0,0) → (100,0) → (50,100) → close.
        let mut triangle = Path::new();
        triangle.move_to(Point::new(px(0.0), px(0.0)));
        triangle.line_to(Point::new(px(100.0), px(0.0)));
        triangle.line_to(Point::new(px(50.0), px(100.0)));
        triangle.close();

        // Centroid of the triangle — must be inside.
        let inside = Point::new(px(50.0), px(33.0));
        // Clearly outside (to the right and below).
        let outside = Point::new(px(200.0), px(200.0));

        assert!(
            <Path as ClipGeometry>::contains(&triangle, inside),
            "centroid of triangle must be inside the path"
        );
        assert!(
            !<Path as ClipGeometry>::contains(&triangle, outside),
            "point far outside bounding box must not be inside the path \
             (before fix: Path::contains always returns true)"
        );
    }

    #[test]
    fn render_clip_path_resolves_owner_local_path_target() {
        use std::cell::Cell;
        use std::rc::Rc;

        use flui_interaction::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let calls = Rc::new(Cell::new(0));
        lane.enter(|| {
            let calls_for_clipper = Rc::clone(&calls);
            let target = handle
                .register_path_clipper(move |size| {
                    calls_for_clipper.set(calls_for_clipper.get() + 1);
                    let mut path = Path::new();
                    path.add_rect(Rect::from_origin_size(Point::ZERO, size));
                    path
                })
                .expect("register path clipper");

            let mut node = RenderClipPath::anti_alias();
            assert_eq!(
                node.set_path_clip_target(Some(target)),
                RenderUpdateImpact::COMPOSITED_LAYER_UPDATE | RenderUpdateImpact::SEMANTICS
            );
            assert_eq!(
                node.set_path_clip_target(Some(target)),
                RenderUpdateImpact::NONE
            );
            assert!(node.has_custom_clipper());

            let path = node.resolve_clip(Size::new(px(20.0), px(30.0)));

            assert!(path.contains(Point::new(px(10.0), px(10.0))));
        });

        assert_eq!(calls.get(), 1);
    }

    // ---------- paint_effects / clip_descriptor ---------------------------

    #[test]
    fn clip_descriptor_rect_default_matches_default_for_size() {
        let size = Size::new(px(80.0), px(40.0));
        let node = RenderClipRect::hard_edge();

        match node.clip_descriptor(size) {
            PaintClip::Rect { rect, behavior } => {
                assert_eq!(rect, <Rect<Pixels> as ClipGeometry>::default_for_size(size));
                assert_eq!(behavior, Clip::HardEdge);
            }
            other => panic!("expected PaintClip::Rect, got {other:?}"),
        }
    }

    #[test]
    fn clip_descriptor_oval_is_an_elliptical_rrect() {
        let size = Size::new(px(100.0), px(60.0));
        let node = RenderClipOval::anti_alias();
        let default_oval = <Oval as ClipGeometry>::default_for_size(size);

        match node.clip_descriptor(size) {
            PaintClip::RRect { rrect, behavior } => {
                assert_eq!(behavior, Clip::AntiAlias);
                assert_eq!(rrect.top_left.x, default_oval.bounds.width() * 0.5);
                assert_eq!(rrect.top_left.y, default_oval.bounds.height() * 0.5);
            }
            other => panic!("expected PaintClip::RRect, got {other:?}"),
        }
    }

    // Pins the descriptor's core safety property (spec AC11): building it
    // for a `PathTarget` carries the token as data and runs the registered
    // clipper zero times, unlike `resolve_clip` above which runs it once.
    #[test]
    fn clip_descriptor_path_target_carries_the_token_and_runs_no_clipper() {
        use std::cell::Cell;
        use std::rc::Rc;

        use flui_interaction::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let calls = Rc::new(Cell::new(0));
        let size = Size::new(px(20.0), px(30.0));
        lane.enter(|| {
            let calls_for_clipper = Rc::clone(&calls);
            let target = handle
                .register_path_clipper(move |size| {
                    calls_for_clipper.set(calls_for_clipper.get() + 1);
                    let mut path = Path::new();
                    path.add_rect(Rect::from_origin_size(Point::ZERO, size));
                    path
                })
                .expect("register path clipper");

            let mut node = RenderClipPath::anti_alias();
            let _ = node.set_path_clip_target(Some(target));

            match node.clip_descriptor(size) {
                PaintClip::PathTarget {
                    size: descriptor_size,
                    ..
                } => assert_eq!(descriptor_size, size),
                other => panic!("expected PaintClip::PathTarget, got {other:?}"),
            }
        });

        assert_eq!(calls.get(), 0, "the descriptor must not run the clipper");
    }

    // Pins AC11's other half: a statically shaped `clip_shape` is shared by
    // refcount into the descriptor, never copied.
    #[test]
    fn clip_descriptor_fixed_path_shares_the_arc_without_copying() {
        let mut path = Path::new();
        path.add_rect(Rect::from_origin_size(
            Point::ZERO,
            Size::new(px(10.0), px(10.0)),
        ));
        let node = RenderClipPath::anti_alias().with_clip_shape(path);
        let stored = node.clip_shape().expect("clip_shape set above");

        match node.clip_descriptor(Size::new(px(20.0), px(20.0))) {
            PaintClip::Path {
                path: descriptor_path,
                ..
            } => assert!(
                std::ptr::eq(&raw const *descriptor_path, stored),
                "descriptor must share the stored Arc<Path>, not copy it"
            ),
            other => panic!("expected PaintClip::Path, got {other:?}"),
        }
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
    fn set_clip_behavior_returns_exact_impact() {
        let mut node = RenderClipRect::anti_alias();
        assert_eq!(
            node.set_clip_behavior(Clip::HardEdge),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        );
        assert_eq!(
            node.set_clip_behavior(Clip::HardEdge),
            flui_rendering::RenderUpdateImpact::NONE
        );
    }

    #[test]
    fn set_clip_shape_returns_exact_impact() {
        let mut node = RenderClipRect::anti_alias();
        let shape = Rect::from_origin_size(Point::ZERO, Size::new(px(10.0), px(10.0)));
        assert_eq!(
            node.set_clip_shape(Some(shape)),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
        assert_eq!(
            node.set_clip_shape(Some(shape)),
            flui_rendering::RenderUpdateImpact::NONE,
        );
    }

    #[test]
    fn path_clip_source_tokens_clone_identity_and_fresh_values_are_distinct() {
        let source_token = ClipSourceToken::fresh();
        let cloned_source_token = source_token.clone();

        assert_eq!(source_token, cloned_source_token);
        assert_ne!(source_token, ClipSourceToken::fresh());
    }

    #[test]
    fn render_clip_path_reports_only_source_token_changes() {
        let source_token = ClipSourceToken::fresh();
        let mut node =
            RenderClipPath::anti_alias().with_path_clip_source_token(source_token.clone());

        assert_eq!(
            node.set_path_clip_source_token(&source_token),
            RenderUpdateImpact::NONE
        );
        assert_eq!(
            node.set_path_clip_source_token(&ClipSourceToken::fresh()),
            RenderUpdateImpact::COMPOSITED_LAYER_UPDATE | RenderUpdateImpact::SEMANTICS
        );
    }

    #[test]
    fn rrect_border_radius_installs_data_only_clip_source() {
        let radius = BorderRadius::circular(px(12.0));
        let node: RenderClipRRect = RenderClip::anti_alias().with_border_radius(radius);
        assert!(node.has_custom_clipper());
        assert_eq!(node.border_radius(), Some(radius));

        let resolved = node.resolve_clip(Size::new(px(100.0), px(50.0)));
        assert_eq!(resolved.top_left.x, px(12.0));
        assert_eq!(resolved.top_right.x, px(12.0));
    }

    #[test]
    fn rrect_border_radius_returns_exact_impact_for_change_and_identity() {
        let radius = BorderRadius::circular(px(12.0));
        let mut node: RenderClipRRect = RenderClip::anti_alias();
        assert_eq!(
            node.set_border_radius(Some(radius)),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
        assert_eq!(
            node.set_border_radius(Some(radius)),
            flui_rendering::RenderUpdateImpact::NONE,
        );
    }

    #[test]
    fn type_aliases_compile() {
        let _r: RenderClipRect = RenderClip::anti_alias();
        let _rr: RenderClipRRect = RenderClip::anti_alias();
        let _o: RenderClipOval = RenderClip::anti_alias();
        let _p: RenderClipPath = RenderClip::anti_alias();
    }

    #[test]
    fn clone_is_supported_even_with_data_clip_source() {
        let node: RenderClipRRect =
            RenderClip::anti_alias().with_border_radius(BorderRadius::circular(px(8.0)));
        let cloned = node.clone();
        assert!(cloned.has_custom_clipper());
        assert_eq!(cloned.clip_behavior(), node.clip_behavior());
        assert_eq!(cloned.border_radius(), node.border_radius());
    }

    #[test]
    fn debug_format_summarizes_clip_sources() {
        let node: RenderClipRect = RenderClip::anti_alias();
        let dbg = format!("{node:?}");
        assert!(dbg.contains("RenderClip"));
        assert!(dbg.contains("clip_behavior"));
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
        assert!(!names.iter().any(|n| n == "custom_clipper"));
    }

    #[test]
    fn to_diagnostics_node_uses_flutter_parity_alias_names() {
        use flui_foundation::Diagnosticable;

        assert_eq!(
            RenderClipRect::anti_alias().to_diagnostics_node().name(),
            Some("RenderClipRect")
        );
        assert_eq!(
            RenderClipRRect::anti_alias().to_diagnostics_node().name(),
            Some("RenderClipRRect")
        );
        assert_eq!(
            RenderClipOval::anti_alias().to_diagnostics_node().name(),
            Some("RenderClipOval")
        );
        assert_eq!(
            RenderClipPath::anti_alias().to_diagnostics_node().name(),
            Some("RenderClipPath")
        );
    }
}
