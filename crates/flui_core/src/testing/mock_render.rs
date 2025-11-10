//! Mock render objects for testing
//!
//! Provides mock implementations of render objects that can be used in tests
//! to verify layout and paint behavior without actual rendering.

use crate::render::{Arity, Children, LayoutContext, PaintContext, Render};
use flui_engine::{BoxedLayer, ContainerLayer};
use flui_types::{BoxConstraints, Offset, Size};
use std::sync::{Arc, Mutex};

/// Mock render object for testing
///
/// Records all layout and paint calls for verification in tests.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::MockRender;
///
/// let mock = MockRender::leaf(Size::new(100.0, 50.0));
///
/// // Layout the mock
/// let size = mock.layout(&ctx);
///
/// // Verify layout was called
/// assert_eq!(mock.layout_call_count(), 1);
/// assert_eq!(size, Size::new(100.0, 50.0));
/// ```
#[derive(Debug, Clone)]
pub struct MockRender {
    /// Fixed size to return from layout
    size: Size,
    /// Number of children (for arity)
    child_count: usize,
    /// Shared state for tracking calls
    state: Arc<Mutex<MockRenderState>>,
}

#[derive(Debug, Default)]
struct MockRenderState {
    layout_calls: usize,
    paint_calls: usize,
    last_constraints: Option<BoxConstraints>,
    last_offset: Option<Offset>,
}

impl MockRender {
    /// Create a new leaf render object (no children)
    ///
    /// # Arguments
    ///
    /// * `size` - The size to return from layout
    pub fn leaf(size: Size) -> Self {
        Self {
            size,
            child_count: 0,
            state: Arc::new(Mutex::new(MockRenderState::default())),
        }
    }

    /// Create a new single-child render object
    ///
    /// # Arguments
    ///
    /// * `size` - The size to return from layout
    pub fn single_child(size: Size) -> Self {
        Self {
            size,
            child_count: 1,
            state: Arc::new(Mutex::new(MockRenderState::default())),
        }
    }

    /// Create a new multi-child render object
    ///
    /// # Arguments
    ///
    /// * `size` - The size to return from layout
    /// * `child_count` - Number of children
    pub fn multi_child(size: Size, child_count: usize) -> Self {
        Self {
            size,
            child_count,
            state: Arc::new(Mutex::new(MockRenderState::default())),
        }
    }

    /// Get the number of times layout was called
    pub fn layout_call_count(&self) -> usize {
        self.state.lock().unwrap().layout_calls
    }

    /// Get the number of times paint was called
    pub fn paint_call_count(&self) -> usize {
        self.state.lock().unwrap().paint_calls
    }

    /// Get the last constraints passed to layout
    pub fn last_constraints(&self) -> Option<BoxConstraints> {
        self.state.lock().unwrap().last_constraints
    }

    /// Get the last offset passed to paint
    pub fn last_offset(&self) -> Option<Offset> {
        self.state.lock().unwrap().last_offset
    }

    /// Reset all call counters
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.layout_calls = 0;
        state.paint_calls = 0;
        state.last_constraints = None;
        state.last_offset = None;
    }
}

impl Render for MockRender {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let mut state = self.state.lock().unwrap();
        state.layout_calls += 1;
        state.last_constraints = Some(*ctx.constraints);

        // For single/multi child, constrain size
        if self.child_count > 0 {
            ctx.constraints.constrain(self.size)
        } else {
            self.size
        }
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let mut state = self.state.lock().unwrap();
        state.paint_calls += 1;
        state.last_offset = Some(ctx.offset);

        Box::new(ContainerLayer::new())
    }

    fn arity(&self) -> Arity {
        match self.child_count {
            0 => Arity::Exact(0),
            1 => Arity::Exact(1),
            n => Arity::Exact(n),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Spy render object that delegates to an inner render object
///
/// Useful for wrapping real render objects to track their behavior.
///
/// # Examples
///
/// ```rust,ignore
/// let inner = RenderPadding::new(EdgeInsets::all(10.0));
/// let spy = SpyRender::new(inner);
///
/// // Use spy in tests
/// spy.layout(&ctx);
///
/// // Check if layout was called
/// assert_eq!(spy.layout_call_count(), 1);
/// ```
#[derive(Debug)]
pub struct SpyRender<R: Render> {
    inner: R,
    state: Arc<Mutex<MockRenderState>>,
}

impl<R: Render> SpyRender<R> {
    /// Create a new spy render wrapping an inner render object
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            state: Arc::new(Mutex::new(MockRenderState::default())),
        }
    }

    /// Get the number of times layout was called
    pub fn layout_call_count(&self) -> usize {
        self.state.lock().unwrap().layout_calls
    }

    /// Get the number of times paint was called
    pub fn paint_call_count(&self) -> usize {
        self.state.lock().unwrap().paint_calls
    }

    /// Get the last constraints passed to layout
    pub fn last_constraints(&self) -> Option<BoxConstraints> {
        self.state.lock().unwrap().last_constraints
    }

    /// Get the last offset passed to paint
    pub fn last_offset(&self) -> Option<Offset> {
        self.state.lock().unwrap().last_offset
    }

    /// Get a reference to the inner render object
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner render object
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Reset all call counters
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.layout_calls = 0;
        state.paint_calls = 0;
        state.last_constraints = None;
        state.last_offset = None;
    }
}

impl<R: Render> Render for SpyRender<R> {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let mut state = self.state.lock().unwrap();
        state.layout_calls += 1;
        state.last_constraints = Some(*ctx.constraints);
        drop(state);

        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let mut state = self.state.lock().unwrap();
        state.paint_calls += 1;
        state.last_offset = Some(ctx.offset);
        drop(state);

        self.inner.paint(ctx)
    }

    fn arity(&self) -> Arity {
        self.inner.arity()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_render_leaf() {
        let mock = MockRender::leaf(Size::new(100.0, 50.0));

        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.paint_call_count(), 0);
        assert_eq!(mock.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_mock_render_call_tracking() {
        let mock = MockRender::leaf(Size::new(100.0, 50.0));

        // Test initial state
        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.paint_call_count(), 0);
    }

    #[test]
    fn test_mock_render_reset() {
        let mock = MockRender::leaf(Size::new(100.0, 50.0));

        // Simulate some calls by directly updating state
        {
            let mut state = mock.state.lock().unwrap();
            state.layout_calls = 5;
            state.paint_calls = 3;
        }

        assert_eq!(mock.layout_call_count(), 5);
        assert_eq!(mock.paint_call_count(), 3);

        mock.reset();

        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.paint_call_count(), 0);
    }
}
