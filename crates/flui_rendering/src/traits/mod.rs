//! Trait definitions for render objects
//!
//! This module provides the complete trait hierarchy for render objects:
//!
//! # Base Traits
//! - `RenderObject` - All render objects implement this
//!
//! # Box Protocol Traits
//! - `RenderBox` - 2D cartesian layout
//! - `SingleChildRenderBox` - One child accessor
//! - `RenderProxyBox` - Pass-through (size = child size)
//! - `RenderShiftedBox` - Custom child positioning
//! - `RenderAligningShiftedBox` - Alignment-based positioning
//! - `MultiChildRenderBox` - Multiple children accessor
//!
//! # Sliver Protocol Traits
//! - `RenderSliver` - Scrollable content
//! - `RenderProxySliver` - Pass-through sliver
//! - `RenderSliverSingleBoxAdapter` - Sliver wrapping one box
//! - `RenderSliverMultiBoxAdaptor` - Sliver with multiple boxes
//!
//! # Ambassador Delegation
//!
//! All traits are marked with `#[ambassador::delegatable_trait]` for automatic
//! trait delegation. This enables implementing complex render objects with minimal
//! boilerplate:
//!
//! ```ignore
//! use ambassador::Delegate;
//!
//! #[derive(Delegate)]
//! #[delegate(RenderProxyBox, target = "proxy")]
//! struct RenderOpacity {
//!     proxy: ProxyBox,
//!     opacity: f32,
//! }
//!
//! // Just implement the marker trait!
//! impl RenderProxyBox for RenderOpacity {}
//!
//! // Automatically get:
//! // - SingleChildRenderBox
//! // - RenderBox
//! // - RenderObject
//! ```

mod render_object;
pub mod r#box;
pub mod sliver;

pub use render_object::{RenderObject, RenderObjectExt};
pub use r#box::*;
pub use sliver::*;
