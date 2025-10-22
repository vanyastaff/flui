//! Tree Management
//!
//! Manages the element tree and rendering pipeline.
//!
//! # Module Structure
//!
//! - `element_tree` - Core element tree storage and operations
//! - `build_owner` - Build scheduling and global key registry
//! - `pipeline` - Rendering pipeline management
//! - `element_pool` - Element recycling for performance

// ============================================================================
// Module Declarations
// ============================================================================

pub mod build_owner;
pub mod element_pool;
pub mod element_tree;
pub mod pipeline;

// ============================================================================
// Public API Re-exports
// ============================================================================

pub use element_tree::ElementTree;
pub use element_pool::{ElementPool, ElementPoolStats};
pub use pipeline::PipelineOwner;
pub use build_owner::{BuildOwner, GlobalKeyId};
