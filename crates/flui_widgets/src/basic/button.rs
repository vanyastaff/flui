//! Button widget for user interactions
//!
//! A clickable button with customizable styling and tap callback.

use std::sync::Arc;

use flui_core::widget::{Widget, StatelessWidget};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets, events::PointerEventData};
use flui_types::styling::{BorderRadius, BoxDecoration};

use crate::{Container, GestureDetector};

/// Callback for button tap events
pub type ButtonCallback = Arc<dyn Fn() + Send + Sync>;

/// Button widget
///
/// A Material Design-inspired button that responds to taps.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::prelude::*;
///
/// Button::builder("Click me")
///     .on_tap(|| println!("Button tapped!"))
///     .build()
/// ```
#[derive(Clone)]
pub struct Button {
    /// Button label text (simplified - just a string for now)
    pub label: String,

    /// Callback when button is tapped
    pub on_tap: Option<ButtonCallback>,

    /// Background color
    pub color: Color,

    /// Button padding
    pub padding: EdgeInsets,

    /// Border radius
    pub border_radius: BorderRadius,

    /// Minimum width
    pub min_width: Option<f32>,

    /// Minimum height
    pub min_height: Option<f32>,
}

impl Button {
    /// Create a new Button with a label
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_tap: None,
            color: Color::rgb(33, 150, 243), // Material Blue
            padding: EdgeInsets::symmetric(16.0, 8.0),
            border_radius: BorderRadius::circular(4.0),
            min_width: Some(88.0),
            min_height: Some(36.0),
        }
    }

    /// Builder for Button
    pub fn builder(label: impl Into<String>) -> ButtonBuilder {
        ButtonBuilder::new(label)
    }
}

impl StatelessWidget for Button {
    fn build(&self, _context: &BuildContext) -> Widget {
        // Create the visual container
        let container = Container::builder()
            .padding(self.padding)
            .decoration(BoxDecoration {
                color: Some(self.color),
                border_radius: Some(self.border_radius),
                ..Default::default()
            })
            .build();

        // Wrap in GestureDetector for tap handling
        if let Some(on_tap) = &self.on_tap {
            let on_tap_clone = Arc::clone(on_tap);
            Widget::GestureDetector(
                GestureDetector::builder()
                    .child(Widget::Container(container))
                    .on_tap(move |_data: &PointerEventData| {
                        on_tap_clone();
                    })
                    .build()
            )
        } else {
            Widget::Container(container)
        }
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("color", &self.color)
            .field("has_callback", &self.on_tap.is_some())
            .finish()
    }
}

/// Builder for Button widget
pub struct ButtonBuilder {
    label: String,
    on_tap: Option<ButtonCallback>,
    color: Color,
    padding: EdgeInsets,
    border_radius: BorderRadius,
    min_width: Option<f32>,
    min_height: Option<f32>,
}

impl ButtonBuilder {
    /// Create a new ButtonBuilder
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_tap: None,
            color: Color::rgb(33, 150, 243),
            padding: EdgeInsets::symmetric(16.0, 8.0),
            border_radius: BorderRadius::circular(4.0),
            min_width: Some(88.0),
            min_height: Some(36.0),
        }
    }

    /// Set the tap callback
    pub fn on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the background color
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Set the padding
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Set the border radius
    pub fn border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Set the minimum width
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Set the minimum height
    pub fn min_height(mut self, height: f32) -> Self {
        self.min_height = Some(height);
        self
    }

    /// Build the Button
    pub fn build(self) -> Button {
        Button {
            label: self.label,
            on_tap: self.on_tap,
            color: self.color,
            padding: self.padding,
            border_radius: self.border_radius,
            min_width: self.min_width,
            min_height: self.min_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_button_creation() {
        let button = Button::new("Click me");
        assert_eq!(button.label, "Click me");
        assert!(button.on_tap.is_none());
    }

    #[test]
    fn test_button_builder() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let button = Button::builder("Test")
            .on_tap(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            })
            .color(Color::rgb(255, 0, 0))
            .build();

        assert_eq!(button.label, "Test");
        assert_eq!(button.color, Color::rgb(255, 0, 0));
        assert!(button.on_tap.is_some());

        // Call the callback
        if let Some(on_tap) = &button.on_tap {
            on_tap();
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn test_button_custom_styling() {
        let button = Button::builder("Styled")
            .color(Color::rgb(100, 200, 50))
            .padding(EdgeInsets::all(20.0))
            .border_radius(BorderRadius::circular(8.0))
            .min_width(120.0)
            .min_height(48.0)
            .build();

        assert_eq!(button.color, Color::rgb(100, 200, 50));
        assert_eq!(button.padding, EdgeInsets::all(20.0));
        assert_eq!(button.min_width, Some(120.0));
        assert_eq!(button.min_height, Some(48.0));
    }
}
