//! Testing utilities
//!
//! This module provides utilities for testing FLUI applications and components.
//!
//! # Overview
//!
//! Testing utilities make it easy to:
//! - Test views in isolation with [`TestHarness`]
//! - Mock render objects with [`MockRender`]
//! - Assert on element tree state with [assertion helpers](assertions)
//! - Build test views with [`ViewTester`]
//!
//! # Components
//!
//! ## Test Harness
//!
//! [`TestHarness`] provides a controlled environment for building and testing view trees:
//!
//! ```rust,ignore
//! use flui_core::testing::TestHarness;
//!
//! #[test]
//! fn test_my_view() {
//!     let mut harness = TestHarness::new();
//!     harness.mount(MyView::new());
//!     harness.pump();
//!
//!     assert!(harness.is_mounted());
//! }
//! ```
//!
//! ## Mock Render Objects
//!
//! [`MockRender`] and [`SpyRender`] allow testing render objects without actual rendering:
//!
//! ```rust,ignore
//! use flui_core::testing::MockRender;
//! use flui_types::Size;
//!
//! let mock = MockRender::leaf(Size::new(100.0, 50.0));
//! // Use in tests, verify layout/paint calls
//! assert_eq!(mock.layout_call_count(), 0);
//! ```
//!
//! ## View Testing
//!
//! [`ViewTester`] provides a fluent API for testing views:
//!
//! ```rust,ignore
//! use flui_core::testing::ViewTester;
//!
//! #[test]
//! fn test_view_builds() {
//!     let result = ViewTester::new()
//!         .with_view(MyView::new());
//!
//!     assert!(result.is_mounted());
//! }
//! ```
//!
//! ## Assertions
//!
//! Assertion helpers for common checks:
//!
//! ```rust,ignore
//! use flui_core::testing::assertions::*;
//!
//! assert_element_exists(&tree, element_id);
//! assert_is_component(&tree, element_id);
//! assert_element_size(&tree, element_id, Size::new(100.0, 50.0));
//! ```
//!
//! # Best Practices
//!
//! 1. **Use TestHarness for integration tests** - Test full view trees
//! 2. **Use MockRender for unit tests** - Test individual render objects
//! 3. **Use ViewTester for view tests** - Test view build logic
//! 4. **Use assertion helpers** - Get clear, descriptive error messages
//!
//! # Examples
//!
//! ## Testing a Simple View
//!
//! ```rust,ignore
//! use flui_core::{View, BuildContext, testing::*};
//!
//! #[derive(Clone)]
//! struct Counter {
//!     count: i32,
//! }
//!
//! impl View for Counter {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         Text::new(format!("Count: {}", self.count))
//!     }
//! }
//!
//! #[test]
//! fn test_counter_builds() {
//!     let result = ViewTester::new()
//!         .with_view(Counter { count: 0 });
//!
//!     assert!(result.is_mounted());
//! }
//! ```
//!
//! ## Testing Render Objects
//!
//! ```rust,ignore
//! use flui_core::testing::MockRender;
//! use flui_types::{Size, BoxConstraints};
//!
//! #[test]
//! fn test_render_layout() {
//!     let mut mock = MockRender::leaf(Size::new(100.0, 50.0));
//!     let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
//!
//!     let ctx = LayoutContext::new(constraints, Children::None);
//!     let size = mock.layout(&ctx);
//!
//!     assert_eq!(size, Size::new(100.0, 50.0));
//!     assert_eq!(mock.layout_call_count(), 1);
//! }
//! ```

// Temporarily disabled pending API updates to new View system
// TODO: Update testing utilities to work with new View API (no rebuild(), uses IntoElement)
// pub mod assertions;
// pub mod mock_render;
// pub mod test_harness;
// pub mod view_tester;

// Re-export main types for convenience
// pub use assertions::*;
// pub use mock_render::{MockRender, SpyRender};
// pub use test_harness::TestHarness;
// pub use view_tester::{TestView, ViewTestResult, ViewTester};
