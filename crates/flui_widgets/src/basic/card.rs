//! Card widget - Material Design card
//!
//! A Material Design card with elevation and rounded corners.
//! Similar to Flutter's Card widget.

use bon::Builder;
use flui_core::view::{AnyView, ChangeFlags, View};
use flui_core::{BuildContext, Element};
use flui_types::{Color, EdgeInsets};
use flui_types::styling::{BorderRadius, BoxDecoration, BoxShadow};

use crate::{Container, DecoratedBox};

/// A Material Design card.
///
/// Card is a composite widget that combines DecoratedBox with rounded corners,
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
#[builder(on(String, into), finish_fn = build_card)]
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
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("key", &self.key)
            .field("color", &self.color)
            .field("elevation", &self.elevation)
            .field("margin", &self.margin)
            .field("shape", &self.shape)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for Card {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            color: self.color,
            elevation: self.elevation,
            margin: self.margin,
            shape: self.shape.clone(),
            child: self.child.clone(),
        }
    }
}

impl Card {
    /// Creates a new Card with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let card = Card::new(Box::new(child));
    /// ```
    pub fn new(child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            color: Color::rgb(255, 255, 255),
            elevation: 1.0,
            margin: None,
            shape: BorderRadius::circular(4.0),
            child: Some(child),
        }
    }

    /// Creates a Card with custom elevation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let card = Card::with_elevation(4.0, Box::new(child));
    /// ```
    pub fn with_elevation(elevation: f32, child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            color: Color::rgb(255, 255, 255),
            elevation,
            margin: None,
            shape: BorderRadius::circular(4.0),
            child: Some(child),
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
    pub fn child(self, child: impl View + 'static) -> CardBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Implement View trait
impl View for Card {
    type Element = Element;
    type State = Box<dyn std::any::Any>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Calculate shadow based on elevation
        let shadows = if self.elevation > 0.0 {
            vec![
                BoxShadow {
                    color: Color::rgba(0, 0, 0, (0.2 * self.elevation.min(10.0) / 10.0) as u8),
                    offset: flui_types::Offset::new(0.0, self.elevation * 0.5),
                    blur_radius: self.elevation * 2.0,
                    spread_radius: 0.0,
                    inset: false,
                },
            ]
        } else {
            vec![]
        };

        let decoration = BoxDecoration {
            color: Some(self.color),
            border_radius: Some(self.shape.clone()),
            box_shadow: if shadows.is_empty() { None } else { Some(shadows) },
            ..Default::default()
        };

        let child_view: Box<dyn AnyView> = if let Some(child) = self.child {
            child
        } else {
            Box::new(crate::SizedBox::new())
        };

        let mut decorated = DecoratedBox::builder()
            .decoration(decoration)
            .build();
        decorated.child = Some(child_view);

        // Wrap with margin if specified
        let final_view: Box<dyn AnyView> = if let Some(margin) = self.margin {
            let mut container = Container::builder()
                .margin(margin)
                .build_container();
            container.child = Some(Box::new(decorated));
            Box::new(container)
        } else {
            Box::new(decorated)
        };

        // Build the final view
        let (boxed_element, state) = final_view.build_any(ctx);
        let element = boxed_element.into_element();
        (element, state)
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        _element: &mut Self::Element,
    ) -> ChangeFlags {
        // Check if any properties changed
        let properties_changed = self.color != prev.color
            || self.elevation != prev.elevation
            || self.margin != prev.margin
            || self.shape != prev.shape;

        if properties_changed {
            // Properties changed - need to rebuild
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_new() {
        let card = Card::new(Box::new(crate::SizedBox::new()));
        assert_eq!(card.elevation, 1.0);
        assert_eq!(card.color, Color::rgb(255, 255, 255));
        assert!(card.child.is_some());
    }

    #[test]
    fn test_card_with_elevation() {
        let card = Card::with_elevation(4.0, Box::new(crate::SizedBox::new()));
        assert_eq!(card.elevation, 4.0);
    }

    #[test]
    fn test_card_builder() {
        let _card = Card::builder()
            .elevation(2.0)
            .color(Color::BLUE)
            .build_card();
    }

    #[test]
    fn test_card_default() {
        let card = Card::default();
        assert_eq!(card.elevation, 1.0);
        assert!(card.child.is_none());
    }
}
