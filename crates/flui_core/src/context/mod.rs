//! Element tree context for widgets
//!
//! This module provides the [`Context`] type for interacting with the element tree,
//! along with supporting types for dependency tracking and tree traversal.
//!
//! # Key Types
//!
//! - [`Context`] - Main context type for tree traversal and widget access
//! - [`DependencyTracker`] - Tracks InheritedWidget dependencies
//! - [`Ancestors`], [`Children`], [`Descendants`] - Tree traversal iterators
//!
//! # Examples
//!
//! ```rust,ignore
//! // Tree traversal
//! let depth = context.depth();
//! for ancestor in context.ancestors() {
//!     println!("Ancestor: {:?}", ancestor);
//! }
//!
//! // InheritedWidget access
//! let theme = context.inherit::<Theme>();
//!
//! // Marking dirty
//! context.mark_dirty();
//! ```

mod impl_;
pub mod dependency;
mod inherited;
mod iterators;

// Re-exports
pub use impl_::Context;
pub use dependency::{DependencyInfo, DependencyTracker};
pub use iterators::{Ancestors, Children, Descendants};