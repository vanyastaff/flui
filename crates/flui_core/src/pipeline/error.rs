//! Pipeline error types
//!
//! Defines error types for pipeline operations with detailed context
//! for debugging and recovery.
//!
//! # Type Safety
//!
//! This module uses smart constructors and validated types to prevent
//! invalid error states at compile time and runtime:
//!
//! - `TimeoutDuration`: Validated newtype ensuring elapsed >= deadline
//! - Smart constructors: Validate invariants before construction
//! - Private fields: Prevent direct construction of invalid states
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::{PipelineError, PipelinePhase};
//!
//! // ✅ Valid timeout (elapsed > deadline)
//! let error = PipelineError::timeout(
//!     PipelinePhase::Layout,
//!     20,  // elapsed
//!     16   // deadline
//! ).unwrap();
//!
//! // ❌ Invalid timeout (elapsed < deadline) - returns Err
//! let invalid = PipelineError::timeout(
//!     PipelinePhase::Layout,
//!     10,  // elapsed < deadline!
//!     16
//! );
//! assert!(invalid.is_err());
//! ```

use std::fmt;

// =========================================================================
// Validated Types
// =========================================================================

/// Validated timeout duration with guaranteed invariant: elapsed >= deadline
///
/// This newtype ensures that timeout durations are always valid by
/// construction, preventing nonsensical states like "timeout with elapsed < deadline".
///
/// # Invariants
///
/// 1. `elapsed_ms >= deadline_ms` - A timeout means we exceeded the deadline
/// 2. `deadline_ms > 0` - Zero or negative deadlines are meaningless
///
/// # Example
///
/// ```rust
/// use flui_core::pipeline::TimeoutDuration;
///
/// // ✅ Valid: elapsed (20ms) > deadline (16ms)
/// let timeout = TimeoutDuration::new(20, 16).unwrap();
/// assert_eq!(timeout.overage_ms(), 4);
///
/// // ❌ Invalid: elapsed < deadline
/// assert!(TimeoutDuration::new(10, 16).is_err());
///
/// // ❌ Invalid: zero deadline
/// assert!(TimeoutDuration::new(10, 0).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeoutDuration {
    elapsed_ms: u64,
    deadline_ms: u64,
}

impl TimeoutDuration {
    /// Create a new validated timeout duration
    ///
    /// # Parameters
    ///
    /// - `elapsed_ms`: Actual time elapsed (must be >= deadline_ms)
    /// - `deadline_ms`: Deadline that was exceeded (must be > 0)
    ///
    /// # Errors
    ///
    /// Returns `InvalidDuration` if:
    /// - `elapsed_ms < deadline_ms` (not actually a timeout!)
    /// - `deadline_ms == 0` (meaningless deadline)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TimeoutDuration;
    ///
    /// let valid = TimeoutDuration::new(20, 16);
    /// assert!(valid.is_ok());
    ///
    /// let invalid = TimeoutDuration::new(10, 16);
    /// assert!(invalid.is_err());
    /// ```
    pub fn new(elapsed_ms: u64, deadline_ms: u64) -> Result<Self, InvalidDuration> {
        if elapsed_ms < deadline_ms {
            return Err(InvalidDuration::NotExceeded {
                elapsed: elapsed_ms,
                deadline: deadline_ms,
            });
        }

        if deadline_ms == 0 {
            return Err(InvalidDuration::ZeroDeadline);
        }

        Ok(Self {
            elapsed_ms,
            deadline_ms,
        })
    }

    /// Get elapsed time in milliseconds
    #[inline]
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Get deadline in milliseconds
    #[inline]
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    /// Get how much we exceeded the deadline by (overage)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TimeoutDuration;
    ///
    /// let timeout = TimeoutDuration::new(20, 16).unwrap();
    /// assert_eq!(timeout.overage_ms(), 4); // 20 - 16 = 4ms over
    /// ```
    #[inline]
    pub fn overage_ms(&self) -> u64 {
        self.elapsed_ms - self.deadline_ms
    }

    /// Get overage as a percentage of deadline
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TimeoutDuration;
    ///
    /// let timeout = TimeoutDuration::new(20, 16).unwrap();
    /// assert_eq!(timeout.overage_percent(), 25.0); // 4ms / 16ms = 25%
    /// ```
    pub fn overage_percent(&self) -> f64 {
        (self.overage_ms() as f64 / self.deadline_ms as f64) * 100.0
    }
}

