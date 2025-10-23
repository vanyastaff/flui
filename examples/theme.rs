//! Theme Example
//!
//! Demonstrates InheritedWidget for sharing theme data across the widget tree.
//!
//! This example shows:
//! - InheritedWidget implementation
//! - Data propagation down the widget tree
//! - Accessing inherited data with BuildContext
//! - Automatic dependency tracking and rebuilds
//! - Flutter-style `of()` and `maybeOf()` static methods
//!
//! The Theme widget provides color and size data to all descendants.
//! When theme data changes, only widgets that depend on it will rebuild.
//!
//! # Flutter-style API Pattern
//!
//! This example demonstrates the Flutter convention of providing static
//! `of()` and `maybeOf()` methods on InheritedWidget types:
//!
//! - `Theme::of(context)` - Required theme (panics if not found)
//! - `Theme::maybe_of(context)` - Optional theme (returns None if not found)
//!
//! This pattern provides a cleaner API compared to calling
//! `context.depend_on_inherited_widget::<Theme>()` directly.
//!
//! Run with: cargo run --example theme

use flui_app::*;
use flui_widgets::prelude::*;
use flui_widgets::DynWidget;

/// Theme data that will be shared across the widget tree
#[derive(Debug, Clone, Copy, PartialEq)]
struct ThemeData {
    /// Primary text color
    primary_color: Color,
    /// Text size
    text_size: f32,
}

impl ThemeData {
    fn light() -> Self {
        Self {
            primary_color: Color::rgb(0, 0, 0),
            text_size: 24.0,
        }
    }

    fn dark() -> Self {
        Self {
            primary_color: Color::rgb(255, 255, 255),
            text_size: 28.0,
        }
    }
}

/// Theme InheritedWidget
///
/// Provides theme data to all descendant widgets
#[derive(Debug)]
struct Theme {
    data: ThemeData,
    child: Box<dyn DynWidget>,
}

// Manual Clone implementation for Box<dyn DynWidget>
// DynWidget extends DynClone, so we can call clone() on the trait object
impl Clone for Theme {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            child: self.child.clone(),
        }
    }
}

impl Theme {
    fn new(data: ThemeData, child: Box<dyn DynWidget>) -> Self {
        Self { data, child }
    }

    /// Access the closest Theme ancestor in the widget tree (optional)
    ///
    /// Returns None if no Theme is found.
    /// This is the non-panicking version.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(theme) = Theme::maybe_of(context) {
    ///     let color = theme.data().primary_color;
    /// }
    /// ```
    pub fn maybe_of(context: &BuildContext) -> Option<Self> {
        context.depend_on_inherited_widget::<Theme>()
    }

    /// Access the closest Theme ancestor in the widget tree (required)
    ///
    /// Panics if no Theme is found in the widget tree.
    /// Use `maybe_of()` for a non-panicking version.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = Theme::of(context);
    /// let color = theme.data().primary_color;
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if no Theme ancestor is found.
    pub fn of(context: &BuildContext) -> Self {
        Self::maybe_of(context).expect("No Theme found in context. Wrap your widget tree with Theme.")
    }
}

impl ProxyWidget for Theme {
    fn child(&self) -> &dyn DynWidget {
        &*self.child
    }
}

impl InheritedWidget for Theme {
    type Data = ThemeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Notify dependents if theme data changed
        self.data != old.data
    }
}

// Manual Widget implementation for Theme
impl Widget for Theme {
    type Element = InheritedElement<Self>;

    fn into_element(self) -> Self::Element {
        InheritedElement::new(self)
    }
}

/// A widget that uses theme data
#[derive(Debug, Clone)]
struct ThemedText {
    text: String,
}

impl ThemedText {
    fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl StatelessWidget for ThemedText {
    fn build(&self, context: &BuildContext) -> Box<dyn DynWidget> {
        // Use Theme::maybe_of() for safe access (Flutter-style pattern)
        if let Some(theme) = Theme::maybe_of(context) {
            let theme_data = theme.data();

            tracing::info!(
                "ThemedText building with color: {:?}, size: {}",
                theme_data.primary_color,
                theme_data.text_size
            );

            Box::new(
                Text::builder()
                    .data(&self.text)
                    .size(theme_data.text_size)
                    .color(theme_data.primary_color)
                    .build(),
            )
        } else {
            // Fallback if no theme found
            tracing::warn!("No theme found, using default styling");
            Box::new(
                Text::builder()
                    .data(&self.text)
                    .size(16.0)
                    .color(Color::rgb(128, 128, 128))
                    .build(),
            )
        }

        // Alternative: Use Theme::of() which panics if theme not found
        // let theme = Theme::of(context);
        // let theme_data = theme.data();
        // Box::new(
        //     Text::builder()
        //         .data(&self.text)
        //         .size(theme_data.text_size)
        //         .color(theme_data.primary_color)
        //         .build(),
        // )
    }
}

/// Root application widget
#[derive(Debug, Clone)]
struct ThemeApp;

impl StatelessWidget for ThemeApp {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Wrap the app in a Theme widget - simplified without depend_on_inherited_widget for now
        Box::new(Theme::new(
            ThemeData::light(),
            Box::new(Text::builder()
                .data("Hello from InheritedWidget!")
                .size(24.0)
                .color(Color::rgb(0, 128, 255))
                .build()),
        ))
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting Theme example");
    tracing::info!("This example demonstrates InheritedWidget for theme propagation");

    // Run the app
    run_app(Box::new(ThemeApp))
}
