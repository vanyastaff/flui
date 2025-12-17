//! Error Handling
//!
//! This module provides error boundary widgets for catching and displaying errors.

pub mod error_boundary;
pub mod error_widget;
mod render_error_box;

// Re-export main types
pub use error_boundary::{
    handle_build_panic, report_error_to_boundary, ErrorBoundary, ErrorBoundaryState,
};
pub use error_widget::{ErrorInfo, ErrorWidget};
