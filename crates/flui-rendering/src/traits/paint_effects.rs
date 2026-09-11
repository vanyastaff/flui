//! A node's own paint effects as one value.
//!
//! [`PaintEffects`] replaces a family of separate hooks (alpha, blend,
//! transform, and the clip scopes a node opened for itself inside `paint`)
//! with one declarative value the walk reads once per node. The shape
//! mirrors Jetpack Compose's `Modifier.graphicsLayer` — a single layer
//! value (`alpha`, `clip`, `scale`/`rotation`/`translation`, ...) whose
//! properties update without re-recording the content that draws through
//! it; FLUI keeps its own retained layer tree underneath, but the
//! producer-facing contract is the same: describe the layer, don't
//! imperatively push and pop it.

use std::sync::Arc;

use flui_types::{
    Matrix4, Pixels, Point, RRect, Rect, Size,
    painting::{Clip, Path},
};

use crate::hit_testing::PathClipTarget;

/// A node's own paint effects, in the order they wrap the node's content.
///
/// # Nesting is fixed on the type, outermost first: `opacity`, `clip`,
/// then `transform`
///
/// Transform is innermost by necessity: [`Self::opacity`] and
/// [`Self::clip`] are both stated in the node's own coordinate space, and
/// the transform maps a child's space into that space — anything placed
/// inside the transform would be read in the child's (possibly scaled)
/// coordinates instead. Opacity is outermost because it is a
/// coordinate-free group alpha over everything the node contributes: it
/// commutes with both a clip and a transform, so putting it outside
/// matches every producer that combines opacity with either.
///
/// A node that needs opacity *inside* a future filter or mask (once those
/// fields exist) expresses that by introducing a child node for the
/// filtered content, exactly as it would with any other layer ordering —
/// never by reordering the fields on this type. A node that genuinely
/// needs a different nesting keeps that scope in `paint` instead of
/// reaching for this value, and forgoes the update-only path for it.
///
/// # `Clip::None` is still a layer
///
/// A [`PaintClip`] whose `behavior` is [`Clip::None`] is not the absence
/// of a clip — the field is still `Some`, and the shape still describes a
/// layer scope; the engine simply skips pushing the clip primitive when it
/// builds that layer. A node whose clip is gated by something other than
/// an explicit clip-behavior property (for example, an overflow check
/// derived from layout) returns `None` for [`Self::clip`] instead: the
/// two conventions exist because one is a property value and the other is
/// a layout-derived decision about whether the scope exists at all.
///
/// # Construction
///
/// This type is `#[non_exhaustive]` with public fields: build one from
/// [`Self::NONE`] and the by-value `with_*` builders below. Cross-crate
/// callers cannot use struct-literal syntax or `..Default::default()`
/// (hence no `Default` impl — one would invite exactly that, which
/// `#[non_exhaustive]` rejects), and adding a field later stays additive.
///
/// # Purity and read sites
///
/// A producer's `paint_effects` must be pure in `(self, size)`: it must
/// not run user code and must not depend on `paint` having already run.
/// The value is read from three places — the paint walk (building a real
/// layer), the composited-layer-update patch arm (rebuilding just the
/// layer's properties without re-recording the node's content), and the
/// default `apply_paint_transform` (`.transform` only, used by coordinate
/// queries such as `transform_to` outside any paint walk). Purity is what
/// makes reading the value safe from all three call sites.
///
/// # Size
///
/// Small enough to return by value (≈128 bytes): built once per node per
/// read and consumed by move, never stored.
///
/// # Examples
///
/// ```
/// use flui_rendering::traits::{PaintClip, PaintEffects, PaintOpacity};
/// use flui_types::{Rect, painting::Clip};
///
/// let effects = PaintEffects::NONE
///     .with_opacity(PaintOpacity::new(128))
///     .with_clip(PaintClip::Rect { rect: Rect::ZERO, behavior: Clip::HardEdge });
/// assert_eq!(effects.opacity.map(|o| o.alpha), Some(128));
/// assert!(matches!(effects.clip, Some(PaintClip::Rect { .. })));
/// assert!(effects.transform.is_none());
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaintEffects {
    /// Coordinate-free group opacity applied over everything this node
    /// paints, including its clip and transform. `None` means the node
    /// contributes no opacity layer.
    pub opacity: Option<PaintOpacity>,

    /// The clip scope this node opens around its content, in the node's
    /// own (pre-transform) coordinate space. `None` means the node opens
    /// no clip layer.
    pub clip: Option<PaintClip>,

    /// The transform this node applies to its content, mapping the
    /// node's children into the node's own space. `None` means the node
    /// applies no transform layer.
    pub transform: Option<Matrix4>,
}

impl PaintEffects {
    /// No effects: the node contributes no opacity, clip, or transform
    /// layer of its own.
    pub const NONE: Self = Self {
        opacity: None,
        clip: None,
        transform: None,
    };

