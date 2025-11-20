//! Card widget - Material Design card
//!
//! A Material Design card with elevation and rounded corners.
//! Similar to Flutter's Card widget.

use bon::Builder;
use flui_core::element::Element;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::styling::BorderRadius;
use flui_types::{Color, EdgeInsets};

// TODO: Re-enable Material after visual_effects migration is complete
// use crate::visual_effects::Material;
use crate::Container;

/// A Material Design card.
///
/// Card is a composite widget that combines Material surface with rounded corners,
/// elevation (shadow), and optional margin/padding.
///
/// ## Key Properties
///
/// - **color**: Background color (default: white)
/// - **elevation**: Shadow depth (default: 1.0)
/// - **margin**: Outer spacing
/// - **shape**: Border radius (default: 4.0)
/// - **child**: Card content
///
/// ## Common Use Cases
///
/// ### Simple card
/// ```rust,ignore
/// Card::new(Text::new("Hello"))
/// ```
///
/// ### Card with elevation
/// ```rust,ignore
/// Card::builder()
///     .elevation(4.0)
///     .child(content)
///     .build()
/// ```
///
/// ### Colored card with margin
/// ```rust,ignore
/// Card::builder()
///     .color(Color::BLUE)
///     .margin(EdgeInsets::all(16.0))
///     .child(widget)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic card
/// Card::new(child_widget)
///
/// // Elevated card
/// Card::builder()
///     .elevation(8.0)
///     .child(content)
///     .build()
///
/// // Custom card
/// Card::builder()
///     .color(Color::rgba(255, 255, 255, 0.9))
///     .elevation(2.0)
///     .margin(EdgeInsets::symmetric(16.0, 8.0))
///     .shape(BorderRadius::circular(12.0))
///     .child(content)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Card {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Background color
    /// Default: Color::WHITE
    #[builder(default = Color::rgb(255, 255, 255))]
    pub color: Color,

    /// Elevation (shadow depth)
    /// Default: 1.0
    #[builder(default = 1.0)]
    pub elevation: f32,

    /// Outer margin
    pub margin: Option<EdgeInsets>,

    /// Border radius
    /// Default: 4.0 on all corners
    #[builder(default = BorderRadius::circular(4.0))]
    pub shape: BorderRadius,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Element>,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("key", &self.key)
            .field("color", &self.color)
            .field("elevation", &self.elevation)
            .field("margin", &self.margin)
            .field("shape", &self.shape)
            .field(
                "child",
                &if self.child.is_some() {
                    "<child>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Card {
    /// Creates a new Card with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let card = Card::new(child_widget);
    /// ```
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            key: None,
            color: Color::rgb(255, 255, 255),
            elevation: 1.0,
            margin: None,
            shape: BorderRadius::circular(4.0),
            child: Some(child.into_element()),
        }
    }

    /// Creates a Card with custom elevation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let card = Card::with_elevation(4.0, child_widget);
    /// ```
    pub fn with_elevation(elevation: f32, child: impl IntoElement) -> Self {
        Self {
            key: None,
            color: Color::rgb(255, 255, 255),
            elevation,
            margin: None,
            shape: BorderRadius::circular(4.0),
            child: Some(child.into_element()),
        }
    }
}

impl Default for Card {
    fn default() -> Self {
        Self {
            key: None,
            color: Color::rgb(255, 255, 255),
            elevation: 1.0,
            margin: None,
            shape: BorderRadius::circular(4.0),
            child: None,
        }
    }
}

// bon Builder Extensions
use card_builder::{IsUnset, SetChild, State};

impl<S: State> CardBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl IntoElement) -> CardBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

impl<S: State> CardBuilder<S> {
    /// Builds the Card widget.
    pub fn build(self) -> Card {
        self.build_internal()
    }
}

// Implement View trait
impl View for Card {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // TODO: Add elevation/shadow support when Material widget is available
        // For now, create a simple card with color and border radius
        use flui_types::styling::BoxDecoration;

        let decoration = BoxDecoration {
            color: Some(self.color),
            border_radius: Some(self.shape),
            ..Default::default()
        };

        let mut container_builder = Container::builder().decoration(decoration);

        if let Some(margin) = self.margin {
            container_builder = container_builder.margin(margin);
        }

        if let Some(child) = self.child {
            container_builder.child(child).build()
        } else {
            container_builder.build()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_new() {
        let card = Card::new(crate::SizedBox::new());
        assert_eq!(card.elevation, 1.0);
        assert_eq!(card.color, Color::rgb(255, 255, 255));
        assert!(card.child.is_some());
    }

    #[test]
    fn test_card_with_elevation() {
        let card = Card::with_elevation(4.0, crate::SizedBox::new());
        assert_eq!(card.elevation, 4.0);
    }

    #[test]
    fn test_card_builder() {
        let _card = Card::builder().elevation(2.0).color(Color::BLUE).build();
    }

    #[test]
    fn test_card_default() {
        let card = Card::default();
        assert_eq!(card.elevation, 1.0);
        assert!(card.child.is_none());
    }
}
