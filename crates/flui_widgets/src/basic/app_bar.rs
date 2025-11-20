//! AppBar widget - implements Material Design app bar
//!
//! A Material Design app bar displayed at the top of the scaffold.
//! Similar to Flutter's AppBar widget.
//!
//! # Usage Patterns
//!
//! ```rust,ignore
//! AppBar::builder()
//!     .title(Text::new("My App"))
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_types::prelude::Color;

/// Material Design app bar.
///
/// AppBar is typically displayed at the top of a Scaffold and provides:
/// - A title (usually text)
/// - Leading widget (usually back button or menu icon)
/// - Actions (typically icon buttons)
/// - Custom background color
///
/// ## Visual Structure
///
/// ```text
/// ┌──────────────────────────────────┐
/// │ [≡] Title            [🔍] [⋮]    │
/// └──────────────────────────────────┘
///  leading   title         actions
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple app bar with title
/// AppBar::builder()
///     .title(Text::new("My App"))
///     .build()
///
/// // App bar with actions
/// AppBar::builder()
///     .title(Text::new("Messages"))
///     .actions(vec![
///         Box::new(IconButton::new(Icon::Search)),
///         Box::new(IconButton::new(Icon::More)),
///     ])
///     .build()
///
/// // Custom colored app bar
/// AppBar::builder()
///     .title(Text::new("Settings"))
///     .background_color(Color::rgb(33, 150, 243))
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(Color, into), finish_fn(name = build_internal, vis = ""))]
pub struct AppBar {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The primary widget displayed in the app bar (typically Text)
    #[builder(setters(vis = "", name = title_internal))]
    pub title: Option<Box<dyn AnyView>>,

    /// Widget to display before the title (typically back button or menu icon)
    pub leading: Option<Box<dyn AnyView>>,

    /// Widgets to display after the title (typically icon buttons)
    #[builder(default = vec![])]
    pub actions: Vec<Box<dyn AnyView>>,

    /// Background color of the app bar
    #[builder(default = Color::rgb(33, 150, 243))] // Material Blue 500
    pub background_color: Color,

    /// Elevation (shadow depth) of the app bar
    #[builder(default = 4.0)]
    pub elevation: f32,

    /// Height of the app bar
    #[builder(default = 56.0)]
    pub height: f32,
}

impl std::fmt::Debug for AppBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBar")
            .field("key", &self.key)
            .field("title", &self.title.as_ref().map(|_| "<AnyView>"))
            .field("leading", &self.leading.as_ref().map(|_| "<AnyView>"))
            .field("actions", &format!("[{} actions]", self.actions.len()))
            .field("background_color", &self.background_color)
            .field("elevation", &self.elevation)
            .field("height", &self.height)
            .finish()
    }
}

impl Clone for AppBar {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            title: self.title.clone(),
            leading: self.leading.clone(),
            actions: self.actions.clone(),
            background_color: self.background_color,
            elevation: self.elevation,
            height: self.height,
        }
    }
}

impl AppBar {
    /// Creates a new AppBar with the given title text.
    ///
    /// # Parameters
    ///
    /// - `title`: Text to display in the app bar
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let app_bar = AppBar::new("My Application");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            key: None,
            title: Some(Box::new(crate::Text::new(title.into()))),
            leading: None,
            actions: vec![],
            background_color: Color::rgb(33, 150, 243),
            elevation: 4.0,
            height: 56.0,
        }
    }

    /// Creates an AppBar with default settings (no title).
    pub fn empty() -> Self {
        Self {
            key: None,
            title: None,
            leading: None,
            actions: vec![],
            background_color: Color::rgb(33, 150, 243),
            elevation: 4.0,
            height: 56.0,
        }
    }
}

impl Default for AppBar {
    fn default() -> Self {
        Self::empty()
    }
}

// bon Builder Extensions
use app_bar_builder::{IsUnset, SetTitle, State};

// Custom title setter
impl<S: State> AppBarBuilder<S>
where
    S::Title: IsUnset,
{
    /// Sets the title widget (works in builder chain).
    pub fn title(self, title: impl View + 'static) -> AppBarBuilder<SetTitle<S>> {
        self.title_internal(Box::new(title))
    }
}

