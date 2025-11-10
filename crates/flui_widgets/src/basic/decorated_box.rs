//! DecoratedBox widget - paints decoration around child
//!
//! A widget that paints a Decoration either before or after its child paints.
//! Similar to Flutter's DecoratedBox widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Solid color background
//! DecoratedBox::colored(Color::RED, child)
//!
//! // Rounded corners with color
//! DecoratedBox::rounded(Color::BLUE, 12.0, child)
//!
//! // Card with shadow
//! DecoratedBox::card(child)
//!
//! // Custom decoration
//! DecoratedBox::with_decoration(decoration, child)
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! DecoratedBox::builder()
//!     .decoration(BoxDecoration::default().with_color(Color::RED))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! decorated_box!(child: widget, decoration: BoxDecoration::default().with_color(Color::RED))
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::{DecorationPosition, RenderDecoratedBox};
use flui_types::styling::BoxDecoration;
use flui_types::Color;

/// A widget that paints a Decoration either before or after its child paints.
///
/// DecoratedBox paints a BoxDecoration around or behind its child.
/// Unlike Container, it does not inset the child by the border widths.
///
/// ## Layout Behavior
///
/// - Passes parent constraints directly to child
/// - Takes the size of its child
/// - Does NOT clip the child (use ClipPath for clipping)
///
/// ## Decoration Position
///
/// - `Background`: Paints decoration behind the child (default)
/// - `Foreground`: Paints decoration in front of the child
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple colored box
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_color(Color::from_rgb(255, 0, 0))
///     )
///     .child(Text::new("Hello"))
///     .build()
///
/// // Radial gradient moon on night sky
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_gradient(Gradient::radial(
///                 Alignment::new(-0.5, -0.6),  // center
///                 0.15,                         // radius
///                 vec![
///                     Color::from_rgb(238, 238, 238),
///                     Color::from_rgb(17, 17, 51),
///                 ],
///                 Some(vec![0.9, 1.0]),        // stops
///             ))
///     )
///     .build()
///
/// // With border and shadow
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_color(Color::WHITE)
///             .with_border_radius(BorderRadius::circular(12.0))
///             .with_box_shadow(BoxShadow::new(
///                 Offset::new(0.0, 4.0),
///                 8.0,  // blur
///                 Color::from_rgba(0, 0, 0, 64),
///             ))
///     )
///     .child(some_widget)
///     .build()
/// ```
///
/// ## See Also
///
/// - Container: Higher-level widget that combines decoration, padding, constraints, etc.
/// - ClipPath: For clipping child to a specific shape
/// - CustomPaint: For custom painting effects
#[derive(Builder)]
#[builder(
    on(String, into),
    on(BoxDecoration, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct DecoratedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The decoration to paint.
    ///
    /// Use BoxDecoration to specify color, gradient, border, border radius,
    /// box shadows, and background image.
    #[builder(default = BoxDecoration::default())]
    pub decoration: BoxDecoration,

    /// Whether to paint the decoration in foreground or background.
    ///
    /// - Background: Paint decoration behind child (default)
    /// - Foreground: Paint decoration in front of child
    #[builder(default = DecorationPosition::Background)]
    pub position: DecorationPosition,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for DecoratedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoratedBox")
            .field("key", &self.key)
            .field("decoration", &self.decoration)
            .field("position", &self.position)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for DecoratedBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            decoration: self.decoration.clone(),
            position: self.position,
            child: self.child.clone(),
        }
    }
}

impl DecoratedBox {
    /// Creates a new empty DecoratedBox with default decoration.
    ///
    /// Note: Prefer using convenience methods like `DecoratedBox::colored()` for most cases.
    pub fn new() -> Self {
        Self {
            key: None,
            decoration: BoxDecoration::default(),
            position: DecorationPosition::Background,
            child: None,
        }
    }

    /// Creates a DecoratedBox with custom decoration and child.
    ///
    /// Use this when you have a pre-built BoxDecoration.
    ///
    /// # Example
    /// ```rust,ignore
    /// let decoration = BoxDecoration::with_color(Color::RED)
    ///     .set_border_radius(Some(BorderRadius::circular(8.0)));
    /// DecoratedBox::with_decoration(decoration, child)
    /// ```
    pub fn with_decoration(decoration: BoxDecoration, child: impl View + 'static) -> Self {
        Self::builder().decoration(decoration).child(child).build()
    }

    // ========== Common Decoration Patterns ==========

    /// Creates a DecoratedBox with solid color background.
    ///
    /// Most common use case - simple colored background.
    ///
    /// # Example
    /// ```rust,ignore
    /// DecoratedBox::colored(Color::BLUE, Text::new("Hello"))
    /// ```
    pub fn colored(color: Color, child: impl View + 'static) -> Self {
        Self::with_decoration(BoxDecoration::with_color(color), child)
    }

