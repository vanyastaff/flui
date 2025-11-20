//! Scaffold widget - implements basic Material Design visual layout structure
//!
//! A widget that provides standard app structure with slots for common elements.
//! Similar to Flutter's Scaffold widget.
//!
//! # Usage Patterns
//!
//! ```rust,ignore
//! Scaffold::builder()
//!     .app_bar(AppBar::new("My App"))
//!     .body(content_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_types::prelude::Color;

/// A widget that implements the basic Material Design visual layout structure.
///
/// Scaffold provides a framework to implement the standard app structure with:
/// - An app bar at the top
/// - A body for the main content
/// - A bottom navigation bar
/// - Floating action buttons
/// - Drawers for navigation
///
/// ## Layout Structure
///
/// ```text
/// ┌─────────────────────┐
/// │ App Bar             │
/// ├─────────────────────┤
/// │                     │
/// │  Body (main content)│
/// │                     │
/// ├─────────────────────┤
/// │ Bottom Nav Bar      │
/// └─────────────────────┘
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic scaffold with app bar and body
/// Scaffold::builder()
///     .app_bar(AppBar::new("My App"))
///     .body(Center::builder()
///         .child(Text::new("Hello, World!"))
///         .build())
///     .build()
///
/// // Scaffold with custom background color
/// Scaffold::builder()
///     .background_color(Color::rgb(240, 240, 240))
///     .body(my_content)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(Color, into), finish_fn(name = build_internal, vis = ""))]
pub struct Scaffold {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Widget to display at the top of the scaffold (typically an AppBar)
    pub app_bar: Option<Box<dyn AnyView>>,

    /// The primary content of the scaffold
    #[builder(setters(vis = "", name = body_internal))]
    pub body: Option<Box<dyn AnyView>>,

    /// Widget to display at the bottom (typically a BottomNavigationBar)
    pub bottom_navigation_bar: Option<Box<dyn AnyView>>,

    /// Button displayed floating above the body (typically a FloatingActionButton)
    pub floating_action_button: Option<Box<dyn AnyView>>,

    /// Panel sliding in from the side for navigation
    pub drawer: Option<Box<dyn AnyView>>,

    /// Panel sliding in from the right side
    pub end_drawer: Option<Box<dyn AnyView>>,

    /// Background color of the scaffold
    #[builder(default = Color::WHITE)]
    pub background_color: Color,

    /// Whether to resize the body when the keyboard appears
    #[builder(default = true)]
    pub resize_to_avoid_bottom_inset: bool,
}

impl std::fmt::Debug for Scaffold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scaffold")
            .field("key", &self.key)
            .field("app_bar", &self.app_bar.as_ref().map(|_| "<AnyView>"))
            .field("body", &self.body.as_ref().map(|_| "<AnyView>"))
            .field(
                "bottom_navigation_bar",
                &self.bottom_navigation_bar.as_ref().map(|_| "<AnyView>"),
            )
            .field(
                "floating_action_button",
                &self.floating_action_button.as_ref().map(|_| "<AnyView>"),
            )
            .field("drawer", &self.drawer.as_ref().map(|_| "<AnyView>"))
            .field("end_drawer", &self.end_drawer.as_ref().map(|_| "<AnyView>"))
            .field("background_color", &self.background_color)
            .field(
                "resize_to_avoid_bottom_inset",
                &self.resize_to_avoid_bottom_inset,
            )
            .finish()
    }
}

impl Clone for Scaffold {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            app_bar: self.app_bar.clone(),
            body: self.body.clone(),
            bottom_navigation_bar: self.bottom_navigation_bar.clone(),
            floating_action_button: self.floating_action_button.clone(),
            drawer: self.drawer.clone(),
            end_drawer: self.end_drawer.clone(),
            background_color: self.background_color,
            resize_to_avoid_bottom_inset: self.resize_to_avoid_bottom_inset,
        }
    }
}

impl Scaffold {
    /// Creates a new Scaffold with default settings.
    pub fn new() -> Self {
        Self {
            key: None,
            app_bar: None,
            body: None,
            bottom_navigation_bar: None,
            floating_action_button: None,
            drawer: None,
            end_drawer: None,
            background_color: Color::WHITE,
            resize_to_avoid_bottom_inset: true,
        }
    }

