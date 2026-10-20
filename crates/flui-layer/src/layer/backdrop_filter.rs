//! `BackdropFilterLayer` — filters what is already painted behind it: frosted glass.

use std::{fmt, hash, sync::Arc};

use flui_foundation::geometry::Rect;
use flui_foundation::inspect::Shared;
use flui_painting::paint::{BlendMode, ImageFilter};

#[derive(Debug)]
struct BackdropGroupMarker;

/// Identity of backdrop filters that sample one shared backdrop.
///
/// Filters carrying the same key within one compositing surface read the
/// backdrop as it was when the first of them was reached, so overlapping
/// panes do not filter each other's output, and the backdrop is captured
/// once. The key is an identity, not a label: clones compare equal, two
/// separately created keys never do.
///
/// ```
/// use flui_layer::BackdropGroupKey;
///
/// let key = BackdropGroupKey::new();
/// assert_eq!(key, key.clone());
/// assert_ne!(key, BackdropGroupKey::new());
/// ```
#[derive(Clone)]
pub struct BackdropGroupKey(Arc<BackdropGroupMarker>);

impl BackdropGroupKey {
    /// Creates a new group identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(BackdropGroupMarker))
    }

    /// The group as a description names it: the same for clones of this key.
    pub(crate) fn shared(&self) -> Shared<'_> {
        Shared::arc("BackdropGroup", &self.0)
    }
}

impl Default for BackdropGroupKey {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for BackdropGroupKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BackdropGroupKey(..)")
    }
}

impl PartialEq for BackdropGroupKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for BackdropGroupKey {}

impl hash::Hash for BackdropGroupKey {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Layer that applies an image filter to backdrop content
///
/// # Architecture
///
/// ```text
/// Backdrop Content → Capture Texture → Apply Filter → Render Child → Composite
/// ```
///
/// # Rendering Process
///
/// 1. Capture current framebuffer content in specified bounds
/// 2. Apply image filter (blur, color adjustments, etc.) via GPU
/// 3. Render filtered backdrop to framebuffer
/// 4. Render child content on top (if present)
/// 5. Composite with blend mode
///
/// # Example
///
/// ```rust
/// use flui_layer::BackdropFilterLayer;
/// use flui_foundation::geometry::Rect;
/// use flui_painting::paint::{BlendMode, ImageFilter};
///
/// // Create frosted glass effect
/// let frosted_glass = BackdropFilterLayer::new(
///     ImageFilter::blur(10.0), // 10px gaussian blur
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BackdropFilterLayer {
    /// Image filter to apply to backdrop
    filter: ImageFilter,

    /// Blend mode for compositing
    blend_mode: BlendMode,

    /// Bounds for backdrop capture (pre-computed for performance)
    bounds: Rect<f64>,

    /// The shared backdrop this filter samples, if it belongs to a group.
    group: Option<BackdropGroupKey>,
}

impl BackdropFilterLayer {
    /// Filters what is already painted behind `bounds` with `filter`, compositing with `blend_mode`.
    pub fn new(filter: ImageFilter, blend_mode: BlendMode, bounds: Rect<f64>) -> Self {
        Self {
            filter,
            blend_mode,
            bounds,
            group: None,
        }
    }

    /// Samples the backdrop shared by `group` instead of the live one.
    #[must_use]
    pub fn with_group(mut self, group: Option<BackdropGroupKey>) -> Self {
        self.group = group;
        self
    }

    /// The shared backdrop group this filter samples, if any.
    pub fn group(&self) -> Option<&BackdropGroupKey> {
        self.group.as_ref()
    }

    /// The filter applied to the backdrop.
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// How the filtered backdrop composites.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// The rectangle of backdrop captured and filtered.
    pub fn bounds(&self) -> Rect<f64> {
        self.bounds
    }
}
