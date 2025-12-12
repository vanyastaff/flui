//! Type aliases for common arity storage patterns.
//!
//! This module provides convenient type aliases for the most commonly used
//! arity storage configurations.

use super::arity_storage::ArityStorage;
use super::types::{Exact, Leaf, Optional, Variable};

/// Type alias for single-child storage (`Exact<1>`).
///
/// Use this for containers that must have exactly one child,
/// like `Padding`, `Align`, `Transform`, etc.
///
/// # Example
///
/// ```rust,ignore
/// struct RenderPadding {
///     children: SingleChildStorage<RenderObjectId>,
///     padding: EdgeInsets,
/// }
/// ```
pub type SingleChildStorage<T> = ArityStorage<T, Exact<1>>;

/// Type alias for optional-child storage (`Optional`).
///
/// Use this for containers that can have zero or one child,
/// like `SizedBox`, `Container`, `ColoredBox`.
///
/// # Example
///
/// ```rust,ignore
/// struct RenderSizedBox {
///     children: OptionalChildStorage<RenderObjectId>,
///     size: Size,
/// }
/// ```
pub type OptionalChildStorage<T> = ArityStorage<T, Optional>;

/// Type alias for variable-children storage (`Variable`).
///
/// Use this for containers that can have any number of children,
/// like `Flex`, `Stack`, `Column`, `Row`.
///
/// # Example
///
/// ```rust,ignore
/// struct RenderFlex {
///     children: VariableChildrenStorage<RenderObjectId>,
///     direction: Axis,
/// }
/// ```
pub type VariableChildrenStorage<T> = ArityStorage<T, Variable>;

/// Type alias for leaf storage (no children).
///
/// Use this for nodes that cannot have children,
/// like `Text`, `Image`, `Spacer`.
///
/// # Example
///
/// ```rust,ignore
/// struct RenderText {
///     children: LeafStorage<RenderObjectId>,
///     text: String,
/// }
/// ```
pub type LeafStorage<T> = ArityStorage<T, Leaf>;