impl fmt::Display for TimeoutDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}ms elapsed (deadline: {}ms, +{}ms over)",
            self.elapsed_ms,
            self.deadline_ms,
            self.overage_ms()
        )
    }
}

// =========================================================================
// Validation Errors
// =========================================================================

/// Error type for invalid timeout duration construction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidDuration {
    /// Elapsed time is less than deadline (not a timeout!)
    NotExceeded {
        /// The elapsed time in milliseconds
        elapsed: u64,
        /// The deadline in milliseconds
        deadline: u64,
    },
    /// Deadline is zero (meaningless)
    ZeroDeadline,
}

impl fmt::Display for InvalidDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotExceeded { elapsed, deadline } => {
                write!(
                    f,
                    "Invalid timeout: elapsed ({}ms) < deadline ({}ms)",
                    elapsed, deadline
                )
            }
            Self::ZeroDeadline => {
                write!(f, "Invalid timeout: deadline must be > 0")
            }
        }
    }
}

impl std::error::Error for InvalidDuration {}

/// Error type for invalid error construction
#[derive(Debug, Clone)]
pub enum InvalidError {
    /// Invalid timeout duration
    InvalidDuration(InvalidDuration),
    /// Empty error message
    EmptyMessage,
    /// Invalid element ID
    InvalidElementId(crate::element::ElementId),
}

impl fmt::Display for InvalidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration(err) => write!(f, "{}", err),
            Self::EmptyMessage => write!(f, "Error message cannot be empty"),
            Self::InvalidElementId(id) => write!(f, "Invalid element ID: {:?}", id),
        }
    }
}

impl std::error::Error for InvalidError {}

impl From<InvalidDuration> for InvalidError {
    fn from(err: InvalidDuration) -> Self {
        Self::InvalidDuration(err)
    }
}

// =========================================================================
// Pipeline Error
// =========================================================================

