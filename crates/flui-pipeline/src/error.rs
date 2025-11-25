//! Pipeline error types.

use flui_foundation::ElementId;
use thiserror::Error;

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Errors that can occur during pipeline execution.
#[derive(Debug, Error)]
pub enum PipelineError {
    /// Element not found in tree.
    #[error("Element not found: {0:?}")]
    ElementNotFound(ElementId),

    /// Element is not a render element.
    #[error("Element {0:?} is not a render element")]
    NotRenderElement(ElementId),

    /// Layout failed for element.
    #[error("Layout failed for element {0:?}: {1}")]
    LayoutFailed(ElementId, String),

    /// Paint failed for element.
    #[error("Paint failed for element {0:?}: {1}")]
    PaintFailed(ElementId, String),

    /// Build failed for element.
    #[error("Build failed for element {0:?}: {1}")]
    BuildFailed(ElementId, String),

    /// Cycle detected in tree.
    #[error("Cycle detected at element {0:?}")]
    CycleDetected(ElementId),

    /// Pipeline was cancelled.
    #[error("Pipeline cancelled")]
    Cancelled,

    /// Timeout during pipeline execution.
    #[error("Pipeline timeout after {0:?}")]
    Timeout(std::time::Duration),

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
        Self::LayoutFailed(id, msg.into())
    }

    /// Creates a new paint error.
    pub fn paint_failed(id: ElementId, msg: impl Into<String>) -> Self {
        Self::PaintFailed(id, msg.into())
    }

    /// Creates a new build error.
    pub fn build_failed(id: ElementId, msg: impl Into<String>) -> Self {
        Self::BuildFailed(id, msg.into())
    }
}
