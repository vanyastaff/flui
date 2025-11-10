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
//! # Additional Utilities
//!
//! ## Test Fixtures
//!
//! Pre-configured test fixtures to reduce boilerplate:
//!
//! ```rust,ignore
//! use flui_core::testing::*;
//!
//! // Use standard test constraints
//! let constraints = TEST_CONSTRAINTS;
//!
//! // Or create specific sizes
//! let size = sizes::LARGE; // 800x600
//!
//! // Quick test helpers
//! let (harness, root_id) = quick_test_pump(MyView::new());
//! ```
//!
//! ## Snapshot Testing
//!
//! Capture and compare element tree snapshots:
//!
//! ```rust,ignore
//! use flui_core::testing::*;
//!
//! let snapshot = ElementTreeSnapshot::capture(&tree);
//! assert!(snapshot.matches(2, 3, 1)); // 2 components, 3 renders, 1 provider
//!
//! // Or use the helper
//! assert_tree_snapshot(&tree, 2, 3, 1);
//! ```
//!
//! ## Tree Inspection
//!
//! Debug and inspect element tree state:
//!
//! ```rust,ignore
//! use flui_core::testing::*;
//!
//! // Print tree structure
//! print_tree(&tree);
//!
//! // Get detailed summary
//! let summary = tree_summary(&tree);
//! println!("{}", summary);
//!
//! // Advanced inspection
//! let inspector = TreeInspector::new(&tree);
//! let dirty_elements = inspector.find_dirty();
//! ```
//!
//! ## Test Macros
//!
//! Convenient macros for common patterns:
//!
//! ```rust,ignore
//! use flui_core::testing::*;
//!
//! // Quick test with automatic setup
//! quick_test!(MyView::new(), |harness, root_id| {
//!     assert!(harness.is_mounted());
//! });
//!
//! // Assert tree structure
//! assert_tree!(tree, {
//!     components: 2,
//!     renders: 3,
//!     providers: 1
//! });
//! ```
//!
//! # Best Practices
//!
//! 1. **Use TestHarness for integration tests** - Test full view trees
//! 2. **Use MockRender for unit tests** - Test individual render objects
//! 3. **Use ViewTester for view tests** - Test view build logic
//! 4. **Use assertion helpers** - Get clear, descriptive error messages
//! 5. **Use fixtures** - Avoid duplicating test setup code
//! 6. **Use snapshots** - Track changes to tree structure over time
//! 7. **Use inspection tools** - Debug failing tests easily
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

pub mod assertions;
pub mod fixtures;
pub mod helpers;
pub mod inspect;
pub mod macros;
pub mod mock_render;
pub mod snapshot;
pub mod test_harness;
pub mod view_tester;






// Re-export main types for convenience
pub use assertions::*;
pub use fixtures::*;
pub use helpers::{test_hook_context, test_hook_context_with_id};
pub use inspect::{print_tree, tree_summary, TreeInspector, TreeSummary};
pub use mock_render::{MockRender, SpyRender};
pub use snapshot::{assert_tree_snapshot, ElementTreeSnapshot, SnapshotDiff};
pub use test_harness::TestHarness;
pub use view_tester::{TestView, TestWidget, ViewTestResult, ViewTester};





