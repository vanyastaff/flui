//! Integration tests for FrameCoordinator
//!
//! This module contains comprehensive integration tests for the `FrameCoordinator`
//! component, ensuring correct orchestration of build→layout→paint phases.
//!
//! # Test Categories
//!
//! 1. **Basic Flow Tests**: Happy path scenarios
//! 2. **Error Handling Tests**: Failure mode verification
//! 3. **Phase Isolation Tests**: Individual phase testing
//! 4. **Edge Cases**: Empty trees, missing roots, etc.
//!
//! # Test Utilities
//!
//! This module provides helper functions and fixtures for testing:
//! - `TestFixture`: Comprehensive test harness
//! - `create_test_tree()`: Generate test element trees
//! - `create_mock_element()`: Create test elements

#[cfg(test)]
mod tests {
    use super::super::{ElementTree, FrameCoordinator};
    use crate::element::{Element, ElementId, RenderElement};
    use crate::foundation::Slot;
    use crate::render::LeafRender;
    use crate::BoxedLayer;
    use flui_types::constraints::BoxConstraints;
    use flui_types::{Offset, Size};
    use parking_lot::RwLock;
    use std::sync::Arc;

    // =========================================================================
    // Test Mock Render Object
    // =========================================================================

    /// Mock render object for testing
    struct MockRender;

    impl LeafRender for MockRender {
        type Metadata = ();