// Build wrapper
impl<S: State> AppBarBuilder<S> {
    /// Builds the AppBar widget.
    pub fn build(self) -> AppBar {
        self.build_internal()
    }
}

// Implement View trait
impl View for AppBar {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        use crate::{Align, ColoredBox, Padding, PhysicalModel, Row, SizedBox};
        use flui_types::{Alignment, EdgeInsets};

        // Build the content row
        let mut row_children: Vec<Box<dyn AnyView>> = Vec::new();

        // Add leading widget (with padding)
        if let Some(leading) = self.leading {
            let padding_widget = Padding {
                key: None,
                padding: EdgeInsets::symmetric(8.0, 0.0),
                child: Some(leading),
            };
            row_children.push(Box::new(padding_widget));
        } else {
            // Add horizontal spacing if no leading widget
            row_children.push(Box::new(SizedBox::builder().width(16.0).build()));
        }

        // Add title (aligned to start, takes up remaining space)
        if let Some(title) = self.title {
            let align_widget = Align {
                key: None,
                alignment: Alignment::CENTER_LEFT,
                width_factor: None,
                height_factor: None,
                child: Some(title),
            };
            row_children.push(Box::new(crate::Expanded::new(align_widget)));
        }

        // Add actions (with padding between each)
        for action in self.actions {
            let padding_widget = Padding {
                key: None,
                padding: EdgeInsets::symmetric(8.0, 0.0),
                child: Some(action),
            };
            row_children.push(Box::new(padding_widget));
        }

        // Add trailing spacing
        row_children.push(Box::new(SizedBox::builder().width(8.0).build()));

        // Create the row
        let row = Row::builder()
            .children(row_children)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        // Wrap in SizedBox to set height
        let sized = SizedBox::builder().height(self.height).child(row).build();

        // Wrap in colored background
        let colored = ColoredBox::builder()
            .color(self.background_color)
            .child(sized)
            .build();

        // Always wrap in PhysicalModel (elevation=0 if no shadow needed)
        PhysicalModel::builder()
            .elevation(self.elevation)
            .color(self.background_color)
            .child(colored)
            .build()
    }
}

/// Macro for creating AppBar with declarative syntax.
#[macro_export]
macro_rules! app_bar {
    ($title:expr) => {
        $crate::AppBar::new($title)
    };
    () => {
        $crate::AppBar::empty()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_bar_new() {
        let widget = AppBar::new("Test");
        assert!(widget.key.is_none());
        assert!(widget.title.is_some());
        assert_eq!(widget.height, 56.0);
        assert_eq!(widget.elevation, 4.0);
    }

    #[test]
    fn test_app_bar_empty() {
        let widget = AppBar::empty();
        assert!(widget.title.is_none());
        assert!(widget.leading.is_none());
        assert!(widget.actions.is_empty());
    }

    #[test]
    fn test_app_bar_default() {
        let widget = AppBar::default();
        assert!(widget.title.is_none());
    }

    #[test]
    fn test_app_bar_builder() {
        let widget = AppBar::builder().build_app_bar();
        assert!(widget.title.is_none());
        assert_eq!(widget.height, 56.0);
    }

    #[test]
    fn test_app_bar_builder_with_title() {
        let widget = AppBar::builder()
            .title(crate::Text::new("Test"))
            .build_app_bar();
        assert!(widget.title.is_some());
    }

    #[test]
    fn test_app_bar_builder_custom() {
        let widget = AppBar::builder()
            .height(64.0)
            .elevation(8.0)
            .background_color(Color::rgb(255, 0, 0))
            .build_app_bar();
        assert_eq!(widget.height, 64.0);
        assert_eq!(widget.elevation, 8.0);
        assert_eq!(widget.background_color, Color::rgb(255, 0, 0));
    }

    #[test]
    fn test_app_bar_macro_with_title() {
        let widget = app_bar!("Test");
        assert!(widget.title.is_some());
    }

    #[test]
    fn test_app_bar_macro_empty() {
        let widget = app_bar!();
        assert!(widget.title.is_none());
    }
}
