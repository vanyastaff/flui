//! Pipeline error types.

use flui_foundation::ElementId;
use std::fmt;
use thiserror::Error;

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Pipeline execution phase.
///
/// Identifies which phase of the pipeline an error occurred in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelinePhase {
    /// Build phase - constructing the element tree.
    Build,
    /// Layout phase - computing sizes and positions.
    Layout,
    /// Paint phase - generating render commands.
    Paint,
    /// Composite phase - combining layers.
    Composite,
}

impl fmt::Display for PipelinePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build => write!(f, "Build"),
            Self::Layout => write!(f, "Layout"),
            Self::Paint => write!(f, "Paint"),
            Self::Composite => write!(f, "Composite"),
        }
    }
}

/// Errors that can occur during pipeline execution.
#[derive(Debug, Clone, Error)]
pub enum PipelineError {
    /// Element not found in tree.
    #[error("Element not found: {0:?}")]
    ElementNotFound(ElementId),

    /// Element is not a render element.
    #[error("Element {0:?} is not a render element")]
    NotRenderElement(ElementId),

    /// Layout failed for element.
    #[error("[{phase}] Layout failed for element {element:?}: {message}")]
    LayoutFailed {
        /// Element that failed.
        element: ElementId,
        /// Error message.
        message: String,
        /// Phase where error occurred.
        phase: PipelinePhase,
    },

    /// Paint failed for element.
    #[error("[{phase}] Paint failed for element {element:?}: {message}")]
    PaintFailed {
        /// Element that failed.
        element: ElementId,
        /// Error message.
        message: String,
        /// Phase where error occurred.
        phase: PipelinePhase,
    },

    /// Build failed for element.
    #[error("[{phase}] Build failed for element {element:?}: {message}")]
    BuildFailed {
        /// Element that failed.
        element: ElementId,
        /// Error message.
        message: String,
        /// Phase where error occurred.
        phase: PipelinePhase,
    },

    /// Cycle detected in tree.
    #[error("Cycle detected at element {0:?}")]
    CycleDetected(ElementId),

    /// Pipeline was cancelled.
    #[error("Pipeline cancelled")]
    Cancelled,

    /// Timeout during pipeline execution.
    #[error("Pipeline timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// Invalid state error.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Generic pipeline error.
    #[error("Pipeline error: {0}")]
    Other(String),
}

impl PipelineError {
    /// Creates a new "element not found" error.
    pub fn not_found(id: ElementId) -> Self {
        Self::ElementNotFound(id)
    }

    /// Creates a new "not render element" error.
    pub fn not_render(id: ElementId) -> Self {
        Self::NotRenderElement(id)
    }

    /// Creates a new layout error.
    pub fn layout_failed(id: ElementId, msg: impl Into<String>) -> Self {
        Self::LayoutFailed {
            element: id,
            message: msg.into(),
            phase: PipelinePhase::Layout,
        }
    }

    /// Creates a new layout error (with Result for compatibility).
    pub fn layout_error(id: ElementId, msg: impl Into<String>) -> Result<Self, &'static str> {
        Ok(Self::LayoutFailed {
            element: id,
            message: msg.into(),
            phase: PipelinePhase::Layout,
        })
    }

    /// Creates a new paint error.
    pub fn paint_failed(id: ElementId, msg: impl Into<String>) -> Self {
        Self::PaintFailed {
            element: id,
            message: msg.into(),
            phase: PipelinePhase::Paint,
        }
    }

    /// Creates a new paint error (with Result for compatibility).
    pub fn paint_error(id: ElementId, msg: impl Into<String>) -> Result<Self, &'static str> {
        Ok(Self::PaintFailed {
            element: id,
            message: msg.into(),
            phase: PipelinePhase::Paint,
        })
    }

    /// Creates a new build error.
    pub fn build_failed(id: ElementId, msg: impl Into<String>) -> Self {
        Self::BuildFailed {
            element: id,
            message: msg.into(),
            phase: PipelinePhase::Build,
        }
    }

    /// Creates a new invalid state error.
    pub fn invalid_state(msg: impl Into<String>) -> Result<Self, &'static str> {
        Ok(Self::InvalidState(msg.into()))
    }

    /// Get the phase where this error occurred.
    pub fn phase(&self) -> PipelinePhase {
        match self {
            Self::LayoutFailed { phase, .. } => *phase,
            Self::PaintFailed { phase, .. } => *phase,
            Self::BuildFailed { phase, .. } => *phase,
            // Default phases for other errors
            Self::ElementNotFound(_) | Self::NotRenderElement(_) | Self::CycleDetected(_) => {
                PipelinePhase::Build
            }
            Self::Cancelled | Self::Timeout(_) | Self::InvalidState(_) | Self::Other(_) => {
                PipelinePhase::Build
            }
        }
    }
}
