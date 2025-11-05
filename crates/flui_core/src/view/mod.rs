//! View layer
//!
//! The view layer provides the BuildContext for widget building and manages
//! the view tree during the build phase.
//!
//! # Overview
//!
//! The view layer sits between widgets and elements, providing context and
//! utilities for building the element tree from widget descriptions.
//!
//! ## Key Components
//!
//! - [`BuildContext`]: Context provided to widgets during build
//! - View tree management (TODO(2025-02): Add ViewTree implementation)
//!
//! # Example
//!
//! ```rust,ignore
//! fn build(&self, ctx: &mut BuildContext) -> View {
//!     // Use BuildContext to build children
//!     Text::new("Hello, World!").into()
//! }
//! ```

pub mod any_view;
pub mod build_context;
pub mod sealed;
#[allow(clippy::module_inception)]  // view/view.rs is intentional for main View trait
pub mod view;
pub mod view_sequence;






pub use build_context::BuildContext;

// View trait and related types
pub use view::{ChangeFlags, View, ViewElement};
pub use any_view::AnyView;
pub use view_sequence::{ViewSequence, ViewElementSequence};

// TODO(2025-02): Add view tree management.
// The ViewTree will track widget-to-element mappings and provide
// efficient lookup during rebuild.






