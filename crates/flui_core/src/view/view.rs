//! View trait - Core abstraction for reactive UI
//!
//! The View trait is the simplified, unified API for building UI in Flui.
//! It eliminates boilerplate while maintaining the proven three-tree architecture.

use super::build_context::BuildContext;
use super::IntoElement;
use crate::element::Element;
use std::any::Any;

/// View trait - simplified API for reactive UI
///
/// Views are lightweight, immutable descriptions of what the UI should look like.
/// They use hooks for state management and return `impl IntoElement` for composition.
///
/// # Design Philosophy
///
/// - **No GAT State**: Use hooks (`use_signal`, `use_memo`) for state management
/// - **No GAT Element**: Return `impl IntoElement` for flexible composition
/// - **No rebuild()**: Framework handles efficient diffing automatically
/// - **Immutable**: Views are created fresh each frame and must be cheap to clone
/// - **Clone Required**: Views must implement `Clone` for type erasure with `AnyView`
/// - **Composable**: Views can contain other views via `IntoElement`
///
/// # Examples
///
/// ## Simple Composite Widget
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext};
///
/// #[derive(Debug, Clone)]
/// struct Card {
///     title: String,
///     content: String,
/// }
///
/// impl View for Card {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child(Text::new(self.title).size(24.0))
///             .child(Padding::all(16.0).child(Text::new(self.content)))
///     }
/// }
/// ```
///
/// ## With Hooks (Stateful Widget)
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext, use_signal};
///
/// #[derive(Debug, Clone)]
/// struct Counter;
///
/// impl View for Counter {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Hooks for state management
///         let count = use_signal(ctx, 0);
///
///         Column::new()
///             .child(Text::new(format!("Count: {}", count.get())))
///             .child(
///                 Button::new("Increment")
///                     .on_click(move || count.update(|n| n + 1))
///             )
///     }
/// }
/// ```
///
/// ## Render Widget (Wraps RenderObject)
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext};
/// use flui_rendering::RenderPadding;
///
/// #[derive(Debug, Clone)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: Option<Box<dyn AnyView>>,
/// }
///
/// impl View for Padding {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         // Tuple syntax for single-child render (shortest!)
///         (RenderPadding::new(self.padding), self.child)
///
///         // Or builder syntax:
///         // RenderPadding::new(self.padding)
///         //     .into_builder()
///         //     .with_child(self.child)
///     }
/// }
/// ```
///
/// # Migration from Old View Trait
///
/// **Before (Old View with GATs):**
/// ```rust,ignore
/// impl View for Padding {
///     type Element = Element;
///     type State = Option<Box<dyn Any>>;
///
///     fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
///         // 20+ lines of manual tree management...
///     }
///
///     fn rebuild(...) -> ChangeFlags { ... }
/// }
/// ```
///
/// **After (Simplified View):**
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         (RenderPadding::new(self.padding), self.child)
///     }
/// }
/// ```
///
/// **75% less boilerplate!**
pub trait View: Clone + 'static {
    /// Build this view into an element
    ///
    /// Returns anything that implements `IntoElement` - typically:
    /// - Other View implementations (composition)
    /// - RenderBuilder from render objects (wrapping render objects)
    /// - Tuples like `(RenderObject, child)` for single-child convenience
    ///
    /// # State Management
    ///
    /// Use hooks instead of GAT State:
    /// ```rust,ignore
    /// fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///     let count = use_signal(ctx, 0);      // Signal for reactive state
    ///     let doubled = use_memo(ctx, |_| {    // Memo for derived state
    ///         count.get() * 2
    ///     });
    ///
    ///     use_effect(ctx, move |_| {           // Effect for side effects
    ///         println!("Count: {}", count.get());
    ///     });
    ///
    ///     // Compose UI...
    /// }
    /// ```
    ///
    /// # BuildContext Parameter
    ///
    /// The `ctx: &BuildContext` parameter provides:
    /// - Access to hooks: `use_signal(ctx, ...)`, `use_memo(ctx, ...)`, etc.
    /// - Tree queries (rarely needed): `ctx.parent()`, `ctx.size()`, etc.
    /// - Inherited data (future): `ctx.depend_on::<Theme>()`
    ///
    /// # Performance
    ///
    /// The framework automatically handles rebuild optimization:
    /// - Compares views by type and props
    /// - Only rebuilds when necessary
    /// - No manual `rebuild()` method needed
    ///
    /// For custom optimization, implement `PartialEq`:
    /// ```rust,ignore
    /// #[derive(Clone, PartialEq)]  // ← Automatic optimization
    /// struct MyView {
    ///     text: String,
    /// }
    /// ```
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

/// ViewElement trait - bridge between View and Element
///
/// This trait allows views to work with different element types.
pub trait ViewElement: 'static {
    /// Convert this typed element into the Element enum
    fn into_element(self: Box<Self>) -> Element;

    /// Mark this element as needing rebuild
    fn mark_dirty(&mut self);

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as Any mut for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Change flags indicating what changed during rebuild
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeFlags(u8);

impl ChangeFlags {
    /// No changes
    pub const NONE: Self = Self(0);

    /// View needs rebuild (children changed)
    pub const NEEDS_BUILD: Self = Self(1 << 0);

    /// Layout needs recalculation
    pub const NEEDS_LAYOUT: Self = Self(1 << 1);

    /// Paint needs refresh
    pub const NEEDS_PAINT: Self = Self(1 << 2);

    /// All changes
    pub const ALL: Self = Self(0xFF);

    /// Create empty flags
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Check if any flag is set
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check if specific flag is set
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union of flags
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for ChangeFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for ChangeFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_flags() {
        assert!(ChangeFlags::NONE.is_empty());
        assert!(!ChangeFlags::NEEDS_BUILD.is_empty());

        let flags = ChangeFlags::NEEDS_BUILD | ChangeFlags::NEEDS_LAYOUT;
        assert!(flags.contains(ChangeFlags::NEEDS_BUILD));
        assert!(flags.contains(ChangeFlags::NEEDS_LAYOUT));
        assert!(!flags.contains(ChangeFlags::NEEDS_PAINT));
    }
}
