//! Error types for the rendering system
//!
//! Provides structured error handling instead of panics for recoverable errors.

use crate::RenderId;
use std::fmt;

/// Result type for rendering operations
pub type RenderResult<T> = Result<T, RenderError>;

/// Errors that can occur during rendering operations
#[derive(Debug, Clone)]
pub enum RenderError {
    /// Layout operation failed
    LayoutFailed {
        /// Name of the render object type
        render_object: &'static str,
        /// Reason for the failure
        reason: String,
    },

    /// Paint operation failed
    PaintFailed {
        /// Name of the render object type
        render_object: &'static str,
        /// Reason for the failure
        reason: String,
    },

    /// Layout constraints could not be satisfied
    ConstraintViolation {
        /// Description of what was expected
        expected: String,
        /// Description of what was actually received
        actual: String,
    },

    /// A child's layout failed
    ChildLayoutFailed {
        /// ID of the child that failed
        child_id: RenderId,
        /// The underlying error message
        message: String,
    },

    /// The render object is in an invalid state for the requested operation
    InvalidState {
        /// Description of the invalid state
        message: String,
    },

    /// The requested node was not found in the tree
    NodeNotFound {
        /// ID of the missing node
        id: RenderId,
    },

    /// Element was not found
    ElementNotFound {
        /// Description
        message: String,
    },

    /// Element is not a render element
    NotARenderElement {
        /// Description
        message: String,
    },

    /// Protocol not supported
    UnsupportedProtocol {
        /// Expected protocol
        expected: &'static str,
        /// Actual protocol (if known)
        actual: String,
    },

    /// Parent data type mismatch
    ParentDataMismatch {
        /// Expected type name
        expected: &'static str,
        /// Actual type name (if known)
        actual: String,
    },

    /// A layer operation failed
    LayerError {
        /// Description of the layer error
        message: String,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayoutFailed {
                render_object,
                reason,
            } => {
                write!(f, "Layout failed for {}: {}", render_object, reason)
            }
            Self::PaintFailed {
                render_object,
                reason,
            } => {
                write!(f, "Paint failed for {}: {}", render_object, reason)
            }
            Self::ConstraintViolation { expected, actual } => {
                write!(
                    f,
                    "Constraint violation: expected {}, got {}",
                    expected, actual
                )
            }
            Self::ChildLayoutFailed { child_id, message } => {
                write!(f, "Child layout failed (id={:?}): {}", child_id, message)
            }
            Self::InvalidState { message } => {
                write!(f, "Invalid render state: {}", message)
            }
            Self::NodeNotFound { id } => {
                write!(f, "Render node not found: {:?}", id)
            }
            Self::ElementNotFound { message } => {
                write!(f, "Element not found: {}", message)
            }
            Self::NotARenderElement { message } => {
                write!(f, "Not a render element: {}", message)
            }
            Self::UnsupportedProtocol { expected, actual } => {
                write!(
                    f,
                    "Unsupported protocol: expected {}, got {}",
                    expected, actual
                )
            }
            Self::ParentDataMismatch { expected, actual } => {
                write!(
                    f,
                    "Parent data type mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            Self::LayerError { message } => {
                write!(f, "Layer error: {}", message)
            }
        }
    }
}

impl std::error::Error for RenderError {}

impl RenderError {
    /// Create a layout failed error
    pub fn layout_failed(render_object: &'static str, reason: impl Into<String>) -> Self {
        Self::LayoutFailed {
            render_object,
            reason: reason.into(),
        }
    }

    /// Create a paint failed error
    pub fn paint_failed(render_object: &'static str, reason: impl Into<String>) -> Self {
        Self::PaintFailed {
            render_object,
            reason: reason.into(),
        }
    }

    /// Create a constraint violation error
    pub fn constraint_violation(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create a child layout failed error
    pub fn child_layout_failed(child_id: RenderId, message: impl Into<String>) -> Self {
        Self::ChildLayoutFailed {
            child_id,
            message: message.into(),
        }
    }

    /// Create an invalid state error
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState {
            message: message.into(),
        }
    }

    /// Create a node not found error
    pub fn node_not_found(id: RenderId) -> Self {
        Self::NodeNotFound { id }
    }

    /// Create a parent data mismatch error
    pub fn parent_data_mismatch(expected: &'static str, actual: impl Into<String>) -> Self {
        Self::ParentDataMismatch {
            expected,
            actual: actual.into(),
        }
    }

    /// Create a layer error
    pub fn layer_error(message: impl Into<String>) -> Self {
        Self::LayerError {
            message: message.into(),
        }
    }
}
