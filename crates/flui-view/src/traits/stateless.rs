//! StatelessView - Views without internal state
//!
//! StatelessView is for simple views that only depend on their configuration.
//! They rebuild completely when their parent rebuilds.

use flui_element::IntoElement;

use flui_element::BuildContext;

/// StatelessView - A view without internal state
///
/// Use StatelessView when your view:
/// - Only depends on configuration passed to it
/// - Doesn't need to persist state between rebuilds
/// - Can be recreated at any time
///
/// # Example
///
/// ```rust,ignore
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
/// ```
///
/// # Thread Safety
///
/// StatelessView requires `Send + 'static` for cross-thread element transfer.
pub trait StatelessView: Send + Sync + 'static {
    /// Build the view, producing child element(s)
    ///
    /// Called by the framework during the build phase.
    /// Return any type that implements `IntoElement`.
    fn build(self, ctx: &dyn BuildContext) -> impl IntoElement;
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

// Note: We intentionally don't provide blanket impls here.
// Each concrete view type should implement StatelessView explicitly.
