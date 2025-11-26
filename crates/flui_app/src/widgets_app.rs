//! WidgetsApp - Base application widget
//!
//! This is analogous to Flutter's WidgetsApp. It provides the foundational
//! widgets and services that all FLUI apps need:
//!
//! - DefaultTextStyle
//! - Directionality
//! - MediaQuery (window size, density, etc.)
//! - Navigator (route management)
//! - Theme (optional, or use MaterialApp for Material Design)
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_app::WidgetsApp;
//!
//! fn main() {
//!     run_app(WidgetsApp::new(MyHomePage));
//! }
//! ```

use flui_core::BuildContext;
use flui_view::{IntoElement, StatelessView};

/// WidgetsApp - Base application widget
///
/// Provides core framework services:
/// - MediaQuery (window size, device pixel ratio)
/// - Directionality (LTR/RTL)
/// - DefaultTextStyle
/// - Navigator (for routing)
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::{run_app, WidgetsApp};
/// use flui_widgets::Text;
///
/// #[derive(Debug, Clone)]
/// struct Home;
///
/// impl StatelessView for Home {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Hello, FLUI!")
///     }
/// }
///
/// fn main() {
///     run_app(WidgetsApp::new(Home));
/// }
/// ```
#[derive(Debug, Clone)]
pub struct WidgetsApp<V>
where
    V: StatelessView + Clone,
{
    /// The home widget (typically your app's main screen)
    home: V,

    /// Application title (used by OS for window title, taskbar, etc.)
    title: Option<String>,

    /// Text direction (LTR or RTL)
    /// Default: LTR (left-to-right)
    text_direction: TextDirection,
}

/// Text direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    /// Left-to-right (English, Spanish, etc.)
    Ltr,
    /// Right-to-left (Arabic, Hebrew, etc.)
    Rtl,
}

impl Default for TextDirection {
    fn default() -> Self {
        Self::Ltr
    }
}

impl<V> WidgetsApp<V>
where
    V: StatelessView + Clone,
{
    /// Create a new WidgetsApp
    ///
    /// # Parameters
    ///
    /// - `home`: The home widget (main screen of your app)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// WidgetsApp::new(MyHomePage)
    /// ```
    pub fn new(home: V) -> Self {
        Self {
            home,
            title: None,
            text_direction: TextDirection::default(),
        }
    }

    /// Set the application title
    ///
    /// Used for window title, taskbar, etc.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set text direction
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// WidgetsApp::new(home)
    ///     .text_direction(TextDirection::Rtl) // For Arabic/Hebrew apps
    /// ```
    #[must_use]
    pub fn text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = direction;
        self
    }
}

impl<V> StatelessView for WidgetsApp<V>
where
    V: StatelessView + Clone,
{
    fn build(self, ctx: &dyn BuildContext) -> impl IntoElement {
        // TODO: Wrap in providers:
        // - MediaQueryProvider (window size, device pixel ratio)
        // - DirectionalityProvider (text direction)
        // - DefaultTextStyleProvider
        // - NavigatorProvider (route management)

        // For now, just build and return the home widget
        // Later we'll add the full provider stack
        self.home.build(ctx)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::Element;

    #[derive(Debug, Clone)]
    struct TestHome;

    impl StatelessView for TestHome {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            Element::empty()
        }
    }

    #[test]
    fn test_widgets_app_creation() {
        let app = WidgetsApp::new(TestHome);
        assert!(app.title.is_none());
        assert_eq!(app.text_direction, TextDirection::Ltr);
    }

    #[test]
    fn test_widgets_app_with_title() {
        let app = WidgetsApp::new(TestHome).title("My App");
        assert_eq!(app.title, Some("My App".to_string()));
    }

    #[test]
    fn test_widgets_app_with_rtl() {
        let app = WidgetsApp::new(TestHome).text_direction(TextDirection::Rtl);
        assert_eq!(app.text_direction, TextDirection::Rtl);
    }
}