/// Pipeline error type
///
/// Represents errors that can occur during pipeline operations.
///
/// # Type Safety
///
/// This type uses validated construction to prevent invalid states:
/// - Timeout errors must have elapsed >= deadline (enforced by `TimeoutDuration`)
/// - Error messages cannot be empty
/// - All construction goes through smart constructors
///
/// # Error Handling Strategy
///
/// Errors are categorized by severity:
/// - **Recoverable**: Can continue with fallback (layout timeout, paint error)
/// - **Fatal**: Cannot continue (tree corruption, invalid state)
///
/// # Example
///
/// ```rust
/// use flui_core::pipeline::{PipelineError, PipelinePhase};
///
/// // ✅ Use smart constructors
/// let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
///
/// fn handle_error(error: PipelineError) {
///     match error {
///         PipelineError::Timeout { .. } => {
///             // Use last good frame
///         }
///         PipelineError::LayoutError { .. } => {
///             // Show error widget
///         }
///         _ => {
///             // Fatal error - panic in development
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum PipelineError {
    /// Operation timed out
    ///
    /// Occurs when an operation exceeds its deadline.
    /// Typically recoverable by using last good frame.
    ///
    /// # Invariants
    ///
    /// - `duration.elapsed_ms() >= duration.deadline_ms()` (guaranteed by TimeoutDuration)
    /// - `duration.deadline_ms() > 0` (guaranteed by TimeoutDuration)
    Timeout {
        /// Pipeline phase that timed out
        phase: PipelinePhase,
        /// Validated timeout duration
        duration: TimeoutDuration,
    },

    /// Layout error
    ///
    /// Occurs during layout phase (size computation, constraint violation).
    /// Recoverable by skipping frame or showing error widget.
    LayoutError {
        /// Element that caused the layout error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Paint error
    ///
    /// Occurs during paint phase (layer generation failure).
    /// Recoverable by using last good frame.
    PaintError {
        /// Element that caused the paint error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Build error
    ///
    /// Occurs during build phase (widget rebuild failure).
    /// Recoverable by showing error widget.
    BuildError {
        /// Element that caused the build error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Tree corruption
    ///
    /// Fatal error - element tree is in invalid state.
    /// Not recoverable - must panic in development.
    TreeCorruption {
        /// Description of the tree corruption
        message: String,
    },

    /// Invalid state
    ///
    /// Fatal error - pipeline is in invalid state.
    /// Not recoverable - must panic in development.
    InvalidState {
        /// Description of the invalid state
        message: String,
    },
}

impl PipelineError {
    /// Check if error is recoverable
    ///
    /// Returns `true` for errors that can be handled gracefully
    /// (timeout, layout error, paint error, build error).
    ///
    /// Returns `false` for fatal errors
    /// (tree corruption, invalid state).
    #[inline]
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Timeout { .. } => true,
            Self::LayoutError { .. } => true,
            Self::PaintError { .. } => true,
            Self::BuildError { .. } => true,
            Self::TreeCorruption { .. } => false,
            Self::InvalidState { .. } => false,
        }
    }

    /// Get error phase
    ///
    /// Returns the pipeline phase where the error occurred.
    #[inline]
    pub fn phase(&self) -> PipelinePhase {
        match self {
            Self::Timeout { phase, .. } => *phase,
            Self::LayoutError { .. } => PipelinePhase::Layout,
            Self::PaintError { .. } => PipelinePhase::Paint,
            Self::BuildError { .. } => PipelinePhase::Build,
            Self::TreeCorruption { .. } => PipelinePhase::Build,
            Self::InvalidState { .. } => PipelinePhase::Build,
        }
    }

    // =========================================================================
    // Smart Constructors (Validated)
    // =========================================================================

    /// Create a timeout error with validation
    ///
    /// # Parameters
    ///
    /// - `phase`: Pipeline phase that timed out
    /// - `elapsed_ms`: Actual elapsed time (must be >= deadline_ms)
    /// - `deadline_ms`: Deadline that was exceeded (must be > 0)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if:
    /// - `elapsed_ms < deadline_ms` (not a timeout!)
    /// - `deadline_ms == 0` (meaningless deadline)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::{PipelineError, PipelinePhase};
    ///
    /// // ✅ Valid timeout
    /// let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
    ///
    /// // ❌ Invalid: elapsed < deadline
    /// assert!(PipelineError::timeout(PipelinePhase::Layout, 10, 16).is_err());
    /// ```
    pub fn timeout(
        phase: PipelinePhase,
        elapsed_ms: u64,
        deadline_ms: u64,
    ) -> Result<Self, InvalidError> {
        let duration = TimeoutDuration::new(elapsed_ms, deadline_ms)?;
        Ok(Self::Timeout { phase, duration })
    }

    /// Create a layout error with validation
    ///
    /// # Parameters
    ///
    /// - `element_id`: Element that caused the error
    /// - `message`: Error description (cannot be empty)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if message is empty.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineError;
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new(42);
    /// let error = PipelineError::layout_error(id, "Constraint violation").unwrap();
    ///
    /// // ❌ Invalid: empty message
    /// assert!(PipelineError::layout_error(id, "").is_err());
    /// ```
    pub fn layout_error(
        element_id: crate::element::ElementId,
        message: impl Into<String>,
    ) -> Result<Self, InvalidError> {
        let message = message.into();
        if message.is_empty() {
            return Err(InvalidError::EmptyMessage);
        }
        Ok(Self::LayoutError {
            element_id,
            message,
        })
    }

    /// Create a paint error with validation
    ///
    /// # Parameters
    ///
    /// - `element_id`: Element that caused the error
    /// - `message`: Error description (cannot be empty)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if message is empty.
    pub fn paint_error(
        element_id: crate::element::ElementId,
        message: impl Into<String>,
    ) -> Result<Self, InvalidError> {
        let message = message.into();
        if message.is_empty() {
            return Err(InvalidError::EmptyMessage);
        }
        Ok(Self::PaintError {
            element_id,
            message,
        })
    }

    /// Create a build error with validation
    ///
    /// # Parameters
    ///
    /// - `element_id`: Element that caused the error
    /// - `message`: Error description (cannot be empty)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if message is empty.
    pub fn build_error(
        element_id: crate::element::ElementId,
        message: impl Into<String>,
    ) -> Result<Self, InvalidError> {
        let message = message.into();
        if message.is_empty() {
            return Err(InvalidError::EmptyMessage);
        }
        Ok(Self::BuildError {
            element_id,
            message,
        })
    }

    /// Create a tree corruption error with validation
    ///
    /// # Parameters
    ///
    /// - `message`: Description of corruption (cannot be empty)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if message is empty.
    pub fn tree_corruption(message: impl Into<String>) -> Result<Self, InvalidError> {
        let message = message.into();
        if message.is_empty() {
            return Err(InvalidError::EmptyMessage);
        }
        Ok(Self::TreeCorruption { message })
    }

    /// Create an invalid state error with validation
    ///
    /// # Parameters
    ///
    /// - `message`: Description of invalid state (cannot be empty)
    ///
    /// # Errors
    ///
    /// Returns `InvalidError` if message is empty.
    pub fn invalid_state(message: impl Into<String>) -> Result<Self, InvalidError> {
        let message = message.into();
        if message.is_empty() {
            return Err(InvalidError::EmptyMessage);
        }
        Ok(Self::InvalidState { message })
    }

    // =========================================================================
    // Accessors for Timeout variant
    // =========================================================================

    /// Get timeout duration (if this is a Timeout error)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::{PipelineError, PipelinePhase};
    ///
    /// let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
    /// let duration = error.timeout_duration().unwrap();
    /// assert_eq!(duration.overage_ms(), 4);
    /// ```
    pub fn timeout_duration(&self) -> Option<TimeoutDuration> {
        match self {
            Self::Timeout { duration, .. } => Some(*duration),
            _ => None,
        }
    }

    /// Get elapsed time (if this is a Timeout error)
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.timeout_duration().map(|d| d.elapsed_ms())
    }

    /// Get deadline (if this is a Timeout error)
    pub fn deadline_ms(&self) -> Option<u64> {
        self.timeout_duration().map(|d| d.deadline_ms())
    }

    /// Get timeout overage (if this is a Timeout error)
    pub fn overage_ms(&self) -> Option<u64> {
        self.timeout_duration().map(|d| d.overage_ms())
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { phase, duration } => {
                write!(f, "Pipeline timeout in {} phase: {}", phase, duration)
            }
            Self::LayoutError {
                element_id,
                message,
            } => {
                write!(f, "Layout error for element {:?}: {}", element_id, message)
            }
            Self::PaintError {
                element_id,
                message,
            } => {
                write!(f, "Paint error for element {:?}: {}", element_id, message)
            }
            Self::BuildError {
                element_id,
                message,
            } => {
                write!(f, "Build error for element {:?}: {}", element_id, message)
            }
            Self::TreeCorruption { message } => {
                write!(f, "Element tree corruption: {}", message)
            }
            Self::InvalidState { message } => {
                write!(f, "Invalid pipeline state: {}", message)
            }
        }
    }
}

