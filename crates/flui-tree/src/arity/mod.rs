//! Compile-time arity markers for tree nodes.
//!
//! The arity system expresses per-node child-count constraints as
//! zero-sized type markers. Implementations attach the marker to
//! their child storage to enforce the constraint at compile time:
//!
//! ```ignore
//! use flui_tree::{Leaf, Single, Variable};
//!
//! struct RenderText { /* … */ }   // marker: Leaf
//! struct RenderPadding<C> { child: C }   // marker: Single
//! struct RenderFlex<C> { children: Vec<C> }   // marker: Variable
//! ```
//!
//! # Arity Markers
//!
//! | Marker | Description | Use Case |
//! |--------|-------------|----------|
//! | [`Leaf`] | 0 children | `RenderText`, `RenderColoredBox` |
//! | [`Optional`] | 0 or 1 child | `RenderSizedBox` |
//! | [`Single`] | exactly 1 child | `RenderPadding`, `RenderTransform` |
//! | [`Exact<N>`] | exactly N children | Custom layouts |
//! | [`AtLeast<N>`] | N or more children | Min-child layouts |
//! | [`Variable`] | any number | `RenderFlex`, `RenderStack` |
//! | [`Range<MIN, MAX>`] | bounded range | Constrained layouts |
//! | [`Never`] | uninhabited | Type-system bottom |
//!
//! Cycle 3 T-7: the storage machinery (`ArityStorage`,
//! `ChildrenStorage`, `ChildrenAccess`, `accessors` module,
//! `runtime` module, `aliases` module) was deleted as zombie
//! surface — ~3,000 LOC with zero in-workspace consumers. Concrete
//! render objects use plain `Option<C>` / `Vec<C>` storage attached
//! to the marker; the marker stays for compile-time documentation +
//! future arity-aware algorithms.

// ============================================================================
// MODULES
// ============================================================================

mod error;
mod traits;
mod types;

// ============================================================================
// RE-EXPORTS
// ============================================================================
pub use error::ArityError;
pub use traits::Arity;
pub use types::{AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable};
