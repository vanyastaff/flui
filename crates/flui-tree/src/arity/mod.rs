//! Advanced compile-time arity system for tree nodes.
//!
//! This module provides a production-grade, zero-cost abstraction for expressing
//! and validating child counts using advanced Rust type system features:
//!
//! - **Const Generics** for compile-time size validation
//! - **GAT (Generic Associated Types)** for flexible accessors
//! - **Associated Constants** for performance tuning
//! - **Sealed traits** for safety
//!
//! # Arity Types
//!
//! | Type | Description | Use Case |
//! |------|-------------|----------|
//! | [`Leaf`] | 0 children | Text, Image, Spacer |
//! | [`Optional`] | 0 or 1 child | `SizedBox`, Container |
//! | [`Single`] | exactly 1 child | Padding, Align, Transform |
//! | [`Exact<N>`] | exactly N children | Custom layouts |
//! | [`AtLeast<N>`] | N or more children | Min-child layouts |
//! | [`Variable`] | any number | Flex, Stack, Column |
//! | [`Range<MIN, MAX>`] | bounded range | Constrained layouts |
//!
//! # Storage Types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`ArityStorage<T, A>`] | Generic storage with arity constraint |
//! | [`SingleChildStorage<T>`] | Alias for `ArityStorage<T, Exact<1>>` |
//! | [`OptionalChildStorage<T>`] | Alias for `ArityStorage<T, Optional>` |
//! | [`VariableChildrenStorage<T>`] | Alias for `ArityStorage<T, Variable>` |
//! | [`LeafStorage<T>`] | Alias for `ArityStorage<T, Leaf>` |
//!
//! # Example
//!
//! ```
//! use flui_tree::arity::{Arity, Single, Variable, ArityStorage};
//!
//! // Single child container
//! struct RenderPadding {
//!     child: ArityStorage<u32, Single>,
//! }
//!
//! // Variable children container
//! struct RenderFlex {
//!     children: ArityStorage<u32, Variable>,
//! }
//! ```

// ============================================================================
// MODULES
// ============================================================================

mod accessors;
mod aliases;
mod arity_storage;
mod error;
mod runtime;
pub mod storage;
mod traits;
mod types;

// ============================================================================
// RE-EXPORTS - Accessors
// ============================================================================

pub use accessors::{
    // Performance enums
    AccessFrequency,
    AccessPattern,
    // Accessors
    BoundedChildren,
    ChildrenAccess,
    Copied,
    FixedChildren,
    NeverAccessor,
    NoChildren,
    OptionalChild,
    SliceChildren,
    SmartChildren,
    TypeInfo,
    TypedChildren,
};

// ============================================================================
// RE-EXPORTS - Runtime
// ============================================================================

pub use runtime::{PerformanceHint, RuntimeArity};

// ============================================================================
// RE-EXPORTS - Error
// ============================================================================

pub use error::ArityError;

// ============================================================================
// RE-EXPORTS - Trait
// ============================================================================

pub use traits::Arity;

// ============================================================================
// RE-EXPORTS - Types
// ============================================================================

pub use types::{AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable};

// ============================================================================
// RE-EXPORTS - Storage
// ============================================================================

pub use arity_storage::{ArityStorage, ArityStorageView};
pub use storage::{ChildrenStorage, ChildrenStorageExt};

// ============================================================================
// RE-EXPORTS - Aliases
// ============================================================================

pub use aliases::{LeafStorage, OptionalChildStorage, SingleChildStorage, VariableChildrenStorage};
