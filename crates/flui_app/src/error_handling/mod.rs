//! Error Handling
//!
//! This module re-exports error handling types from flui_core.
//!
//! Error handling is now part of the core framework for automatic panic catching
//! during widget builds.

// Re-export all error handling types from flui_core
pub use flui_core::error_handling::{
    handle_build_panic, report_error_to_boundary, ErrorBoundary, ErrorBoundaryState, ErrorInfo,
    ErrorWidget,
};