    /// Creates a DecoratedBox with color and rounded corners.
    ///
    /// Perfect for buttons, cards, and rounded elements.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Blue rounded box with 12px radius
    /// DecoratedBox::rounded(Color::BLUE, 12.0, content)
    /// ```
    pub fn rounded(color: Color, radius: f32, child: impl View + 'static) -> Self {
        let decoration = BoxDecoration::with_color(color)
            .set_border_radius(Some(flui_types::styling::BorderRadius::circular(radius)));
        Self::with_decoration(decoration, child)
    }

    /// Creates a card-style DecoratedBox with elevation shadow.
    ///
    /// Material Design card with white background, rounded corners, and shadow.
    ///
    /// # Example
    /// ```rust,ignore
    /// DecoratedBox::card(content)
    /// ```
    pub fn card(child: impl View + 'static) -> Self {
        use flui_types::{
            styling::{BorderRadius, BoxShadow},
            Color, Offset,
        };

        let shadow = BoxShadow::new(
            Color::rgba(0, 0, 0, 25),
            Offset::new(0.0, 2.0),
            4.0, // blur_radius
            0.0, // spread_radius
        );

        let decoration = BoxDecoration::with_color(Color::WHITE)
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_box_shadow(Some(vec![shadow]));

        Self::with_decoration(decoration, child)
    }

    /// Creates a DecoratedBox with gradient background.
    ///
    /// # Example
    /// ```rust,ignore
    /// use flui_types::styling::Gradient;
    /// let gradient = Gradient::linear(...);
    /// DecoratedBox::gradient(gradient, child)
    /// ```
    pub fn gradient(gradient: flui_types::styling::Gradient, child: impl View + 'static) -> Self {
        Self::with_decoration(BoxDecoration::with_gradient(gradient), child)
    }

    /// Creates a DecoratedBox with foreground decoration.
    ///
    /// The decoration will be painted in front of the child,
    /// useful for overlays or masks.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Semi-transparent overlay
    /// DecoratedBox::foreground_colored(
    ///     Color::rgba(0, 0, 0, 128),
    ///     image_widget
    /// )
    /// ```
    pub fn foreground_colored(color: Color, child: impl View + 'static) -> Self {
        Self::builder()
            .decoration(BoxDecoration::with_color(color))
            .position(DecorationPosition::Foreground)
            .child(child)
            .build()
    }

    /// Validates DecoratedBox configuration.
    ///
    /// Currently no validation needed for DecoratedBox,
    /// but provided for consistency with other widgets.
    pub fn validate(&self) -> Result<(), String> {
        // BoxDecoration validates itself
        Ok(())
    }
}

impl Default for DecoratedBox {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View for DecoratedBox - New architecture
impl View for DecoratedBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let child = self
            .child
            .or_else(|| Some(Box::new(crate::SizedBox::new())));
        (RenderDecoratedBox::with_position(
            self.decoration.clone(),
            self.position,
        ), child)
    }
}

// bon Builder Extensions
use decorated_box_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> DecoratedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// DecoratedBox::builder()
    ///     .decoration(BoxDecoration::default().with_color(Color::RED))
    ///     .child(Text::new("Hello"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> DecoratedBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Public build() wrapper
impl<S: State> DecoratedBoxBuilder<S> {
    /// Builds the DecoratedBox widget with automatic validation in debug mode.
    pub fn build(self) -> DecoratedBox {
        let decorated_box = self.build_internal();

        // In debug mode, validate configuration and warn on issues
        #[cfg(debug_assertions)]
        if let Err(e) = decorated_box.validate() {
            tracing::warn!("DecoratedBox validation warning: {}", e);
        }

        decorated_box
    }
}

