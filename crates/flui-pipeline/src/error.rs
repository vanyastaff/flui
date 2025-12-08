//! Pipeline error types
//!
//! Provides error types for all pipeline phases (build, layout, paint).
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::{PipelineError, PipelinePhase, PipelineResult};
//!
//! fn do_layout(id: usize) -> PipelineResult<()> {
//!     // ... layout logic ...
//!     Err(PipelineError::layout_failed(id, "invalid constraints"))
//! }
//! ```

use std::fmt;
use thiserror::Error;

/// Result type for pipeline operations
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Pipeline phase identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PipelinePhase {
    /// Build phase - widget tree construction
    #[default]
    Build,
    /// Layout phase - size computation
    Layout,
    /// Paint phase - layer generation
    Paint,
}

impl PipelinePhase {
    /// Returns the phase name as a static string.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Layout => "layout",
            Self::Paint => "paint",
        }
    }

    /// Returns `true` if this is the build phase.
    #[inline]
    #[must_use]
    pub const fn is_build(self) -> bool {
        matches!(self, Self::Build)
    }

    /// Returns `true` if this is the layout phase.
    #[inline]
    #[must_use]
    pub const fn is_layout(self) -> bool {
        matches!(self, Self::Layout)
    }

    /// Returns `true` if this is the paint phase.
    #[inline]
    #[must_use]
    pub const fn is_paint(self) -> bool {
        matches!(self, Self::Paint)
    }
}

impl fmt::Display for PipelinePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Pipeline error
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum PipelineError {
    /// Build phase failed for an element
    #[error("Build failed for element {element_id}: {message}")]
    BuildFailed {
        /// Element that failed to build
        element_id: usize,
        /// Error message
        message: String,
    },

    /// Layout phase failed for an element
    #[error("Layout failed for element {element_id}: {message}")]
    LayoutFailed {
        /// Element that failed layout
        element_id: usize,
        /// Error message
        message: String,
    },

    /// Paint phase failed for an element
    #[error("Paint failed for element {element_id}: {message}")]
    PaintFailed {
        /// Element that failed paint
        element_id: usize,
        /// Error message
        message: String,
    },

    /// Element not found in tree
    #[error("Element not found: {0}")]
    ElementNotFound(usize),

    /// Invalid state detected
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Operation cancelled (timeout or manual)
    #[error("Operation cancelled: {0}")]
    Cancelled(String),

    /// No root element attached
    #[error("No root element attached")]
    NoRoot,

    /// Root already attached
    #[error("Root element has already been attached")]
    RootAlreadyAttached,

    /// Constraint violation
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
}

impl PipelineError {
    /// Create build failed error
    #[must_use]
    pub fn build_failed(element_id: usize, message: impl Into<String>) -> Self {
        Self::BuildFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create layout failed error
    #[must_use]
    pub fn layout_failed(element_id: usize, message: impl Into<String>) -> Self {
        Self::LayoutFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create paint failed error
    #[must_use]
    pub fn paint_failed(element_id: usize, message: impl Into<String>) -> Self {
        Self::PaintFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create element not found error
    #[must_use]
    pub const fn element_not_found(element_id: usize) -> Self {
        Self::ElementNotFound(element_id)
    }

    /// Create invalid state error
    #[must_use]
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }

    /// Create cancelled error
    #[must_use]
    pub fn cancelled(reason: impl Into<String>) -> Self {
        Self::Cancelled(reason.into())
    }

    /// Get the pipeline phase this error occurred in
    #[must_use]
    pub const fn phase(&self) -> PipelinePhase {
        match self {
            Self::BuildFailed { .. } => PipelinePhase::Build,
            Self::LayoutFailed { .. } | Self::ConstraintViolation(_) => PipelinePhase::Layout,
            Self::PaintFailed { .. } => PipelinePhase::Paint,
            // Default to build for generic errors
            Self::ElementNotFound(_)
            | Self::InvalidState(_)
            | Self::Cancelled(_)
            | Self::NoRoot
            | Self::RootAlreadyAttached => PipelinePhase::Build,
        }
    }

    /// Get element ID if error is element-specific
    #[must_use]
    pub const fn element_id(&self) -> Option<usize> {
        match self {
            Self::BuildFailed { element_id, .. }
            | Self::LayoutFailed { element_id, .. }
            | Self::PaintFailed { element_id, .. }
            | Self::ElementNotFound(element_id) => Some(*element_id),
            _ => None,
        }
    }

    /// Returns `true` if the error is recoverable.
    ///
    /// Recoverable errors can be handled without terminating the pipeline.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ElementNotFound(_) | Self::Cancelled(_) | Self::ConstraintViolation(_)
        )
    }

    /// Returns `true` if this is a build phase error.
    #[must_use]
    pub const fn is_build_error(&self) -> bool {
        self.phase().is_build()
    }

    /// Returns `true` if this is a layout phase error.
    #[must_use]
    pub const fn is_layout_error(&self) -> bool {
        self.phase().is_layout()
    }

    /// Returns `true` if this is a paint phase error.
    #[must_use]
    pub const fn is_paint_error(&self) -> bool {
        self.phase().is_paint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_failed() {
        let id = 42usize;
        let err = PipelineError::build_failed(id, "test error");

        assert_eq!(err.phase(), PipelinePhase::Build);
        assert_eq!(err.element_id(), Some(id));
        assert!(err.to_string().contains("42"));
        assert!(err.to_string().contains("test error"));
        assert!(err.is_build_error());
    }

    #[test]
    fn test_layout_failed() {
        let id = 10usize;
        let err = PipelineError::layout_failed(id, "bad constraints");

        assert_eq!(err.phase(), PipelinePhase::Layout);
        assert_eq!(err.element_id(), Some(id));
        assert!(err.is_layout_error());
    }

    #[test]
    fn test_paint_failed() {
        let id = 5usize;
        let err = PipelineError::paint_failed(id, "canvas error");

        assert_eq!(err.phase(), PipelinePhase::Paint);
        assert_eq!(err.element_id(), Some(id));
        assert!(err.is_paint_error());
    }

    #[test]
    fn test_phase_display() {
        assert_eq!(format!("{}", PipelinePhase::Build), "build");
        assert_eq!(format!("{}", PipelinePhase::Layout), "layout");
        assert_eq!(format!("{}", PipelinePhase::Paint), "paint");
    }

    #[test]
    fn test_phase_as_str() {
        assert_eq!(PipelinePhase::Build.as_str(), "build");
        assert_eq!(PipelinePhase::Layout.as_str(), "layout");
        assert_eq!(PipelinePhase::Paint.as_str(), "paint");
    }

    #[test]
    fn test_is_recoverable() {
        assert!(PipelineError::element_not_found(1).is_recoverable());
        assert!(PipelineError::cancelled("timeout").is_recoverable());
        assert!(!PipelineError::NoRoot.is_recoverable());
        assert!(!PipelineError::build_failed(1, "error").is_recoverable());
    }

    #[test]
    fn test_phase_predicates() {
        assert!(PipelinePhase::Build.is_build());
        assert!(!PipelinePhase::Build.is_layout());
        assert!(!PipelinePhase::Build.is_paint());

        assert!(!PipelinePhase::Layout.is_build());
        assert!(PipelinePhase::Layout.is_layout());
        assert!(!PipelinePhase::Layout.is_paint());

        assert!(!PipelinePhase::Paint.is_build());
        assert!(!PipelinePhase::Paint.is_layout());
        assert!(PipelinePhase::Paint.is_paint());
    }
}
