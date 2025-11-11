//! Mock sliver render objects for testing
//!
//! Provides mock implementations of sliver render objects that can be used in tests
//! to verify sliver layout and paint behavior without actual rendering.

use crate::render::{Arity, RenderSliver, SliverLayoutContext, SliverPaintContext};
use flui_types::{Offset, SliverConstraints, SliverGeometry};
use std::sync::{Arc, Mutex};

/// Mock sliver render object for testing
///
/// Records all layout and paint calls for verification in tests.
/// Similar to `MockRender` but for sliver-based rendering.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::MockSliverRender;
/// use flui_types::SliverGeometry;
///
/// let mock = MockSliverRender::leaf(SliverGeometry::simple(100.0, 100.0));
///
/// // Layout the mock
/// let geometry = mock.layout(&ctx);
///
/// // Verify layout was called
/// assert_eq!(mock.layout_call_count(), 1);
/// assert_eq!(geometry.scroll_extent, 100.0);
/// ```
#[derive(Debug, Clone)]
pub struct MockSliverRender {
    /// Fixed geometry to return from layout
    geometry: SliverGeometry,
    /// Number of children (for arity)
    child_count: usize,
    /// Shared state for tracking calls
    state: Arc<Mutex<MockSliverRenderState>>,
}

#[derive(Debug, Default)]
struct MockSliverRenderState {
    layout_calls: usize,
    paint_calls: usize,
    last_constraints: Option<SliverConstraints>,
    last_offset: Option<Offset>,
}

impl MockSliverRender {
    /// Create a new leaf sliver render object (no children)
    ///
    /// # Arguments
    ///
    /// * `geometry` - The geometry to return from layout
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::SliverGeometry;
    /// use flui_core::testing::MockSliverRender;
    ///
    /// let mock = MockSliverRender::leaf(SliverGeometry::simple(200.0, 150.0));
    /// assert_eq!(mock.arity(), Arity::Exact(0));
    /// ```
    pub fn leaf(geometry: SliverGeometry) -> Self {
        Self {
            geometry,
            child_count: 0,
            state: Arc::new(Mutex::new(MockSliverRenderState::default())),
        }
    }

    /// Create a new single-child sliver render object
    ///
    /// # Arguments
    ///
    /// * `geometry` - The geometry to return from layout
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::SliverGeometry;
    /// use flui_core::testing::MockSliverRender;
    ///
    /// let mock = MockSliverRender::single_child(SliverGeometry::simple(200.0, 150.0));
    /// assert_eq!(mock.arity(), Arity::Exact(1));
    /// ```
    pub fn single_child(geometry: SliverGeometry) -> Self {
        Self {
            geometry,
            child_count: 1,
            state: Arc::new(Mutex::new(MockSliverRenderState::default())),
        }
    }

    /// Create a new multi-child sliver render object
    ///
    /// # Arguments
    ///
    /// * `geometry` - The geometry to return from layout
    /// * `child_count` - Number of children
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::SliverGeometry;
    /// use flui_core::testing::MockSliverRender;
    ///
    /// let mock = MockSliverRender::multi_child(SliverGeometry::simple(200.0, 150.0), 5);
    /// assert_eq!(mock.arity(), Arity::Exact(5));
    /// ```
    pub fn multi_child(geometry: SliverGeometry, child_count: usize) -> Self {
        Self {
            geometry,
            child_count,
            state: Arc::new(Mutex::new(MockSliverRenderState::default())),
        }
    }

    /// Create a mock sliver with custom scroll and paint extents
    ///
    /// Convenience method for creating a sliver with specific extents.
    ///
    /// # Arguments
    ///
    /// * `scroll_extent` - Total scrollable extent
    /// * `paint_extent` - Currently visible extent
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_core::testing::MockSliverRender;
    ///
    /// // Create a sliver with 1000px total, 300px visible
    /// let mock = MockSliverRender::with_extents(1000.0, 300.0);
    /// ```
    pub fn with_extents(scroll_extent: f32, paint_extent: f32) -> Self {
        Self::leaf(SliverGeometry::simple(scroll_extent, paint_extent))
    }

    /// Get the number of times layout was called
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mock = MockSliverRender::leaf(SliverGeometry::default());
    /// assert_eq!(mock.layout_call_count(), 0);
    /// // After layout...
    /// assert_eq!(mock.layout_call_count(), 1);
    /// ```
    pub fn layout_call_count(&self) -> usize {
        self.state.lock().unwrap().layout_calls
    }

    /// Get the number of times paint was called
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mock = MockSliverRender::leaf(SliverGeometry::default());
    /// assert_eq!(mock.paint_call_count(), 0);
    /// // After paint...
    /// assert_eq!(mock.paint_call_count(), 1);
    /// ```
    pub fn paint_call_count(&self) -> usize {
        self.state.lock().unwrap().paint_calls
    }

    /// Get the last constraints passed to layout
    ///
    /// Returns `None` if layout has never been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mock = MockSliverRender::leaf(SliverGeometry::default());
    /// assert!(mock.last_constraints().is_none());
    /// // After layout...
    /// assert!(mock.last_constraints().is_some());
    /// ```
    pub fn last_constraints(&self) -> Option<SliverConstraints> {
        self.state.lock().unwrap().last_constraints
    }

