//! Test fixtures and builders
//!
//! Provides reusable test fixtures to avoid duplication in tests.

use crate::{
    element::{ElementId, ElementTree},
    pipeline::PipelineOwner,
    view::BuildContext,
    View,
};
use flui_types::{BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

use super::TestHarness;

/// Standard test constraints (800x600)
pub const TEST_CONSTRAINTS: BoxConstraints = BoxConstraints {
    min_width: 0.0,
    max_width: 800.0,
    min_height: 0.0,
    max_height: 600.0,
};

/// Tight test constraints (800x600)
pub fn tight_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(800.0, 600.0))
}

/// Loose test constraints (unbounded)
pub fn loose_constraints() -> BoxConstraints {
    BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY))
}

/// Fixed size constraints
pub fn fixed_size_constraints(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::tight(Size::new(width, height))
}

/// Common test sizes
pub mod sizes {
    use flui_types::Size;

    /// Small size (100x100)
    pub const SMALL: Size = Size {
        width: 100.0,
        height: 100.0,
    };

    /// Medium size (300x200)
    pub const MEDIUM: Size = Size {
        width: 300.0,
        height: 200.0,
    };

    /// Large size (800x600)
    pub const LARGE: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    /// Square size (200x200)
    pub const SQUARE: Size = Size {
        width: 200.0,
        height: 200.0,
    };
}

/// Test harness builder with common configurations
///
/// # Examples
///
/// ```rust,ignore
/// let harness = TestHarnessBuilder::new()
///     .with_size(800.0, 600.0)
///     .build();
/// ```
#[derive(Debug)]
pub struct TestHarnessBuilder {
    size: Option<Size>,
}

impl TestHarnessBuilder {
    /// Create a new test harness builder
    pub fn new() -> Self {
        Self { size: None }
    }

    /// Set the viewport size for testing
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Some(Size::new(width, height));
        self
    }

    /// Use standard desktop size (800x600)
    pub fn desktop(self) -> Self {
        self.with_size(800.0, 600.0)
    }

    /// Use mobile portrait size (375x667)
    pub fn mobile_portrait(self) -> Self {
        self.with_size(375.0, 667.0)
    }

    /// Use mobile landscape size (667x375)
    pub fn mobile_landscape(self) -> Self {
        self.with_size(667.0, 375.0)
    }

    /// Use tablet size (768x1024)
    pub fn tablet(self) -> Self {
        self.with_size(768.0, 1024.0)
    }

    /// Build the test harness
    pub fn build(self) -> TestHarness {
        TestHarness::new()
    }
}

impl Default for TestHarnessBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Quick test helper - create and mount a view in one call
///
/// # Examples
///
/// ```rust,ignore
/// let (harness, root_id) = quick_test(MyView::new());
/// ```
pub fn quick_test<V: View>(view: V) -> (TestHarness, ElementId) {
    let mut harness = TestHarness::new();
    let root_id = harness.mount(view);
    (harness, root_id)
}

/// Quick test with pump - create, mount, and pump the pipeline
///
/// # Examples
///
/// ```rust,ignore
/// let (harness, root_id) = quick_test_pump(MyView::new());
/// // Pipeline has been pumped, ready to assert
/// ```
pub fn quick_test_pump<V: View>(view: V) -> (TestHarness, ElementId) {
    let mut harness = TestHarness::new();
    let root_id = harness.mount(view);
    harness.pump();
    (harness, root_id)
}

/// Create a minimal build context for testing
///
/// # Examples
///
/// ```rust,ignore
/// let ctx = test_build_context();
/// let element = my_view.build(&ctx);
/// ```
pub fn test_build_context() -> BuildContext {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    BuildContext::new(tree, ElementId::new(1))
}

/// Test pipeline owner
pub fn test_pipeline() -> PipelineOwner {
    use crate::pipeline::PipelineBuilder;
    PipelineBuilder::new().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_constraints() {
        let constraints = TEST_CONSTRAINTS;
        assert_eq!(constraints.max_width, 800.0);
        assert_eq!(constraints.max_height, 600.0);
    }

    #[test]
    fn test_tight_constraints() {
        let constraints = tight_constraints();
        assert_eq!(constraints.min_width, 800.0);
        assert_eq!(constraints.max_width, 800.0);
    }

    #[test]
    fn test_sizes() {
        assert_eq!(sizes::SMALL.width, 100.0);
        assert_eq!(sizes::MEDIUM.width, 300.0);
        assert_eq!(sizes::LARGE.width, 800.0);
    }

    #[test]
    fn test_harness_builder() {
        let harness = TestHarnessBuilder::new().desktop().build();
        assert!(!harness.is_mounted());
    }
}
