//! LeaderLayer - Linked positioning anchor
//!
//! This layer establishes a coordinate space that FollowerLayer instances
//! can link to. Used for tooltips, dropdowns, and connected overlays.

use flui_types::geometry::{Offset, Pixels, Rect, Size};
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for leader-follower linkage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerLink {
    id: u64,
}

impl LayerLink {
    /// Creates a new unique layer link.
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Returns the internal ID for debugging.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Default for LayerLink {
    fn default() -> Self {
        Self::new()
    }
}

/// Layer that establishes a coordinate space for linked positioning.
///
/// A LeaderLayer creates an anchor point that FollowerLayer instances
/// can attach to. When the leader moves, all linked followers move with it.
///
/// # Use Cases
///
/// - Tooltips that follow a target widget
/// - Dropdown menus attached to buttons
/// - Popups anchored to specific locations
/// - Connected overlay effects
///
/// # Architecture
///
/// ```text
/// LeaderLayer (anchor)
///   │
///   │ Provides coordinate space via LayerLink
///   ▼
/// FollowerLayer(s)
///   │
///   │ Transform relative to leader
///   ▼
/// Content positioned relative to anchor
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::{LeaderLayer, FollowerLayer, LayerLink};
/// use flui_types::geometry::{Offset, Size};
///
/// // Create a link between leader and follower
/// let link = LayerLink::new();
///
/// // Leader defines the anchor point
/// let leader = LeaderLayer::new(link, Size::new(100.0, 30.0));
///
/// // Follower positions relative to the leader
/// let follower = FollowerLayer::new(link)
///     .with_target_offset(Offset::new(0.0, 35.0)); // Below the leader
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaderLayer {
    /// Link for follower attachment
    link: LayerLink,

    /// Size of the leader area
    size: Size<Pixels>,

    /// Offset from parent
    offset: Offset<Pixels>,
}

impl LeaderLayer {
    /// Creates a new leader layer with the given link and size.
    #[inline]
    pub fn new(link: LayerLink, size: Size<Pixels>) -> Self {
        Self {
            link,
            size,
            offset: Offset::ZERO,
        }
    }

    /// Creates a leader layer with an offset.
    #[inline]
    pub fn with_offset(link: LayerLink, size: Size<Pixels>, offset: Offset<Pixels>) -> Self {
        Self { link, size, offset }
    }

    /// Sets the offset.
    #[inline]
    pub fn offset(mut self, offset: Offset<Pixels>) -> Self {
        self.offset = offset;
        self
    }

    /// Returns the layer link.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// Returns the size.
    #[inline]
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Returns the offset.
    #[inline]
    pub fn get_offset(&self) -> Offset<Pixels> {
        self.offset
    }

    /// Returns the bounds.
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::from_xywh(
            self.offset.dx,
            self.offset.dy,
            self.size.width,
            self.size.height,
        )
    }

    /// Sets the size.
    #[inline]
    pub fn set_size(&mut self, size: Size<Pixels>) {
        self.size = size;
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset<Pixels>) {
        self.offset = offset;
    }
}

// Thread safety
unsafe impl Send for LeaderLayer {}
unsafe impl Sync for LeaderLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_link_unique() {
        let link1 = LayerLink::new();
        let link2 = LayerLink::new();

        assert_ne!(link1, link2);
        assert_ne!(link1.id(), link2.id());
    }

    #[test]
    fn test_leader_layer_new() {
        let link = LayerLink::new();
        let size = Size::new(100.0, 50.0);
        let layer = LeaderLayer::new(link, size);

        assert_eq!(layer.link(), link);
        assert_eq!(layer.size(), size);
        assert_eq!(layer.get_offset(), Offset::ZERO);
    }

    #[test]
    fn test_leader_layer_with_offset() {
        let link = LayerLink::new();
        let size = Size::new(100.0, 50.0);
        let offset = Offset::new(10.0, 20.0);
        let layer = LeaderLayer::with_offset(link, size, offset);

        assert_eq!(layer.get_offset(), offset);
    }

    #[test]
    fn test_leader_layer_bounds() {
        let link = LayerLink::new();
        let size = Size::new(100.0, 50.0);
        let offset = Offset::new(10.0, 20.0);
        let layer = LeaderLayer::with_offset(link, size, offset);

        let bounds = layer.bounds();
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 20.0);
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 50.0);
    }

    #[test]
    fn test_leader_layer_setters() {
        let link = LayerLink::new();
        let mut layer = LeaderLayer::new(link, Size::new(10.0, 10.0));

        layer.set_size(Size::new(200.0, 100.0));
        layer.set_offset(Offset::new(5.0, 5.0));

        assert_eq!(layer.size(), Size::new(200.0, 100.0));
        assert_eq!(layer.get_offset(), Offset::new(5.0, 5.0));
    }

    #[test]
    fn test_leader_layer_builder() {
        let link = LayerLink::new();
        let layer = LeaderLayer::new(link, Size::new(100.0, 50.0)).offset(Offset::new(10.0, 20.0));

        assert_eq!(layer.get_offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_leader_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<LeaderLayer>();
        assert_sync::<LeaderLayer>();
        assert_send::<LayerLink>();
        assert_sync::<LayerLink>();
    }
}