/// Macro for creating DecoratedBox with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty decorated box
/// decorated_box!()
///
/// // With child only (default decoration)
/// decorated_box!(child: Text::new("Hello"))
///
/// // With child and decoration
/// decorated_box!(child: widget, decoration: BoxDecoration::default().with_color(Color::RED))
///
/// // Properties only (no child)
/// decorated_box!(decoration: BoxDecoration::default().with_color(Color::RED))
/// ```
#[macro_export]
macro_rules! decorated_box {
    // Empty decorated box
    () => {
        $crate::DecoratedBox::new()
    };

    // With child only (default decoration)
    (child: $child:expr) => {
        $crate::DecoratedBox::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::DecoratedBox::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::DecoratedBox {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

// DecoratedBox now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    use flui_rendering::RenderPadding;
    use flui_types::styling::{BorderRadius, BoxShadow, Gradient, LinearGradient, TileMode};
    use flui_types::{Alignment, Color, EdgeInsets, Offset};

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            (RenderPadding::new(EdgeInsets::ZERO), ())}
    }

    #[test]
    fn test_decorated_box_new() {
        let widget = DecoratedBox::new();
        assert!(widget.key.is_none());
        assert_eq!(widget.decoration, BoxDecoration::default());
        assert_eq!(widget.position, DecorationPosition::Background);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_decorated_box_colored() {
        let widget = DecoratedBox::colored(Color::RED, MockView);
        assert_eq!(widget.decoration, BoxDecoration::with_color(Color::RED));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_rounded() {
        let widget = DecoratedBox::rounded(Color::BLUE, 12.0, MockView);
        assert_eq!(widget.decoration.color, Some(Color::BLUE));
        assert!(widget.decoration.border_radius.is_some());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_card() {
        let widget = DecoratedBox::card(MockView);
        assert_eq!(widget.decoration.color, Some(Color::WHITE));
        assert!(widget.decoration.border_radius.is_some());
        assert!(widget.decoration.box_shadow.is_some());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_foreground_colored() {
        let widget = DecoratedBox::foreground_colored(Color::rgba(0, 0, 0, 128), MockView);
        assert_eq!(widget.position, DecorationPosition::Foreground);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_default() {
        let widget = DecoratedBox::default();
        assert_eq!(widget.decoration, BoxDecoration::default());
        assert_eq!(widget.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_builder() {
        let decoration = BoxDecoration::with_color(Color::rgb(255, 0, 0));

        let widget = DecoratedBox::builder()
            .decoration(decoration.clone())
            .build();

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_builder_with_position() {
        let decoration = BoxDecoration::with_color(Color::GREEN);

        let widget = DecoratedBox::builder()
            .decoration(decoration.clone())
            .position(DecorationPosition::Foreground)
            .build();

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_struct_literal() {
        let decoration = BoxDecoration::with_color(Color::YELLOW);

        let widget = DecoratedBox {
            decoration: decoration.clone(),
            position: DecorationPosition::Foreground,
            ..Default::default()
        };

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_with_decoration() {
        let decoration = BoxDecoration::with_color(Color::RED)
            .set_border_radius(Some(BorderRadius::circular(8.0)));
        let widget = DecoratedBox::with_decoration(decoration.clone(), MockView);
        assert_eq!(widget.decoration, decoration);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_gradient() {
        let gradient = Gradient::Linear(LinearGradient::new(
            Alignment::TOP_LEFT,
            Alignment::BOTTOM_RIGHT,
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        ));

        let widget = DecoratedBox::gradient(gradient.clone(), MockView);
        assert_eq!(widget.decoration, BoxDecoration::with_gradient(gradient));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_builder_with_child() {
        let decoration = BoxDecoration::with_color(Color::WHITE);

        let widget = DecoratedBox::builder()
            .decoration(decoration.clone())
            .child(MockView)
            .build();

        assert_eq!(widget.decoration, decoration);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_validate() {
        let widget = DecoratedBox::default();
        assert!(widget.validate().is_ok());

        let widget = DecoratedBox::colored(Color::RED, MockView);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_decorated_box_macro_empty() {
        let widget = decorated_box!();
        assert_eq!(widget.decoration, BoxDecoration::default());
    }

    #[test]
    fn test_decorated_box_macro_with_child() {
        let widget = decorated_box!(child: MockView);
        assert!(widget.child.is_some());
        assert_eq!(widget.decoration, BoxDecoration::default());
    }

    #[test]
    fn test_decorated_box_macro_with_child_and_decoration() {
        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = decorated_box!(child: MockView, decoration: decoration.clone());
        assert!(widget.child.is_some());
        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_decorated_box_macro_with_decoration() {
        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = decorated_box! {
            decoration: decoration.clone(),
        };
        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_all_convenience_methods() {
        // Test that all convenience methods create widgets with children
        assert!(DecoratedBox::colored(Color::RED, MockView).child.is_some());
        assert!(DecoratedBox::rounded(Color::BLUE, 12.0, MockView)
            .child
            .is_some());
        assert!(DecoratedBox::card(MockView).child.is_some());
        assert!(
            DecoratedBox::foreground_colored(Color::rgba(0, 0, 0, 128), MockView)
                .child
                .is_some()
        );

        // Verify positions
        assert_eq!(
            DecoratedBox::foreground_colored(Color::BLACK, MockView).position,
            DecorationPosition::Foreground
        );
        assert_eq!(
            DecoratedBox::colored(Color::BLACK, MockView).position,
            DecorationPosition::Background
        );
    }
}
