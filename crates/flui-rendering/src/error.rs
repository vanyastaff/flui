//! Error types for the rendering system.
//!
//! This module provides a unified error type for render operations,
//! replacing panics with recoverable errors where appropriate.

use flui_foundation::RenderId;
use thiserror::Error;

/// Errors that can occur during rendering operations.
///
/// Cycle 4: `#[non_exhaustive]` future-compat marker. Matches the
/// workspace 2026 quality bar — public error enums in foundation /
/// tree / painting / engine all carry the attribute (cycle 3 I-11
/// added it on `DiagnosticLevel` / `DiagnosticsTreeStyle`).
///
/// Cycle 4 R-17: the 5 message-carrying variants
/// (`InvalidConstraints`, `RelayoutBoundaryViolation`, `LayerError`,
/// `CompositingError`, `SemanticsError`) store `Box<str>` rather
/// than `String`. `Box<str>` is a 16-byte fat pointer (vs `String`'s
/// 24-byte `Vec<u8>` header) and never wastes capacity on the heap
/// — error messages are written-once / read-rarely, so the `Vec`
/// growth amortisation `String` provides has no value here. Same
/// pattern as cycle 3 PR #106 (TreeError::Internal). Constructors
/// accept `impl Into<Box<str>>` which covers `&str`, `String`, and
/// `Box<str>` callers unchanged.
// Cycle 4 R-25: dropped `Clone` derive. Workspace grep
// (`rg 'RenderError.*clone\(\)'`) returned zero consumers of
// `.clone()` on RenderError; errors are terminal values that
// propagate through `?` or `Result::map_err`, neither of which
// requires Clone. Removing the derive matches the canonical Rust
// error idiom (*Programming Rust* 2nd ed §7 "Error Handling":
// errors are throwaways, not duplicates). If a future caller needs
// to fan out a RenderError to multiple consumers, wrap in `Arc`
// at that callsite -- cheap, explicit, and avoids the
// implicit-deep-copy footgun.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RenderError {
    // ========================================================================
    // Attachment Errors
    // ========================================================================
    /// Render object is not attached to a pipeline owner.
    #[error("render object is not attached to a pipeline owner")]
    NotAttached,

    /// Render object is already attached to a pipeline owner.
    #[error("render object is already attached to a pipeline owner")]
    AlreadyAttached,

    /// View configuration not set before use.
    #[error("view configuration not set")]
    ConfigurationNotSet,

    // ========================================================================
    // Tree Errors
    // ========================================================================
    /// Render node not found in tree.
    #[error("render node not found: {0:?}")]
    NodeNotFound(RenderId),

    /// Invalid parent-child relationship.
    #[error("invalid parent-child relationship")]
    InvalidParentChild,

    /// Cycle detected in render tree.
    #[error("cycle detected in render tree")]
    CycleDetected,

    // ========================================================================
    // Layout Errors
    // ========================================================================
    /// Invalid constraints provided to layout.
    #[error("invalid constraints: {message}")]
    InvalidConstraints {
        /// Description of the constraint violation.
        message: Box<str>,
    },

    /// Layout performed during paint phase.
    #[error("layout cannot be performed during paint phase")]
    LayoutDuringPaint,

    /// Layout performed on detached node.
    #[error("cannot layout detached render object")]
    LayoutDetached,

    /// Relayout boundary violation.
    #[error("relayout boundary violated: {message}")]
    RelayoutBoundaryViolation {
        /// Description of the violation.
        message: Box<str>,
    },

    // ========================================================================
    // Paint Errors
    // ========================================================================
    /// Paint performed before layout.
    #[error("paint performed before layout")]
    PaintBeforeLayout,

    /// Paint performed on detached node.
    #[error("cannot paint detached render object")]
    PaintDetached,

    /// Layer operation failed.
    #[error("layer operation failed: {message}")]
    LayerError {
        /// Description of the layer error.
        message: Box<str>,
    },

    // ========================================================================
    // Pipeline Errors
    // ========================================================================
    /// Pipeline phase executed in wrong order.
    #[error("pipeline phase {phase} executed out of order")]
    PhaseOrderViolation {
        /// The phase that was executed incorrectly.
        phase: &'static str,
    },

    /// Root render object not set.
    #[error("root render object not set")]
    RootNotSet,

    // ========================================================================
    // Compositing Errors
    // ========================================================================
    /// Compositing bits update failed.
    #[error("compositing bits update failed: {message}")]
    CompositingError {
        /// Description of the compositing error.
        message: Box<str>,
    },

    // ========================================================================
    // Semantics Errors
    // ========================================================================
    /// Semantics operation failed.
    #[error("semantics operation failed: {message}")]
    SemanticsError {
        /// Description of the semantics error.
        message: Box<str>,
    },

    /// Semantics not enabled.
    #[error("semantics system not enabled")]
    SemanticsNotEnabled,

    // ChildHandleError variant removed in Mythos Step 5b along with the
    // child_handle.rs / children_access.rs modules it served.

    // ========================================================================
    // Mythos Step 12 -- structured terminal failures
    // ========================================================================
    /// Geometry returned from a render object's `perform_layout` is
    /// structurally invalid (NaN, negative dimensions, larger than
    /// `f32::MAX / 2`, etc.). The frame is dropped; the previous
    /// geometry remains valid.
    #[error("invalid geometry from {render_object}: {reason}")]
    InvalidGeometry {
        /// Static debug name of the offending render object.
        render_object: &'static str,
        /// Reason the geometry failed validation.
        reason: &'static str,
    },

    /// A render object received an unbounded constraint where it
    /// expected bounded input. The parent must provide bounds (e.g.
    /// wrap the child in a `SizedBox` or `Container`).
    #[error("unbounded constraint at {render_object}; parent must provide bounds")]
    UnboundedConstraint {
        /// Static debug name of the render object that needed bounds.
        render_object: &'static str,
    },

    /// Layout traversal exceeded the depth limit. Almost always
    /// indicates infinite parent-child recursion in user code.
    #[error("layout depth limit exceeded ({limit}); infinite recursion suspected")]
    LayoutDepthExceeded {
        /// The depth limit that was exceeded.
        limit: usize,
    },

    /// A render object's `perform_layout_raw` or `paint` panicked. The
    /// pipeline catches via `std::panic::catch_unwind`, drops the
    /// in-flight frame, and surfaces this variant so the caller can
    /// decide (drop the node, retry next frame, abort).
    ///
    /// Mythos Step 12 (2026-05-20): the catch_unwind plumbing is live.
    /// See [`RenderEntry::layout`](crate::storage::RenderEntry::layout)
    /// for the layout wrapper and `PipelineOwner::<PaintPhase>` for the
    /// paint wrapper. The `Mapping decisions` section of
    /// `crates/flui-rendering/ARCHITECTURE.md` documents the design.
    #[error("render object {render_object} panicked during {phase}")]
    Poisoned {
        /// Static debug name of the offending render object.
        render_object: &'static str,
        /// Phase during which the panic occurred (e.g. `"layout"`).
        phase: &'static str,
    },
}