impl std::error::Error for PipelineError {}

// =============================================================================
// Conversion to flui_pipeline::PipelineError
// =============================================================================

impl From<PipelineError> for flui_pipeline::PipelineError {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::Timeout { phase, duration } => {
                flui_pipeline::PipelineError::Cancelled(format!(
                    "Timeout in {:?} phase: {}ms elapsed (deadline: {}ms)",
                    phase,
                    duration.elapsed_ms(),
                    duration.deadline_ms()
                ))
            }
            PipelineError::LayoutError {
                element_id,
                message,
            } => flui_pipeline::PipelineError::LayoutFailed {
                element_id: flui_foundation::ElementId::new(element_id.get()),
                message,
            },
            PipelineError::PaintError {
                element_id,
                message,
            } => flui_pipeline::PipelineError::PaintFailed {
                element_id: flui_foundation::ElementId::new(element_id.get()),
                message,
            },
            PipelineError::BuildError {
                element_id,
                message,
            } => flui_pipeline::PipelineError::BuildFailed {
                element_id: flui_foundation::ElementId::new(element_id.get()),
                message,
            },
            PipelineError::TreeCorruption { message } => {
                flui_pipeline::PipelineError::InvalidState(format!("Tree corruption: {}", message))
            }
            PipelineError::InvalidState { message } => {
                flui_pipeline::PipelineError::InvalidState(message)
            }
        }
    }
}

