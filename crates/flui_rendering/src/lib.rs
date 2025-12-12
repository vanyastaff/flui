//! FLUI Rendering Layer
//!
//! This crate provides the rendering infrastructure for FLUI, a Flutter-inspired
//! declarative UI framework for Rust. It implements the proven three-tree architecture:
//!
//! ```text
//! View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
//! ```
//!
//! # Architecture
//!
//! The rendering system is built on a **Protocol-based architecture** that provides
//! compile-time type safety through associated types.
//!
//! ## Protocol System
//!
//! The `Protocol` trait defines four associated types that determine how layout works:
//!
//! - **Object**: Type of render objects (`dyn RenderBox`, `dyn RenderSliver`)
//! - **Constraints**: Layout input (`BoxConstraints`, `SliverConstraints`)
//! - **ParentData**: Child metadata (`BoxParentData`, `SliverParentData`)
//! - **Geometry**: Layout output (`Size`, `SliverGeometry`)
//!
//! ## Two Main Protocols
//!
//! ### BoxProtocol
//! - 2D Cartesian layout with rectangular constraints
//! - Used for: fixed size widgets, flex layouts, effects
//! - Example: `Container`, `Row`, `Column`, `Padding`
//!
//! ### SliverProtocol
//! - Scrollable content with viewport-aware constraints
//! - Used for: infinite scrolling, lazy rendering
//! - Example: `ListView`, `GridView`, `SliverAppBar`
//!
//! ## Type-Safe Containers
//!
//! Generic containers use `Protocol::Object` for automatic type selection:
//!
//! ```ignore
//! pub struct Proxy<P: Protocol> {
//!     child: Single<P>,        // Uses P::Object automatically
//!     geometry: P::Geometry,   // Size or SliverGeometry
//! }
//! ```
//!
//! This eliminates runtime downcasts and provides compile-time guarantees.
//!
//! # Module Organization
//!
//! - **protocol**: Core `Protocol` trait and implementations
//! - **constraints**: Layout input types (`BoxConstraints`, `SliverConstraints`)
//! - **geometry**: Layout output types (`Size`, `SliverGeometry`)
//! - **parent_data**: Child metadata types
//! - **containers**: Generic child storage (`Single`, `Children`, `Proxy`, etc.)
//! - **traits**: Render object trait hierarchy
//!
//! # Quick Start
//!
//! ```ignore
//! use flui_rendering::prelude::*;
//!
//! // Box protocol example
//! struct RenderMyWidget {
//!     proxy: ProxyBox,
//!     color: Color,
//! }
//!
//! impl RenderBox for RenderMyWidget {
//!     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
//!         // Layout child
//!         let size = if let Some(child) = self.proxy.child_mut() {
//!             child.perform_layout(constraints)
//!         } else {
//!             constraints.smallest()
//!         };
//!
//!         self.proxy.set_geometry(size);
//!         size
//!     }
//!
//!     fn size(&self) -> Size {
//!         *self.proxy.geometry()
//!     }
//!
//!     fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
//!         // Paint implementation
//!     }
//! }
//! ```
//!
//! # Key Benefits
//!
//! - ✅ **Compile-time type safety**: Protocol mismatch caught by compiler
//! - ✅ **Zero-cost abstractions**: Generic containers with no runtime overhead
//! - ✅ **No downcasts**: Direct method access on children
//! - ✅ **Extensible**: Add new protocols without changing core system
//!
//! # Features
//!
//! - `serde` - Enable serialization support for constraints and geometry

// Core modules
#[doc(hidden)]

pub mod protocol;
pub mod constraints;
pub mod geometry;
pub mod parent_data;
pub mod containers;
pub mod traits;

// Protocol implementations (separated to avoid circular deps)
mod box_protocol;
mod sliver_protocol;

// Prelude for convenient imports
pub mod prelude;

// Re-export commonly used types at crate root
pub use protocol::{BoxProtocol, Protocol, SliverProtocol};
pub use constraints::{BoxConstraints, SliverConstraints};
pub use geometry::{Size, SliverGeometry};
pub use parent_data::{BoxParentData, ParentData, SliverParentData};
pub use traits::{RenderBox, RenderObject, RenderSliver};
