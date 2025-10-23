//! LeafRenderBox - Generic type for RenderObjects with no children

use super::{RenderState, RenderBoxMixin};

/// Generic RenderBox for widgets with no children
///
/// Examples: RenderErrorBox, RenderImage (when standalone)
///
/// # Type Parameter
///
/// - `T`: The data specific to this RenderObject type
#[derive(Debug)]
pub struct LeafRenderBox<T> {
    /// Shared state (size, constraints, flags)
    pub state: RenderState,

    /// Type-specific data
    pub data: T,
}

impl<T> LeafRenderBox<T> {
    /// Create a new LeafRenderBox
    pub fn new(data: T) -> Self {
        Self {
            state: RenderState::new(),
            data,
        }
    }

    /// Get immutable reference to data
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get mutable reference to data
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

impl<T> RenderBoxMixin for LeafRenderBox<T> {
    fn state(&self) -> &RenderState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut RenderState {
        &mut self.state
    }
}