    /// Creates a Scaffold with just a body widget.
    pub fn with_body(body: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            app_bar: None,
            body: Some(body),
            bottom_navigation_bar: None,
            floating_action_button: None,
            drawer: None,
            end_drawer: None,
            background_color: Color::WHITE,
            resize_to_avoid_bottom_inset: true,
        }
    }
}

impl Default for Scaffold {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use scaffold_builder::{IsUnset, SetBody, State};

// Custom body setter
impl<S: State> ScaffoldBuilder<S>
where
    S::Body: IsUnset,
{
    /// Sets the body widget (works in builder chain).
    pub fn body(self, body: impl View + 'static) -> ScaffoldBuilder<SetBody<S>> {
        self.body_internal(Box::new(body))
    }
}

// Build wrapper
impl<S: State> ScaffoldBuilder<S> {
    /// Builds the Scaffold widget.
    pub fn build(self) -> Scaffold {
        self.build_internal()
    }
}

// Implement View trait
impl View for Scaffold {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        use crate::{ColoredBox, Column, Stack};

        // Build the scaffold layout as a column
        let mut children: Vec<Box<dyn AnyView>> = Vec::new();

        // Add app bar if present
        if let Some(app_bar) = self.app_bar {
            children.push(app_bar);
        }

        // Add body (wrapped in Expanded to fill remaining space)
        if let Some(body) = self.body {
            children.push(Box::new(crate::Expanded {
                flex: 1,
                child: body,
            }));
        }

        // Add bottom navigation bar if present
        if let Some(bottom_nav) = self.bottom_navigation_bar {
            children.push(bottom_nav);
        }

        // Create the main column
        let column = Column::builder().children(children).build();

        // Wrap in colored background
        let with_background = ColoredBox::builder()
            .color(self.background_color)
            .child(column)
            .build();

        // Always use Stack to support FAB and drawers
        // Even if they're not present, Stack with single child works fine
        let mut stack_children: Vec<Box<dyn AnyView>> = vec![Box::new(with_background)];

        // Add FAB if present (positioned bottom-right)
        if let Some(fab) = self.floating_action_button {
            let positioned = crate::Positioned {
                key: None,
                left: None,
                top: None,
                right: Some(16.0),
                bottom: Some(16.0),
                width: None,
                height: None,
                child: Some(fab),
            };
            stack_children.push(Box::new(positioned));
        }

        // Note: Drawers would require gesture detection and animation
        // which is beyond the scope of this basic implementation
        // For now, we just note their presence in the structure

        Stack::builder().children(stack_children).build()
    }
}

/// Macro for creating Scaffold with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty scaffold
/// scaffold!()
///
/// // With body
/// scaffold! {
///     body: my_widget
/// }
///
/// // With all properties
/// scaffold! {
///     background_color: Color::WHITE,
///     body: content,
///     app_bar: my_app_bar
/// }
/// ```
#[macro_export]
macro_rules! scaffold {
    // Empty scaffold
    () => {
        $crate::Scaffold::new()
    };

    // With properties using builder pattern
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::Scaffold::builder()
            $(.$field($value))+
            .build()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_new() {
        let widget = Scaffold::new();
        assert!(widget.key.is_none());
        assert!(widget.app_bar.is_none());
        assert!(widget.body.is_none());
        assert_eq!(widget.background_color, Color::WHITE);
        assert!(widget.resize_to_avoid_bottom_inset);
    }

    #[test]
    fn test_scaffold_with_body() {
        let body = Box::new(crate::SizedBox::new());
        let widget = Scaffold::with_body(body);
        assert!(widget.body.is_some());
    }

    #[test]
    fn test_scaffold_default() {
        let widget = Scaffold::default();
        assert!(widget.body.is_none());
    }

    #[test]
    fn test_scaffold_builder() {
        let widget = Scaffold::builder().build_scaffold();
        assert_eq!(widget.background_color, Color::WHITE);
    }

    #[test]
    fn test_scaffold_builder_with_body() {
        let widget = Scaffold::builder()
            .body(crate::SizedBox::new())
            .build_scaffold();
        assert!(widget.body.is_some());
    }

    #[test]
    fn test_scaffold_builder_with_background() {
        let widget = Scaffold::builder()
            .background_color(Color::rgb(240, 240, 240))
            .build_scaffold();
        assert_eq!(widget.background_color, Color::rgb(240, 240, 240));
    }

    #[test]
    fn test_scaffold_macro() {
        let widget = scaffold!();
        assert!(widget.body.is_none());
    }
}
