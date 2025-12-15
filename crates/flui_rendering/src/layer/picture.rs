//! Picture layer for recorded drawing commands.

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flui_types::{Offset, Point, Rect};

use super::base::{EngineLayer, Layer, LayerId, SceneBuilder};

// ============================================================================
// Picture
// ============================================================================

/// Unique identifier for a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PictureId(u64);

impl PictureId {
    /// Creates a new unique picture ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value.
    pub fn get(&self) -> u64 {
        self.0
    }
}

impl Default for PictureId {
    fn default() -> Self {
        Self::new()
    }
}

/// A recorded sequence of drawing commands.
///
/// Pictures are immutable once created and can be rendered multiple times
/// efficiently. They are created by recording drawing commands to a
/// `PictureRecorder`.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `Picture` class from `dart:ui`.
#[derive(Debug, Clone)]
pub struct Picture {
    /// Unique identifier.
    id: PictureId,

    /// Bounds of the recorded content.
    bounds: Rect,

    /// Whether to prefer raster caching.
    will_change_hint: bool,

    /// Approximate byte size for caching decisions.
    approximate_bytes_used: usize,
}

impl Picture {
    /// Creates a new picture.
    pub fn new(bounds: Rect) -> Self {
        Self {
            id: PictureId::new(),
            bounds,
            will_change_hint: false,
            approximate_bytes_used: 0,
        }
    }

    /// Returns the picture ID.
    pub fn id(&self) -> PictureId {
        self.id
    }

    /// Returns the bounds of the picture.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Returns whether this picture will change and shouldn't be cached.
    pub fn will_change(&self) -> bool {
        self.will_change_hint
    }

    /// Sets whether this picture will change.
    pub fn set_will_change(&mut self, will_change: bool) {
        self.will_change_hint = will_change;
    }

    /// Returns the approximate memory used by this picture.
    pub fn approximate_bytes_used(&self) -> usize {
        self.approximate_bytes_used
    }

    /// Sets the approximate bytes used.
    pub fn set_approximate_bytes_used(&mut self, bytes: usize) {
        self.approximate_bytes_used = bytes;
    }

    /// Disposes the picture, releasing resources.
    pub fn dispose(&self) {
        // In a real implementation, this would release GPU resources
    }
}

// ============================================================================
// PictureLayer
// ============================================================================

/// A layer that contains a recorded picture.
///
/// This is a leaf layer that doesn't have children. It contains a `Picture`
/// which is a recorded sequence of drawing commands.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PictureLayer` class.
#[derive(Debug)]
pub struct PictureLayer {
    /// Unique identifier.
    id: LayerId,

    /// Engine layer handle.
    engine_layer: Option<EngineLayer>,

    /// The picture to draw.
    picture: Option<Arc<Picture>>,

    /// Offset for the picture.
    offset: Offset,

    /// Whether layer tree compositing is complex.
    is_complex_hint: bool,

    /// Whether the picture will change on the next frame.
    will_change_hint: bool,

    /// Whether this layer needs to be added to the scene.
    needs_add_to_scene: bool,
}

impl Default for PictureLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl PictureLayer {
    /// Creates a new picture layer.
    pub fn new() -> Self {
        Self {
            id: LayerId::new(),
            engine_layer: None,
            picture: None,
            offset: Offset::ZERO,
            is_complex_hint: false,
            will_change_hint: false,
            needs_add_to_scene: true,
        }
    }

    /// Creates a picture layer with the given picture and offset.
    pub fn with_picture(picture: Picture, offset: Offset) -> Self {
        Self {
            id: LayerId::new(),
            engine_layer: None,
            picture: Some(Arc::new(picture)),
            offset,
            is_complex_hint: false,
            will_change_hint: false,
            needs_add_to_scene: true,
        }
    }

    /// Returns the picture.
    pub fn picture(&self) -> Option<&Arc<Picture>> {
        self.picture.as_ref()
    }