/// Pipeline phase identifier
///
/// Identifies which phase of the pipeline an error occurred in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelinePhase {
    /// Build phase (widget rebuild)
    Build,
    /// Layout phase (size computation)
    Layout,
    /// Paint phase (layer generation)
    Paint,
}

impl fmt::Display for PipelinePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build => write!(f, "Build"),
            Self::Layout => write!(f, "Layout"),
            Self::Paint => write!(f, "Paint"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_is_recoverable() {
        let timeout = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
        assert!(timeout.is_recoverable());

        let corruption = PipelineError::tree_corruption("Invalid element reference").unwrap();
        assert!(!corruption.is_recoverable());
    }

    #[test]
    fn test_error_phase() {
        use crate::ElementId;
        let layout_error =
            PipelineError::layout_error(ElementId::new(42), "Constraint violation").unwrap();
        assert_eq!(layout_error.phase(), PipelinePhase::Layout);

        let paint_error =
            PipelineError::paint_error(ElementId::new(42), "Layer creation failed").unwrap();
        assert_eq!(paint_error.phase(), PipelinePhase::Paint);
    }

    #[test]
    fn test_error_display() {
        let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
        let display = format!("{}", error);
        assert!(display.contains("timeout"));
        assert!(display.contains("Layout"));
    }

    // =========================================================================
    // Validation Tests for TimeoutDuration
    // =========================================================================

    #[test]
    fn test_timeout_duration_valid() {
        // Valid: elapsed > deadline
        let duration = TimeoutDuration::new(20, 16).unwrap();
        assert_eq!(duration.elapsed_ms(), 20);
        assert_eq!(duration.deadline_ms(), 16);
        assert_eq!(duration.overage_ms(), 4);
        assert_eq!(duration.overage_percent(), 25.0);

        // Valid: elapsed == deadline
        let duration = TimeoutDuration::new(16, 16).unwrap();
        assert_eq!(duration.elapsed_ms(), 16);
        assert_eq!(duration.deadline_ms(), 16);
        assert_eq!(duration.overage_ms(), 0);
        assert_eq!(duration.overage_percent(), 0.0);
    }

    #[test]
    fn test_timeout_duration_invalid_not_exceeded() {
        // Invalid: elapsed < deadline
        let result = TimeoutDuration::new(10, 16);
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidDuration::NotExceeded { elapsed, deadline } => {
                assert_eq!(elapsed, 10);
                assert_eq!(deadline, 16);
            }
            _ => panic!("Expected NotExceeded error"),
        }
    }

    #[test]
    fn test_timeout_duration_invalid_zero_deadline() {
        // Invalid: zero deadline
        let result = TimeoutDuration::new(10, 0);
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidDuration::ZeroDeadline => {}
            _ => panic!("Expected ZeroDeadline error"),
        }
    }

    #[test]
    fn test_timeout_duration_display() {
        let duration = TimeoutDuration::new(20, 16).unwrap();
        let display = format!("{}", duration);
        assert!(display.contains("20ms"));
        assert!(display.contains("16ms"));
        assert!(display.contains("4ms"));
    }

    // =========================================================================
    // Validation Tests for PipelineError Smart Constructors
    // =========================================================================

    #[test]
    fn test_timeout_constructor_valid() {
        let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();
        match error {
            PipelineError::Timeout { phase, duration } => {
                assert_eq!(phase, PipelinePhase::Layout);
                assert_eq!(duration.elapsed_ms(), 20);
                assert_eq!(duration.deadline_ms(), 16);
            }
            _ => panic!("Expected Timeout variant"),
        }
    }

    #[test]
    fn test_timeout_constructor_invalid() {
        // elapsed < deadline
        let result = PipelineError::timeout(PipelinePhase::Layout, 10, 16);
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::InvalidDuration(InvalidDuration::NotExceeded { .. }) => {}
            _ => panic!("Expected InvalidDuration(NotExceeded) error"),
        }

        // zero deadline
        let result = PipelineError::timeout(PipelinePhase::Layout, 10, 0);
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::InvalidDuration(InvalidDuration::ZeroDeadline) => {}
            _ => panic!("Expected InvalidDuration(ZeroDeadline) error"),
        }
    }

    #[test]
    fn test_layout_error_constructor_valid() {
        use crate::ElementId;
        let error =
            PipelineError::layout_error(ElementId::new(42), "Constraint violation").unwrap();
        match error {
            PipelineError::LayoutError {
                element_id,
                message,
            } => {
                assert_eq!(element_id, ElementId::new(42));
                assert_eq!(message, "Constraint violation");
            }
            _ => panic!("Expected LayoutError variant"),
        }
    }

    #[test]
    fn test_layout_error_constructor_empty_message() {
        use crate::ElementId;
        let result = PipelineError::layout_error(ElementId::new(42), "");
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::EmptyMessage => {}
            _ => panic!("Expected EmptyMessage error"),
        }
    }

    #[test]
    fn test_paint_error_constructor_valid() {
        use crate::ElementId;
        let error =
            PipelineError::paint_error(ElementId::new(42), "Layer creation failed").unwrap();
        match error {
            PipelineError::PaintError {
                element_id,
                message,
            } => {
                assert_eq!(element_id, ElementId::new(42));
                assert_eq!(message, "Layer creation failed");
            }
            _ => panic!("Expected PaintError variant"),
        }
    }

    #[test]
    fn test_paint_error_constructor_empty_message() {
        use crate::ElementId;
        let result = PipelineError::paint_error(ElementId::new(42), "");
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::EmptyMessage => {}
            _ => panic!("Expected EmptyMessage error"),
        }
    }

    #[test]
    fn test_build_error_constructor_valid() {
        use crate::ElementId;
        let error =
            PipelineError::build_error(ElementId::new(42), "Widget rebuild failed").unwrap();
        match error {
            PipelineError::BuildError {
                element_id,
                message,
            } => {
                assert_eq!(element_id, ElementId::new(42));
                assert_eq!(message, "Widget rebuild failed");
            }
            _ => panic!("Expected BuildError variant"),
        }
    }

    #[test]
    fn test_build_error_constructor_empty_message() {
        use crate::ElementId;
        let result = PipelineError::build_error(ElementId::new(42), "");
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::EmptyMessage => {}
            _ => panic!("Expected EmptyMessage error"),
        }
    }

    #[test]
    fn test_tree_corruption_constructor_valid() {
        let error = PipelineError::tree_corruption("Invalid element reference").unwrap();
        match error {
            PipelineError::TreeCorruption { message } => {
                assert_eq!(message, "Invalid element reference");
            }
            _ => panic!("Expected TreeCorruption variant"),
        }
    }

    #[test]
    fn test_tree_corruption_constructor_empty_message() {
        let result = PipelineError::tree_corruption("");
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::EmptyMessage => {}
            _ => panic!("Expected EmptyMessage error"),
        }
    }

    #[test]
    fn test_invalid_state_constructor_valid() {
        let error = PipelineError::invalid_state("Inconsistent pipeline state").unwrap();
        match error {
            PipelineError::InvalidState { message } => {
                assert_eq!(message, "Inconsistent pipeline state");
            }
            _ => panic!("Expected InvalidState variant"),
        }
    }

    #[test]
    fn test_invalid_state_constructor_empty_message() {
        let result = PipelineError::invalid_state("");
        assert!(result.is_err());
        match result.unwrap_err() {
            InvalidError::EmptyMessage => {}
            _ => panic!("Expected EmptyMessage error"),
        }
    }

    // =========================================================================
    // Tests for Timeout Accessors
    // =========================================================================

    #[test]
    fn test_timeout_accessors() {
        let error = PipelineError::timeout(PipelinePhase::Layout, 20, 16).unwrap();

        assert!(error.timeout_duration().is_some());
        assert_eq!(error.elapsed_ms(), Some(20));
        assert_eq!(error.deadline_ms(), Some(16));
        assert_eq!(error.overage_ms(), Some(4));
    }

    #[test]
    fn test_timeout_accessors_non_timeout_error() {
        use crate::ElementId;
        let error = PipelineError::layout_error(ElementId::new(42), "Test").unwrap();

        assert!(error.timeout_duration().is_none());
        assert_eq!(error.elapsed_ms(), None);
        assert_eq!(error.deadline_ms(), None);
        assert_eq!(error.overage_ms(), None);
    }
}
