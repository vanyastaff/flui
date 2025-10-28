//! Arity - compile-time child count specification
//!
//! Arity types encode the number of children a widget can have at the type level.
//! This enables compile-time validation and optimization.
//!
//! # Three Arity Types
//!
//! - **LeafArity** - No children (e.g., Text, Image)
//! - **SingleArity** - Exactly one child (e.g., Padding, Center)
//! - **MultiArity** - Multiple children (e.g., Row, Column, Stack)
//!
//! # Why Arity?
//!
//! 1. **Compile-time validation** - Catches errors at compile time
//! 2. **Type safety** - Prevents invalid widget trees
//! 3. **Optimization** - Enables specialized implementations
//! 4. **Documentation** - Makes child count explicit
//!
//! # Examples
//!
//! ```rust
//! use flui_core::{Widget, LeafArity, SingleArity, MultiArity};
//!
//! // Leaf widget - no children
//! impl Widget for Text {
//!     type Element = TextElement;
//!     type Arity = LeafArity;  // No children allowed
//! }
//!
//! // Single child widget
//! impl Widget for Padding {
//!     type Element = PaddingElement;
//!     type Arity = SingleArity;  // Exactly one child
//! }
//!
//! // Multi child widget
//! impl Widget for Column {
//!     type Element = ColumnElement;
//!     type Arity = MultiArity;  // Multiple children
//! }
//! ```

use std::fmt;

/// Arity - trait for compile-time child count specification
///
/// This trait is sealed and can only be implemented by the three
/// types in this module: `LeafArity`, `SingleArity`, `MultiArity`.
///
/// # Purpose
///
/// Arity types enable the type system to enforce child count constraints:
/// - Prevents accidentally adding children to leaf widgets
/// - Ensures single-child widgets don't get multiple children
/// - Makes child count explicit in widget definitions
///
/// # Type-Level Computation
///
/// Arity is a zero-sized type that exists only at compile time.
/// It has no runtime cost - all validation happens during compilation.
///
/// # Examples
///
/// ```rust
/// use flui_core::{Widget, LeafArity};
///
/// // Text widget has no children
/// impl Widget for Text {
///     type Arity = LeafArity;
/// }
///
/// // Compiler prevents this:
/// // let text = Text::new("hello");
/// // text.add_child(other);  // ← Compile error!
/// ```
pub trait Arity: private::Sealed + fmt::Debug + Send + Sync + 'static {
    /// Human-readable name for debugging
    const NAME: &'static str;
}

/// LeafArity - widget with no children
///
/// Used by widgets that are leaf nodes in the widget tree and
/// cannot have any children.
///
/// # Examples
///
/// - **Text** - Displays text
/// - **Image** - Displays image
/// - **Icon** - Displays icon
/// - **Placeholder** - Empty space
///
/// # Usage
///
/// ```rust
/// use flui_core::{Widget, RenderObjectWidget, LeafArity};
///
/// #[derive(Debug)]
/// struct Text {
///     content: String,
/// }
///
/// impl Widget for Text {
///     type Element = TextElement;
///     type Arity = LeafArity;  // ← No children
/// }
///
/// impl RenderObjectWidget for Text {
///     type RenderObject = RenderParagraph;
///     type Arity = LeafArity;  // ← Consistent
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeafArity;

impl Arity for LeafArity {
    const NAME: &'static str = "LeafArity";
}

/// SingleArity - widget with exactly one child
///
/// Used by widgets that wrap a single child widget and provide
/// some service or transformation.
///
/// # Examples
///
/// - **Padding** - Adds padding around child
/// - **Center** - Centers child
/// - **SizedBox** - Constrains child size
/// - **Opacity** - Makes child transparent
/// - **Transform** - Transforms child
///
/// # Usage
///
/// ```rust
/// use flui_core::{Widget, RenderObjectWidget, SingleArity, BoxedWidget};
///
/// #[derive(Debug)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: BoxedWidget,
/// }
///
/// impl Widget for Padding {
///     type Element = RenderObjectElement<Self>;
///     type Arity = SingleArity;  // ← One child
/// }
///
/// impl RenderObjectWidget for Padding {
///     type RenderObject = RenderPadding;
///     type Arity = SingleArity;  // ← Consistent
/// }
/// ```
///
/// # Getting the Child
///
/// Single-child widgets typically store the child as a field:
///
/// ```rust
/// #[derive(Debug)]
/// struct Container {
///     child: BoxedWidget,  // ← Store child
/// }
///
/// impl SingleChildRenderObjectWidget for Container {
///     fn child(&self) -> &BoxedWidget {
///         &self.child
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SingleArity;