    /// Returns `self` with [`Self::opacity`] set.
    ///
    /// Not `const fn`: kept symmetric with [`Self::with_clip`] and
    /// [`Self::with_transform`] rather than special-cased, even though
    /// this particular builder has no drop glue of its own.
    #[must_use]
    pub fn with_opacity(mut self, opacity: PaintOpacity) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Returns `self` with [`Self::clip`] set.
    ///
    /// Not `const fn`: [`PaintClip::Path`] carries an `Arc<Path>`, which
    /// needs drop glue a `const fn` builder cannot have.
    #[must_use]
    pub fn with_clip(mut self, clip: PaintClip) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Returns `self` with [`Self::transform`] set.
    ///
    /// Not `const fn`: kept symmetric with [`Self::with_clip`] rather
    /// than special-cased.
    #[must_use]
    pub fn with_transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }
}

/// A node's own group opacity, expressed as a paint effect.
///
/// `Copy + PartialEq + Eq`, unlike [`PaintClip`]: nothing here can carry a
/// memoised or shared payload, so an oracle may compare the whole value
/// (`assert_eq!(fx.opacity, Some(PaintOpacity::new(128)))`), and a blend
/// mode added later keeps all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PaintOpacity {
    /// Alpha in `0..=255`, applied to everything the node paints.
    ///
    /// `255` (fully opaque) is representable and expected: a node forced
    /// into `always_needs_compositing` (for example, to keep a stable
    /// layer for repeated composited updates) still describes a full-alpha
    /// group layer here rather than omitting [`PaintEffects::opacity`].
    pub alpha: u8,
}

impl PaintOpacity {
    /// Creates a paint-effect opacity from a raw alpha value.
    ///
    /// A `blend` field is added here once a producer needs one — it is
    /// deliberately not present as a phantom placeholder today.
    #[must_use]
    pub const fn new(alpha: u8) -> Self {
        Self { alpha }
    }
}

/// A node's own clip scope, expressed as a paint effect.
///
/// No `PartialEq`: [`Path`]'s derived equality includes its memoised
/// bounds cache, so two clips built from an equal path compare unequal
/// once either has had its bounds computed — a trap for any production
/// code that compared descriptors instead of the layers they produce.
/// Nothing in production needs to compare two `PaintClip` values; test
/// oracles compare the layers this value produces (kind, plus rect/rrect
/// fields, or an explicit accessor that ignores memoised bounds for a
/// path).
///
/// # Construction
///
/// Unlike [`PaintEffects`], each existing variant stays directly
/// constructible with ordinary struct-literal syntax from any crate —
/// `#[non_exhaustive]` on an enum only forces a wildcard arm on a
/// downstream `match`, so a producer writes `PaintClip::Rect { rect,
/// behavior }` and needs no builder.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PaintClip {
    /// Axis-aligned rectangular clip.
    Rect {
        /// The clip rectangle, in the node's own coordinate space.
        rect: Rect<Pixels>,
        /// How to handle content outside the clip boundary.
        behavior: Clip,
    },
    /// Rounded-rectangle clip.
    RRect {
        /// The rounded clip rectangle, in the node's own coordinate
        /// space.
        rrect: RRect,
        /// How to handle content outside the clip boundary.
        behavior: Clip,
    },
    /// Arbitrary path clip with a statically known shape.
    Path {
        /// Shared, not copied: the producer boxes the path once and every
        /// reader — including a coordinate query building this value just
        /// to read [`PaintEffects::transform`] — bumps a refcount instead
        /// of cloning a command buffer.
        path: Arc<Path>,
        /// How to handle content outside the clip boundary.
        behavior: Clip,
    },

    /// A path clip whose shape is a function of the node's size, resolved
    /// through the active owner lane by the paint walk — never by the
    /// producer. The descriptor is data: a 16-byte token plus the size the
    /// clipper is evaluated at, so building this value on a coordinate
    /// query (`transform_to` through a `ClipPath` with a custom clipper)
    /// runs no user code and allocates nothing; the walk resolves it once,
    /// inside the paint frame, with [`resolve_path_clip`].
    ///
    /// [`Clip::None`] still yields a layer for this variant, exactly as it
    /// does for [`Self::Rect`], [`Self::RRect`], and [`Self::Path`].
    PathTarget {
        /// The owner-lane token registered for the clipper.
        target: PathClipTarget,
        /// The size the clipper is evaluated at — the `size` the
        /// producer's `paint_effects(size)` was asked for. Carried here
        /// because the walk builds layers from the descriptor alone
        /// (own-effect arm, patch arm, and fragment-scope replay) and
        /// must not need the node back to evaluate it.
        size: Size,
        /// How to handle content outside the clip boundary.
        behavior: Clip,
    },
}

/// Resolves a [`PaintClip::PathTarget`] through the active owner lane.
///
/// On any resolution failure — no lane active (`InactiveRealm`), an
/// unregistered target — the clip degrades to the whole box: a rectangle
/// path of `size` at the origin. That is the same degrade
/// `RenderClip<Path>` performs for a token it cannot resolve, kept in one
/// place so paint, hit-test and semantics agree.
pub fn resolve_path_clip(target: PathClipTarget, size: Size) -> Path {
    match crate::hit_testing::resolve_path_clip_target(target, size) {
        Ok(path) => path,
        Err(error) => {
            tracing::debug!(?error, "path clip target resolution failed");
            let mut path = Path::new();
            path.add_rect(Rect::from_origin_size(Point::ZERO, size));
            path
        }
    }
}