    /// Sets the picture.
    pub fn set_picture(&mut self, picture: Option<Picture>) {
        self.picture = picture.map(Arc::new);
        self.needs_add_to_scene = true;
    }

    /// Returns the offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the offset.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.needs_add_to_scene = true;
        }
    }

    /// Returns whether this layer is complex.
    pub fn is_complex_hint(&self) -> bool {
        self.is_complex_hint
    }

    /// Sets whether this layer is complex.
    pub fn set_is_complex_hint(&mut self, is_complex: bool) {
        self.is_complex_hint = is_complex;
    }

    /// Returns whether this picture will change.
    pub fn will_change_hint(&self) -> bool {
        self.will_change_hint
    }

    /// Sets whether this picture will change.
    pub fn set_will_change_hint(&mut self, will_change: bool) {
        self.will_change_hint = will_change;
    }
}

impl Layer for PictureLayer {
    fn id(&self) -> LayerId {
        self.id
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.engine_layer.as_ref()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.engine_layer = layer;
    }

    fn parent(&self) -> Option<&dyn Layer> {
        None
    }

    fn remove(&mut self) {
        // Picture layers don't have parent tracking
    }

    fn needs_add_to_scene(&self) -> bool {
        self.needs_add_to_scene
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = true;
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        // Leaf layer, nothing to update
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        if let Some(picture) = &self.picture {
            let effective_offset = layer_offset + self.offset;
            builder.add_picture(effective_offset, picture.id().get());
        }
        self.needs_add_to_scene = false;
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Check if offset is within picture bounds
        if let Some(picture) = &self.picture {
            let local_offset = offset - self.offset;
            let point = Point::new(local_offset.dx, local_offset.dy);
            if picture.bounds().contains(point) {
                return Some(self);
            }
        }
        None
    }

    fn bounds(&self) -> Rect {
        self.picture
            .as_ref()
            .map(|p| p.bounds().translate_offset(self.offset))
            .unwrap_or(Rect::ZERO)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_picture_id_unique() {
        let id1 = PictureId::new();
        let id2 = PictureId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_picture_new() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let picture = Picture::new(bounds);
        assert_eq!(picture.bounds(), bounds);
        assert!(!picture.will_change());
    }

    #[test]
    fn test_picture_layer_new() {
        let layer = PictureLayer::new();
        assert!(layer.picture().is_none());
        assert_eq!(layer.offset(), Offset::ZERO);
    }

    #[test]
    fn test_picture_layer_with_picture() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let picture = Picture::new(bounds);
        let offset = Offset::new(10.0, 20.0);

        let layer = PictureLayer::with_picture(picture, offset);
        assert!(layer.picture().is_some());
        assert_eq!(layer.offset(), offset);
    }

    #[test]
    fn test_picture_layer_bounds() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let picture = Picture::new(bounds);
        let offset = Offset::new(10.0, 20.0);

        let layer = PictureLayer::with_picture(picture, offset);
        let layer_bounds = layer.bounds();

        assert_eq!(layer_bounds.left(), 10.0);
        assert_eq!(layer_bounds.top(), 20.0);
        assert_eq!(layer_bounds.width(), 100.0);
        assert_eq!(layer_bounds.height(), 100.0);
    }

    #[test]
    fn test_picture_layer_set_values() {
        let mut layer = PictureLayer::new();

        let bounds = Rect::from_ltwh(0.0, 0.0, 50.0, 50.0);
        let picture = Picture::new(bounds);
        layer.set_picture(Some(picture));
        assert!(layer.picture().is_some());

        layer.set_offset(Offset::new(5.0, 10.0));
        assert_eq!(layer.offset(), Offset::new(5.0, 10.0));

        layer.set_is_complex_hint(true);
        assert!(layer.is_complex_hint());

        layer.set_will_change_hint(true);
        assert!(layer.will_change_hint());
    }
}
