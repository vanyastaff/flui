//! DynWidget - Object-safe base trait for heterogeneous widget collections
//!
//! This module defines the `DynWidget` trait, which is object-safe and allows
//! widgets to be stored in heterogeneous collections like `Vec<Box<dyn DynWidget>>`.
//!
//! # Why DynWidget?
//!
//! The `Widget` trait has associated types, which makes it not object-safe.
//! This means you cannot create `Box<dyn Widget>` or `Vec<Box<dyn Widget>>`.
//!
//! `DynWidget` solves this by being object-safe - it doesn't have associated types.
//! Any type that implements `Widget` automatically implements `DynWidget` via a blanket impl.
//!
//! # Usage
//!
//! ```rust,ignore
//! // For heterogeneous collections
//! let widgets: Vec<Box<dyn DynWidget>> = vec![
//!     Box::new(Text::new("Hello")),
//!     Box::new(Button::new("Click")),
//!     Box::new(Row::new(vec![])),
//! ];
//!
//! // For concrete types with zero-cost
//! let text = Text::new("Hello");
//! let element = text.into_element(); // Uses Widget trait, no boxing!
//! ```

use std::fmt;

use downcast_rs::{impl_downcast, Downcast};
use dyn_clone::DynClone;
use crate::foundation::Key;
use crate::element::DynElement;

/// Object-safe base trait for all widgets
///
/// This trait is automatically implemented for all types that implement `Widget`.
/// It's used when you need trait objects (`Box<dyn DynWidget>`) for heterogeneous
/// widget collections.
///
/// # Design Pattern
///
/// Flui uses a two-trait pattern:
/// - **DynWidget** (this trait) - Object-safe, for `Box<dyn DynWidget>` collections
/// - **Widget** - Has associated types, for zero-cost concrete usage
///
/// # When to Use
///
/// - Use `Box<dyn DynWidget>` when you need to store widgets of different types
/// - Use `Widget` trait bound when working with concrete widget types
///
/// # Example
///
/// ```rust,ignore
/// struct Row {
///     children: Vec<Box<dyn DynWidget>>,  // Heterogeneous children
/// }
///
/// impl Row {
///     fn new(children: Vec<Box<dyn DynWidget>>) -> Self {
///         Self { children }
///     }
/// }
/// ```
pub trait DynWidget: DynClone + Downcast + fmt::Debug + Send + Sync {
    /// Create the Element that manages this widget's lifecycle
    ///
    /// This returns a boxed element for object safety. For zero-cost element
    /// creation, use `Widget::into_element()` instead.
    ///
    /// This is called when the widget is first inserted into the tree.
    /// The element persists across rebuilds, while the widget is recreated.
    fn create_element(&self) -> Box<dyn DynElement>;

    /// Optional key for widget identification
    ///
    /// Keys are used to preserve state when widgets move in the tree.
    /// Without keys, widgets are matched by type and position only.
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Type name for debugging
    #[must_use]
    fn type_name(&self) -> &'static str;

    /// Check if this widget can be updated with another widget
    ///
    /// By default, widgets can update if they have the same type and key.
    #[must_use]
    fn can_update(&self, other: &dyn DynWidget) -> bool;
}

// Enable cloning for boxed DynWidget trait objects
dyn_clone::clone_trait_object!(DynWidget);

// Enable downcasting for DynWidget trait objects
impl_downcast!(DynWidget);