//! Protocol trait and implementations for render object families.
//!
//! The Protocol trait defines the type system for render object families.
//! Each protocol specifies associated types that determine how layout,
//! constraints, and children work within that protocol's domain.
//!
//! # Module Structure
//!
//! - [`base`]: Core `Protocol` trait definition
//! - [`box_protocol`]: `BoxProtocol` for 2D cartesian layout
//! - [`sliver`]: `SliverProtocol` for scrollable content
//! - [`adapters`]: Protocol adapters for cross-protocol communication
//!
//! # Protocols
//!
//! - [`BoxProtocol`]: 2D cartesian layout with rectangular constraints
//! - [`SliverProtocol`]: Scrollable content with viewport-aware constraints
//!
//! # Type Flow
//!
//! ```text
//!                     Protocol Trait
//!                          │
//!            ┌─────────────┼─────────────┐
//!            ▼             ▼             ▼
//!     type Object   type Constraints  type Geometry
//!            │             │             │
//!            ▼             ▼             ▼
//!     Container      Layout Input    Layout Output
//!      Storage       (parent→child)  (child→parent)
//! ```

mod adapters;
mod base;
mod box_protocol;
mod sliver;

// Re-export core trait
pub use base::Protocol;

// Re-export protocol implementations
pub use box_protocol::BoxProtocol;
pub use sliver::SliverProtocol;

// Re-export adapters
pub use adapters::{ProtocolAdapter, SliverToBoxAdapter};