impl Arity for SingleArity {
    const NAME: &'static str = "SingleArity";
}

/// MultiArity - widget with multiple children
///
/// Used by layout widgets that arrange multiple child widgets
/// according to some layout algorithm.
///
/// # Examples
///
/// - **Row** - Horizontal layout
/// - **Column** - Vertical layout
/// - **Stack** - Layered layout
/// - **Wrap** - Wrapping layout
/// - **ListView** - Scrollable list
///
/// # Usage
///
/// ```rust
/// use flui_core::{Widget, RenderObjectWidget, MultiArity, BoxedWidget};
///
/// #[derive(Debug)]
/// struct Column {
///     children: Vec<BoxedWidget>,
/// }
///
/// impl Widget for Column {
///     type Element = RenderObjectElement<Self>;
///     type Arity = MultiArity;  // ← Multiple children
/// }
///
/// impl RenderObjectWidget for Column {
///     type RenderObject = RenderFlex;
///     type Arity = MultiArity;  // ← Consistent
/// }
/// ```
///
/// # Getting the Children
///
/// Multi-child widgets typically store children as a Vec:
///
/// ```rust
/// #[derive(Debug)]
/// struct Row {
///     children: Vec<BoxedWidget>,  // ← Store children
/// }
///
/// impl MultiChildRenderObjectWidget for Row {
///     fn children(&self) -> &[BoxedWidget] {
///         &self.children
///     }
/// }
/// ```
///
/// # Performance
///
/// Multi-child widgets should use `Vec<BoxedWidget>` for storage:
/// - Efficient iteration
/// - Cache-friendly memory layout
/// - Easy to add/remove children
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultiArity;

impl Arity for MultiArity {
    const NAME: &'static str = "MultiArity";
}

/// Sealed trait pattern - prevents external implementations
mod private {
    pub trait Sealed {}

    impl Sealed for super::LeafArity {}
    impl Sealed for super::SingleArity {}
    impl Sealed for super::MultiArity {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arity_names() {
        assert_eq!(LeafArity::NAME, "LeafArity");
        assert_eq!(SingleArity::NAME, "SingleArity");
        assert_eq!(MultiArity::NAME, "MultiArity");
    }

    #[test]
    fn test_arity_zero_sized() {
        use std::mem::size_of;

        // All arity types are zero-sized
        assert_eq!(size_of::<LeafArity>(), 0);
        assert_eq!(size_of::<SingleArity>(), 0);
        assert_eq!(size_of::<MultiArity>(), 0);
    }

    #[test]
    fn test_arity_equality() {
        // Arity types are always equal to themselves
        assert_eq!(LeafArity, LeafArity);
        assert_eq!(SingleArity, SingleArity);
        assert_eq!(MultiArity, MultiArity);

        // Different arity types are not equal
        assert_ne!(LeafArity, SingleArity);
        assert_ne!(SingleArity, MultiArity);
        assert_ne!(LeafArity, MultiArity);
    }

    #[test]
    fn test_arity_clone() {
        let leaf = LeafArity;
        let _cloned = leaf.clone();

        let single = SingleArity;
        let _cloned = single.clone();

        let multi = MultiArity;
        let _cloned = multi.clone();
    }

    #[test]
    fn test_arity_debug() {
        let leaf = LeafArity;
        assert_eq!(format!("{:?}", leaf), "LeafArity");

        let single = SingleArity;
        assert_eq!(format!("{:?}", single), "SingleArity");

        let multi = MultiArity;
        assert_eq!(format!("{:?}", multi), "MultiArity");
    }

    #[test]
    fn test_arity_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(LeafArity);
        set.insert(SingleArity);
        set.insert(MultiArity);

        assert_eq!(set.len(), 3);
        assert!(set.contains(&LeafArity));
        assert!(set.contains(&SingleArity));
        assert!(set.contains(&MultiArity));
    }

    // Test that Arity is used correctly in widget definitions
    #[test]
    fn test_arity_in_widget() {
        use crate::{Widget, Element};

        // Just verify that arity types exist and implement Arity trait
        fn assert_arity<A: Arity>() {
            // Compile-time check
        }

        assert_arity::<LeafArity>();
        assert_arity::<SingleArity>();
        assert_arity::<MultiArity>();
    }
}