        fn layout(&mut self, _constraints: BoxConstraints) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _offset: Offset) -> BoxedLayer {
            Box::new(flui_engine::ContainerLayer::new())
        }
    }

    // =========================================================================
    // Test Utilities
    // =========================================================================

    /// Test fixture providing a complete test environment for FrameCoordinator
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let fixture = TestFixture::new();
    /// let result = fixture.coordinator.build_frame(
    ///     &fixture.tree,
    ///     Some(fixture.root),
    ///     BoxConstraints::tight(Size::new(800.0, 600.0))
    /// );
    /// ```
    struct TestFixture {
        coordinator: FrameCoordinator,
        tree: Arc<RwLock<ElementTree>>,
        root: ElementId,
    }

    impl TestFixture {
        /// Create a new test fixture with a simple tree
        ///
        /// Tree structure:
        /// ```text
        /// root
        ///  ├─ child1
        ///  └─ child2
        /// ```
        fn new() -> Self {
            let coordinator = FrameCoordinator::new();
            let tree = Arc::new(RwLock::new(ElementTree::new()));

            // Create simple tree
            let root = {
                let mut tree_guard = tree.write();

                // Root
                let mut root_elem = create_mock_element();
                root_elem.mount(None, Some(Slot::new(0)));
                let root_id = tree_guard.insert(root_elem);

                // Child 1
                let mut child1 = create_mock_element();
                child1.mount(Some(root_id), Some(Slot::new(0)));
                tree_guard.insert(child1);

                // Child 2
                let mut child2 = create_mock_element();
                child2.mount(Some(root_id), Some(Slot::new(1)));
                tree_guard.insert(child2);

                root_id
            };

            Self {
                coordinator,
                tree,
                root,
            }
        }

        /// Create a fixture with a specific tree size
        fn with_size(size: usize) -> Self {
            let coordinator = FrameCoordinator::new();
            let tree = Arc::new(RwLock::new(ElementTree::new()));

            let root = {
                let mut tree_guard = tree.write();

                // Root
                let mut root_elem = create_mock_element();
                root_elem.mount(None, Some(Slot::new(0)));
                let root_id = tree_guard.insert(root_elem);

                // Add children
                for i in 0..size {
                    let mut child = create_mock_element();
                    child.mount(Some(root_id), Some(Slot::new(i)));
                    tree_guard.insert(child);
                }

                root_id
            };

            Self {
                coordinator,
                tree,
                root,
            }
        }

        /// Create an empty fixture (no elements)
        fn empty() -> (FrameCoordinator, Arc<RwLock<ElementTree>>) {
            let coordinator = FrameCoordinator::new();
            let tree = Arc::new(RwLock::new(ElementTree::new()));
            (coordinator, tree)
        }
    }

    /// Create a mock render element for testing
    fn create_mock_element() -> Element {
        Element::Render(RenderElement::new(Box::new(MockRender)))
    }

    // =========================================================================
    // 1. Basic Flow Tests
    // =========================================================================

    #[test]
    fn test_frame_coordinator_creation() {
        let coordinator = FrameCoordinator::new();

        // Verify initial state
        assert_eq!(coordinator.build().dirty_count(), 0);
        assert_eq!(coordinator.layout().dirty_count(), 0);
        assert_eq!(coordinator.paint().dirty_count(), 0);
    }

    #[test]
    fn test_frame_coordinator_default() {
        let coordinator = FrameCoordinator::default();

        // Default should be same as new()
        assert_eq!(coordinator.build().dirty_count(), 0);
    }

    #[test]
    fn test_frame_coordinator_accessors() {
        let mut coordinator = FrameCoordinator::new();

        // Test immutable accessors
        let _build = coordinator.build();
        let _layout = coordinator.layout();
        let _paint = coordinator.paint();
        let _scheduler = coordinator.scheduler();

        // Test mutable accessors
        let _build_mut = coordinator.build_mut();
        let _layout_mut = coordinator.layout_mut();
        let _paint_mut = coordinator.paint_mut();
        let _scheduler_mut = coordinator.scheduler_mut();
    }

    #[test]
    fn test_build_frame_with_empty_tree() {
        let (mut coordinator, tree) = TestFixture::empty();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let result = coordinator.build_frame(&tree, None, constraints);

        // Should succeed but return None (no root)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_frame_with_simple_tree() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let result =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        // Should succeed and return a layer
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_build_frame_with_large_tree() {
        let mut fixture = TestFixture::with_size(100);

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let result =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        // Should handle large trees
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_build_frame_with_different_constraints() {
        let mut fixture = TestFixture::new();

        // Test various constraint scenarios
        let test_cases = vec![
            BoxConstraints::tight(Size::new(800.0, 600.0)),
            BoxConstraints::tight(Size::new(1920.0, 1080.0)),
            BoxConstraints::tight(Size::new(400.0, 300.0)),
            BoxConstraints::loose(Size::new(1000.0, 800.0)),
        ];

        for constraints in test_cases {
            let result =
                fixture
                    .coordinator
                    .build_frame(&fixture.tree, Some(fixture.root), constraints);

            assert!(result.is_ok(), "Failed with constraints: {:?}", constraints);
        }
    }

    #[test]
    fn test_build_frame_updates_scheduler() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Initial state
        assert_eq!(fixture.coordinator.scheduler().total_frames(), 0);

        // Build frame
        let _ = fixture
            .coordinator
            .build_frame(&fixture.tree, Some(fixture.root), constraints);

        // Scheduler should track the frame
        assert_eq!(fixture.coordinator.scheduler().total_frames(), 1);
    }

    // =========================================================================
    // 2. Error Handling Tests
    // =========================================================================

    #[test]
    fn test_build_frame_with_invalid_root() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Use non-existent root ID
        let invalid_root = 9999;
        let result =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(invalid_root), constraints);

        // Should succeed but return None (root not found)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    // =========================================================================
    // 3. Phase Isolation Tests
    // =========================================================================

    #[test]
    fn test_flush_build_only() {
        let mut fixture = TestFixture::new();

        // Mark elements as dirty
        {
            let tree_guard = fixture.tree.read();
            fixture
                .coordinator
                .build_mut()
                .mark_dirty(fixture.root, &tree_guard);
        }

        let initial_dirty = fixture.coordinator.build().dirty_count();
        assert!(initial_dirty > 0, "Should have dirty elements");

        // Flush build phase only
        fixture.coordinator.flush_build(&fixture.tree);

        // Build queue should be empty
        assert_eq!(fixture.coordinator.build().dirty_count(), 0);
    }

    #[test]
    fn test_flush_layout_only() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Flush layout phase
        let result =
            fixture
                .coordinator
                .flush_layout(&fixture.tree, Some(fixture.root), constraints);

        // Should succeed
        assert!(result.is_ok());
    }

    #[test]
    fn test_flush_paint_only() {
        let mut fixture = TestFixture::new();

        // Flush paint phase
        let result = fixture
            .coordinator
            .flush_paint(&fixture.tree, Some(fixture.root));

        // Should succeed and return a layer
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_phase_independence() {
        let mut fixture = TestFixture::new();

        // Each phase should be callable independently
        fixture.coordinator.flush_build(&fixture.tree);

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let _ = fixture
            .coordinator
            .flush_layout(&fixture.tree, Some(fixture.root), constraints);

        let _ = fixture
            .coordinator
            .flush_paint(&fixture.tree, Some(fixture.root));
    }

    // =========================================================================
    // 4. Edge Cases
    // =========================================================================

    #[test]
    fn test_multiple_build_frames() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Build multiple frames
        for _ in 0..10 {
            let result =
                fixture
                    .coordinator
                    .build_frame(&fixture.tree, Some(fixture.root), constraints);

            assert!(result.is_ok());
        }

        // Scheduler should track all frames
        assert_eq!(fixture.coordinator.scheduler().total_frames(), 10);
    }

    #[test]
    fn test_build_frame_with_zero_size_constraints() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(0.0, 0.0));
        let result =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        // Should handle zero-size constraints
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_frame_idempotent() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Build frame twice without changes
        let result1 =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        let result2 =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        // Both should succeed
        assert!(result1.is_ok());
        assert!(result2.is_ok());
    }

    #[test]
    fn test_scheduler_integration() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Build some frames
        for _ in 0..5 {
            let _ = fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);
        }

        // Check scheduler metrics
        let scheduler = fixture.coordinator.scheduler();
        assert_eq!(scheduler.total_frames(), 5);
        assert_eq!(scheduler.skipped_frames(), 0);
        assert_eq!(scheduler.skip_rate(), 0.0);
    }

    // =========================================================================
    // 5. Concurrency Tests (with Arc<RwLock>)
    // =========================================================================

    #[test]
    fn test_concurrent_tree_access() {
        let mut fixture = TestFixture::new();

        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Build frame (acquires locks internally)
        let result =
            fixture
                .coordinator
                .build_frame(&fixture.tree, Some(fixture.root), constraints);

        assert!(result.is_ok());

        // Tree should be accessible after build_frame
        let tree_guard = fixture.tree.read();
        assert!(tree_guard.get(fixture.root).is_some());
    }
}
