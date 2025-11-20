//! Core trait for reactive UI components.

use super::build_context::BuildContext;
use super::IntoElement;

/// Immutable description of a UI component.
///
/// Views are lightweight configuration objects that describe what the UI should look like.
/// The framework converts them into [`Element`]s which manage mutable state and lifecycle.
///
/// # Architecture
///
/// ```text
/// View (immutable) → Element (mutable) → RenderObject (layout/paint)
/// ```
///
/// # Examples
///
/// ## Composite widget
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct Card {
///     title: String,
///     content: String,
/// }
///
/// impl View for Card {
///     fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child(Text::new(&self.title))
///             .child(Text::new(&self.content))
///     }
/// }
/// ```
///
/// ## Stateful widget with hooks
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct Counter;
///
/// impl View for Counter {
///     fn build(&self, ctx: &BuildContext) -> impl IntoElement {
///         let count = use_signal(ctx, 0);
///
///         Column::new()
///             .child(Text::new(format!("Count: {}", count.get())))
///             .child(Button::new("Increment")
///                 .on_click(move || count.update(|n| n + 1)))
///     }
/// }
/// ```
///
/// ## Render widget
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: Option<AnyElement>,
/// }
///
/// impl View for Padding {
///     fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
///         RenderPadding::new(self.padding).maybe_child(self.child.clone())
///     }
/// }
/// ```
///
/// [`Element`]: crate::element::Element
pub trait View: 'static {
    /// Builds this view into an element tree.
    ///
    /// Called by the framework when the view needs to be rendered. Returns any type
    /// that implements [`IntoElement`].
    ///
    /// # Parameters
    ///
    /// * `ctx` - Build context providing access to hooks and tree queries
    ///
    /// # Return types
    ///
    /// - Another `View` for composition
    /// - `RenderObject` with children via [`RenderBoxExt`]
    /// - Tuple `(RenderObject, children)`
    ///
    /// # State management
    ///
    /// Use hooks for reactive state:
    ///
    /// ```rust,ignore
    /// fn build(&self, ctx: &BuildContext) -> impl IntoElement {
    ///     let count = use_signal(ctx, 0);           // Reactive state
    ///     let doubled = use_memo(ctx, move |_| count.get() * 2);  // Derived
    ///
    ///     Text::new(format!("{}", doubled.get()))
    /// }
    /// ```
    ///
    /// [`RenderBoxExt`]: crate::render::RenderBoxExt
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}
