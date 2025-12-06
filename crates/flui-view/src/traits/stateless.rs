//! `StatelessView` - Views without internal state
//!
//! `StatelessView` is for simple views that only depend on their configuration.
//! They rebuild completely when their parent rebuilds.
//!
//! # Flutter Equivalence
//!
//! This trait is equivalent to Flutter's `StatelessWidget`. The key concepts:
//! - Immutable configuration (fields are moved into build)
//! - No mutable state across rebuilds
//! - Efficient for simple UI components
//!
//! # Performance Considerations
//!
//! **When to use StatelessView:**
//! - View only depends on constructor arguments
//! - No need to persist data across rebuilds
//! - Lightweight, frequently rebuilt components
//!
//! **Optimization tips:**
//! - Use `const` constructors where possible (Rust: `const fn new()`)
//! - Keep build methods fast and pure (no side effects)
//! - Cache expensive computations in parent components
//! - Consider `StatefulView` if you need to cache derived data across rebuilds

use crate::{BuildContext, IntoView};

/// `StatelessView` - A view without internal state
///
/// This is equivalent to Flutter's `StatelessWidget`. Use it for views that:
/// - Only depend on configuration passed to them
/// - Don't need to persist state between rebuilds
/// - Can be recreated at any time without affecting behavior
///
/// # Build Method
///
/// The `build` method is called in three situations:
/// 1. The first time the widget is inserted into the tree
/// 2. When the widget's parent changes configuration
/// 3. When an `InheritedWidget` (Provider) it depends on changes
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{StatelessView, BuildContext, IntoView};
///
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
///
/// // Usage
/// let view = Stateless(Greeting { name: "World".to_string() });
/// ```
///
/// # Performance Tips
///
/// - Keep `build` methods fast - they may be called every frame during animations
/// - Use const constructors when possible: `const fn new() -> Self`
/// - Avoid allocations in `build` when possible
/// - For expensive computations, cache results in parent or use `StatefulView`
///
/// # Thread Safety
///
/// `StatelessView` requires `Send + Sync + 'static` for cross-thread element transfer.
/// All fields must also be `Send + Sync`.
///
/// # See Also
///
/// - [`StatefulView`](crate::StatefulView) - For views with mutable state
/// - [`BuildContext`](crate::BuildContext) - Context passed during build
/// - Flutter's [StatelessWidget documentation](https://api.flutter.dev/flutter/widgets/StatelessWidget-class.html)
pub trait StatelessView: Send + Sync + 'static {
    /// Build the view, producing child view object(s).
    ///
    /// This method describes the part of the user interface represented by this widget.
    /// The framework calls this method in three situations:
    ///
    /// 1. **First build** - When the widget is first inserted into the tree
    /// 2. **Parent rebuild** - When the parent widget rebuilds and provides a new instance
    /// 3. **Dependency change** - When an inherited widget this depends on changes
    ///
    /// # Parameters
    ///
    /// - `self` - The view is consumed (moved) during build
    /// - `ctx` - Build context providing access to framework services
    ///
    /// # Returns
    ///
    /// Any type that implements [`IntoView`](crate::IntoView), typically another view or widget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
    ///     Column::new()
    ///         .child(Text::new(self.title))
    ///         .child(Text::new(self.subtitle))
    /// }
    /// ```
    ///
    /// # Flutter Equivalent
    ///
    /// This is equivalent to `StatelessWidget.build(BuildContext context)` in Flutter.
    fn build(self, ctx: &dyn BuildContext) -> impl IntoView;
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

// Note: We intentionally don't provide blanket impls here.
// Each concrete view type should implement StatelessView explicitly.