    /// Get the last offset passed to paint
    ///
    /// Returns `None` if paint has never been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mock = MockSliverRender::leaf(SliverGeometry::default());
    /// assert!(mock.last_offset().is_none());
    /// // After paint...
    /// assert!(mock.last_offset().is_some());
    /// ```
    pub fn last_offset(&self) -> Option<Offset> {
        self.state.lock().unwrap().last_offset
    }

    /// Reset all call counters and recorded state
    ///
    /// Useful for reusing the same mock across multiple test cases.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mock = MockSliverRender::leaf(SliverGeometry::default());
    /// // ... perform some operations ...
    /// assert_eq!(mock.layout_call_count(), 3);
    ///
    /// mock.reset();
    /// assert_eq!(mock.layout_call_count(), 0);
    /// ```
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.layout_calls = 0;
        state.paint_calls = 0;
        state.last_constraints = None;
        state.last_offset = None;
    }

    /// Update the geometry that will be returned from layout
    ///
    /// Useful for testing dynamic sliver behavior.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut mock = MockSliverRender::leaf(SliverGeometry::simple(100.0, 100.0));
    /// // ... test with initial geometry ...
    ///
    /// mock.set_geometry(SliverGeometry::simple(200.0, 150.0));
    /// // ... test with updated geometry ...
    /// ```
    pub fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }
}

impl RenderSliver for MockSliverRender {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let mut state = self.state.lock().unwrap();
        state.layout_calls += 1;
        state.last_constraints = Some(ctx.constraints);

        self.geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> flui_painting::Canvas {
        let mut state = self.state.lock().unwrap();
        state.paint_calls += 1;
        state.last_offset = Some(ctx.offset);

        flui_painting::Canvas::new()
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

/// Spy sliver render object that delegates to an inner sliver render object
///
/// Useful for wrapping real sliver render objects to track their behavior.
///
/// # Examples
///
/// ```rust,ignore
/// let inner = RenderSliverPadding::new(EdgeInsets::all(10.0));
/// let spy = SpySliverRender::new(inner);
///
/// // Use spy in tests
/// spy.layout(&ctx);
///
/// // Check if layout was called
/// assert_eq!(spy.layout_call_count(), 1);
/// ```
#[derive(Debug)]
pub struct SpySliverRender<R: RenderSliver> {
    inner: R,
    state: Arc<Mutex<MockSliverRenderState>>,
}

impl<R: RenderSliver> SpySliverRender<R> {
    /// Create a new spy sliver render wrapping an inner sliver render object
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let inner = RenderSliverPadding::new(EdgeInsets::all(10.0));
    /// let spy = SpySliverRender::new(inner);
    /// ```
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            state: Arc::new(Mutex::new(MockSliverRenderState::default())),
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
    pub fn last_constraints(&self) -> Option<SliverConstraints> {
        self.state.lock().unwrap().last_constraints
    }

    /// Get the last offset passed to paint
    pub fn last_offset(&self) -> Option<Offset> {
        self.state.lock().unwrap().last_offset
    }

    /// Get a reference to the inner sliver render object
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner sliver render object
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

impl<R: RenderSliver> RenderSliver for SpySliverRender<R> {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let mut state = self.state.lock().unwrap();
        state.layout_calls += 1;
        state.last_constraints = Some(ctx.constraints);
        drop(state);

        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &SliverPaintContext) -> flui_painting::Canvas {
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
    fn test_mock_sliver_render_leaf() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        let mock = MockSliverRender::leaf(geometry);

        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.paint_call_count(), 0);
        assert_eq!(mock.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_mock_sliver_render_single_child() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        let mock = MockSliverRender::single_child(geometry);

        assert_eq!(mock.arity(), Arity::Exact(1));
    }

    #[test]
    fn test_mock_sliver_render_multi_child() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        let mock = MockSliverRender::multi_child(geometry, 3);

        assert_eq!(mock.arity(), Arity::Exact(3));
    }

    #[test]
    fn test_mock_sliver_render_with_extents() {
        let mock = MockSliverRender::with_extents(1000.0, 300.0);

        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_mock_sliver_render_call_tracking() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        let mock = MockSliverRender::leaf(geometry);

        // Test initial state
        assert_eq!(mock.layout_call_count(), 0);
        assert_eq!(mock.paint_call_count(), 0);
        assert!(mock.last_constraints().is_none());
        assert!(mock.last_offset().is_none());
    }

    #[test]
    fn test_mock_sliver_render_reset() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        let mock = MockSliverRender::leaf(geometry);

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

    #[test]
    fn test_set_geometry() {
        let mut mock = MockSliverRender::leaf(SliverGeometry::simple(100.0, 50.0));

        // Update geometry
        mock.set_geometry(SliverGeometry::simple(200.0, 150.0));

        // Verify new geometry is used
        assert_eq!(mock.geometry.scroll_extent, 200.0);
        assert_eq!(mock.geometry.paint_extent, 150.0);
    }
}