/// Result type alias for render operations.
pub type RenderResult<T> = Result<T, RenderError>;

impl RenderError {
    /// Creates an invalid constraints error with a message.
    ///
    /// Cycle 4 R-17: message stored as `Box<str>` (heap allocation
    /// shrinks from 24 bytes of `String` header to 16 bytes of fat
    /// pointer).
    ///
    /// PR #112 review fix: constructor bound is `impl AsRef<str>`
    /// rather than `impl Into<Box<str>>`. `AsRef<str>` is strictly
    /// more permissive -- it accepts `&str`, `String`, `Box<str>`,
    /// `&String`, `Cow<str>`, etc. The allocation happens once via
    /// `message.as_ref().into()` (`str` -> `Box<str>`). Pre-fix
    /// `Into<Box<str>>` rejected `&String` callers (no impl from
    /// `&String` to `Box<str>` without deref coercion).
    pub fn invalid_constraints(message: impl AsRef<str>) -> Self {
        Self::InvalidConstraints {
            message: message.as_ref().into(),
        }
    }

    /// Creates a relayout boundary violation error.
    pub fn relayout_boundary(message: impl AsRef<str>) -> Self {
        Self::RelayoutBoundaryViolation {
            message: message.as_ref().into(),
        }
    }

    /// Creates a layer error.
    pub fn layer(message: impl AsRef<str>) -> Self {
        Self::LayerError {
            message: message.as_ref().into(),
        }
    }

    /// Creates a compositing error.
    pub fn compositing(message: impl AsRef<str>) -> Self {
        Self::CompositingError {
            message: message.as_ref().into(),
        }
    }

    /// Creates a semantics error.
    pub fn semantics(message: impl AsRef<str>) -> Self {
        Self::SemanticsError {
            message: message.as_ref().into(),
        }
    }

    /// Creates an InvalidGeometry error.
    pub fn invalid_geometry(render_object: &'static str, reason: &'static str) -> Self {
        Self::InvalidGeometry {
            render_object,
            reason,
        }
    }

    /// Creates an UnboundedConstraint error.
    pub fn unbounded_constraint(render_object: &'static str) -> Self {
        Self::UnboundedConstraint { render_object }
    }

    /// Creates a LayoutDepthExceeded error.
    pub fn layout_depth_exceeded(limit: usize) -> Self {
        Self::LayoutDepthExceeded { limit }
    }

    /// Creates a Poisoned error.
    pub fn poisoned(render_object: &'static str, phase: &'static str) -> Self {
        Self::Poisoned {
            render_object,
            phase,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RenderError::NotAttached;
        assert_eq!(
            err.to_string(),
            "render object is not attached to a pipeline owner"
        );

        let err = RenderError::invalid_constraints("min > max");
        assert_eq!(err.to_string(), "invalid constraints: min > max");
    }

    #[test]
    fn test_error_helpers() {
        let err = RenderError::layer("buffer overflow");
        assert!(matches!(err, RenderError::LayerError { .. }));

        let err = RenderError::compositing("invalid bit state");
        assert!(matches!(err, RenderError::CompositingError { .. }));
    }
}
