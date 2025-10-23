//! SingleRenderBox - Generic type for RenderObjects with one child

use super::{RenderState, RenderBoxMixin};
use flui_core::BoxedRenderObject;

/// Generic RenderBox for widgets with one child
///
/// Examples: RenderPadding, RenderOpacity, RenderTransform
///
/// # Type Parameter
///
/// - `T`: The data specific to this RenderObject type
#[derive(Debug)]
pub struct SingleRenderBox<T> {
    /// Shared state (size, constraints, flags)
    pub state: RenderState,

    /// Type-specific data
    pub data: T,

    /// The single child
    pub child: Option<BoxedRenderObject>,
}

impl<T> SingleRenderBox<T> {
    /// Create a new SingleRenderBox
    pub fn new(data: T) -> Self {
        Self {
            state: RenderState::new(),
            data,
            child: None,
        }
    }

    /// Create with a child
    pub fn with_child(data: T, child: BoxedRenderObject) -> Self {
        Self {
            state: RenderState::new(),
            data,
            child: Some(child),
        }
    }

    /// Get reference to child
    pub fn child(&self) -> Option<&BoxedRenderObject> {
        self.child.as_ref()
    }

    /// Get mutable reference to child
    pub fn child_mut(&mut self) -> Option<&mut BoxedRenderObject> {
        self.child.as_mut()
    }

    /// Get reference to type-specific data
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get mutable reference to type-specific data
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Adopt a child (single-child version)
    ///
    /// This is the generic implementation for all SingleRenderBox types.
    /// It replaces the existing child with the new one.
    pub fn adopt_child(&mut self, child: BoxedRenderObject) {
        self.child = Some(child);
        self.state_mut().mark_needs_layout();
    }
}

impl<T> RenderBoxMixin for SingleRenderBox<T> {
    fn state(&self) -> &RenderState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut RenderState {
        &mut self.state
    }
}

