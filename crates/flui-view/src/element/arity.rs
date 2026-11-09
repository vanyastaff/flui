//! Element arity system for compile-time child count constraints.
//!
//! This module provides a type-safe way to express element child constraints
//! at compile time using the arity markers from `flui_foundation::arity`.
//!
//! # Arity Types
//!
//! - `Single` - Exactly one child (e.g., StatelessElement, ProxyElement)
//! - `Variable` - N children (e.g., RenderElement with multiple children)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::element::{ElementCore, Single};
//!
//! // StatelessElement has exactly one child
//! pub struct StatelessElement<V> {
//!     core: ElementCore<V, Single>,
//! }
//! ```

// Re-export the arity markers the render-object protocol uses, so both sides name one type
pub use flui_foundation::{Arity, Leaf, Optional, Single, Variable};

/// Element-specific arity marker trait.
///
/// E3 (atomic box→arena swap): this used to carry a `type Storage:
/// ElementChildStorage` associated type that owned the per-element box
/// child graph. The slab-resident [`ElementTree`](crate::tree::ElementTree)
/// is now the single element graph, so there is no per-element storage to
/// associate. The trait remains as the compile-time child-count constraint
/// (`Single` = 1, `Variable` = N) layered over `flui_foundation::Arity`; it
/// simply no longer carries storage.
///
/// `Leaf` and `Optional` are render-object arity only: no element instantiates
/// them, so they do not implement `ElementArity`.
pub trait ElementArity: Arity {}

/// Single arity - exactly one child required.
///
/// Used for elements that wrap a single child (e.g., StatelessElement,
/// ProxyElement).
impl ElementArity for Single {}

/// Variable arity - N children allowed.
///
/// Used for elements that can have multiple children (e.g., RenderElement).
impl ElementArity for Variable {}
