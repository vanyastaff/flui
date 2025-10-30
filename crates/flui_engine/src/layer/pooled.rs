//! Pooled layer wrappers with automatic return to pool on drop
//!
//! This module provides wrapper types that automatically return layers
//! to the pool when they're dropped, improving pool hit rates.

use super::{BoxedLayer, ContainerLayer, Layer, ClipRectLayer, PictureLayer, pool};
use crate::painter::Painter;
use flui_types::{Rect, Offset};
use flui_types::events::{Event, HitTestResult};

/// Wrapper for ContainerLayer that automatically returns to pool on drop
pub struct PooledContainerLayer {
    inner: Option<ContainerLayer>,
}

impl PooledContainerLayer {
    /// Create from a ContainerLayer (typically from pool::acquire_container())
    pub fn new(container: ContainerLayer) -> Self {
        Self {
            inner: Some(container),
        }
    }

    /// Get mutable reference to inner layer
    pub fn as_mut(&mut self) -> &mut ContainerLayer {
        self.inner.as_mut().expect("PooledContainerLayer already consumed")
    }

    /// Take the inner layer, consuming self without returning to pool
    /// Useful when you need to pass ownership elsewhere
    pub fn take(mut self) -> ContainerLayer {
        self.inner.take().expect("PooledContainerLayer already consumed")
    }
}

impl Drop for PooledContainerLayer {
    fn drop(&mut self) {
        if let Some(container) = self.inner.take() {
            pool::release_container(container);
        }
    }
}

impl Layer for PooledContainerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if let Some(ref inner) = self.inner {
            inner.paint(painter);
        }
    }

    fn bounds(&self) -> Rect {
        self.inner.as_ref().map(|l| l.bounds()).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        self.inner.as_ref().map(|l| l.is_visible()).unwrap_or(false)
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.inner.as_ref().map(|l| l.hit_test(position, result)).unwrap_or(false)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.inner.as_mut().map(|l| l.handle_event(event)).unwrap_or(false)
    }

    fn dispose(&mut self) {
        if let Some(ref mut inner) = self.inner {
            inner.dispose();
        }
    }
}

/// Wrapper for ClipRectLayer that automatically returns to pool on drop
pub struct PooledClipRectLayer {
    inner: Option<ClipRectLayer>,
}

impl PooledClipRectLayer {
    /// Create from a ClipRectLayer (typically from pool::acquire_clip_rect())
    pub fn new(clip_rect: ClipRectLayer) -> Self {
        Self {
            inner: Some(clip_rect),
        }
    }

    /// Get mutable reference to inner layer
    pub fn as_mut(&mut self) -> &mut ClipRectLayer {
        self.inner.as_mut().expect("PooledClipRectLayer already consumed")
    }

    /// Take the inner layer, consuming self without returning to pool
    pub fn take(mut self) -> ClipRectLayer {
        self.inner.take().expect("PooledClipRectLayer already consumed")
    }
}

impl Drop for PooledClipRectLayer {
    fn drop(&mut self) {
        if let Some(clip_rect) = self.inner.take() {
            pool::release_clip_rect(clip_rect);
        }
    }
}

impl Layer for PooledClipRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if let Some(ref inner) = self.inner {
            inner.paint(painter);
        }
    }

    fn bounds(&self) -> Rect {
        self.inner.as_ref().map(|l| l.bounds()).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        self.inner.as_ref().map(|l| l.is_visible()).unwrap_or(false)
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.inner.as_ref().map(|l| l.hit_test(position, result)).unwrap_or(false)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.inner.as_mut().map(|l| l.handle_event(event)).unwrap_or(false)
    }

    fn dispose(&mut self) {
        if let Some(ref mut inner) = self.inner {
            inner.dispose();
        }
    }
}

/// Helper function to create a pooled container layer
pub fn acquire_pooled_container() -> PooledContainerLayer {
    PooledContainerLayer::new(pool::acquire_container())
}

/// Helper function to create a pooled clip rect layer
pub fn acquire_pooled_clip_rect() -> PooledClipRectLayer {
    PooledClipRectLayer::new(pool::acquire_clip_rect())
}

/// Wrapper for PictureLayer that automatically returns to pool on drop
pub struct PooledPictureLayer {
    inner: Option<PictureLayer>,
}

impl PooledPictureLayer {
    /// Create from a PictureLayer (typically from pool::acquire_picture())
    pub fn new(picture: PictureLayer) -> Self {
        Self {
            inner: Some(picture),
        }
    }

    /// Get mutable reference to inner layer
    pub fn as_mut(&mut self) -> &mut PictureLayer {
        self.inner.as_mut().expect("PooledPictureLayer already consumed")
    }

    /// Take the inner layer, consuming self without returning to pool
    pub fn take(mut self) -> PictureLayer {
        self.inner.take().expect("PooledPictureLayer already consumed")
    }
}

impl Drop for PooledPictureLayer {
    fn drop(&mut self) {
        if let Some(picture) = self.inner.take() {
            pool::release_picture(picture);
        }
    }
}

impl Layer for PooledPictureLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if let Some(ref inner) = self.inner {
            inner.paint(painter);
        }
    }

    fn bounds(&self) -> Rect {
        self.inner.as_ref().map(|l| l.bounds()).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        self.inner.as_ref().map(|l| l.is_visible()).unwrap_or(false)
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.inner.as_ref().map(|l| l.hit_test(position, result)).unwrap_or(false)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.inner.as_mut().map(|l| l.handle_event(event)).unwrap_or(false)
    }

    fn dispose(&mut self) {
        if let Some(ref mut inner) = self.inner {
            inner.dispose();
        }
    }
}

/// Helper function to create a pooled picture layer
pub fn acquire_pooled_picture() -> PooledPictureLayer {
    PooledPictureLayer::new(pool::acquire_picture())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pooled_container_auto_return() {
        pool::clear_all_pools();

        {
            let _pooled = acquire_pooled_container();
            // pooled will be dropped here and returned to pool
        }

        assert_eq!(pool::container_pool_size(), 1);
    }

    #[test]
    fn test_pooled_container_take() {
        pool::clear_all_pools();

        {
            let pooled = acquire_pooled_container();
            let _container = pooled.take(); // take ownership, don't return to pool
            // container dropped here without returning to pool
        }

        assert_eq!(pool::container_pool_size(), 0);
    }

    #[test]
    fn test_pooled_clip_rect_auto_return() {
        pool::clear_all_pools();

        {
            let _pooled = acquire_pooled_clip_rect();
            // pooled will be dropped here and returned to pool
        }

        assert_eq!(pool::clip_rect_pool_size(), 1);
    }
}
