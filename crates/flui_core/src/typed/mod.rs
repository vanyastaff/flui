//! Typed RenderObject system with compile-time arity constraints
//!
//! This module provides an alternative, more type-safe architecture for RenderObjects
//! compared to the dynamic `DynRenderObject` trait.
//!
//! # Key Concepts
//!
//! ## Arity as Types
//!
//! Instead of checking child count at runtime, we encode it in the type system:
//!
//! - `LeafArity`: No children (e.g., `RenderParagraph`, `RenderImage`)
//! - `SingleArity`: Exactly one child (e.g., `RenderOpacity`, `RenderPadding`)
//! - `MultiArity`: Zero or more children (e.g., `RenderFlex`, `RenderStack`)
//!
//! ## Benefits
//!
//! 1. **Compile-time safety**: Trying to call `.children()` on a `LeafArity`
//!    object results in a compile error, not a runtime panic.
//!
//! 2. **Zero-cost abstractions**: No `Box<dyn>`, no `downcast_mut`, no virtual
//!    dispatch overhead. Everything is monomorphized and can be inlined.
//!
//! 3. **Better IDE support**: The compiler knows exactly what methods are
//!    available on each context type.

pub mod arity;
pub mod context;
pub mod render_object;

// Re-export main types for convenience
pub use arity::{RenderArity, LeafArity, SingleArity, MultiArity};
pub use context::{LayoutCx, PaintCx};
pub use render_object::{RenderObject, HitTestable};
