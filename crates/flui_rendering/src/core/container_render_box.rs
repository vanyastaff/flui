//! ContainerRenderBox - Generic type for RenderObjects with multiple children

use super::{RenderState, RenderBoxMixin};
use flui_core::BoxedRenderObject;

/// Generic RenderBox for widgets with multiple children
///
/// Examples: RenderFlex, RenderStack, RenderWrap
///
/// # Type Parameter
///
/// - `T`: The data specific to this RenderObject type
#[derive(Debug)]
pub struct ContainerRenderBox<T> {
    /// Shared state (size, constraints, flags)
    pub state: RenderState,

    /// Type-specific data
    pub data: T,

    /// The children
    pub children: Vec<BoxedRenderObject>,
}

impl<T> ContainerRenderBox<T> {
    /// Create a new ContainerRenderBox
    pub fn new(data: T) -> Self {
        Self {
            state: RenderState::new(),
            data,
            children: Vec::new(),
        }
    }

    /// Create with children
    pub fn with_children(data: T, children: Vec<BoxedRenderObject>) -> Self {
        Self {
            state: RenderState::new(),
            data,
            children,
        }
    }

    /// Get reference to children
    pub fn children(&self) -> &[BoxedRenderObject] {
        &self.children
    }

    /// Get mutable reference to children
    pub fn children_mut(&mut self) -> &mut Vec<BoxedRenderObject> {
        &mut self.children
    }

    /// Adopt a child (multi-child version)
    ///
    /// This is the generic implementation for all ContainerRenderBox types.
    /// It adds the child to the children vector.
    pub fn adopt_child(&mut self, child: BoxedRenderObject) {
        self.children.push(child);
        self.state_mut().mark_needs_layout();
    }
}

impl<T> RenderBoxMixin for ContainerRenderBox<T> {
    fn state(&self) -> &RenderState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut RenderState {
        &mut self.state
    }
}
