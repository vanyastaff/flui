//! DecoratedBox widget - paints decoration around child
//!
//! A widget that paints a Decoration either before or after its child paints.
//! Similar to Flutter's DecoratedBox widget.

use bon::Builder;
use flui_core::view::children::Child;
use flui_core::IntoElement;
use flui_rendering::objects::DecorationPosition;
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
    #[builder(default = BoxDecoration::default())]
    pub decoration: BoxDecoration,

    /// Whether to paint the decoration in foreground or background.
    #[builder(default = DecorationPosition::Background)]
    pub position: DecorationPosition,

    /// The child widget.
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
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
                    "<child>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl DecoratedBox {
    /// Creates a new empty DecoratedBox with default decoration.
    pub fn new() -> Self {
        Self {
            key: None,
            decoration: BoxDecoration::default(),
            position: DecorationPosition::Background,
            child: Child::none(),
        }
    }

    /// Creates a DecoratedBox with custom decoration and child.
    pub fn with_decoration(decoration: BoxDecoration, child: impl IntoElement) -> Self {
        Self::builder().decoration(decoration).child(child).build()
    }

    // ========== Common Decoration Patterns ==========

    /// Creates a DecoratedBox with solid color background.
    pub fn colored(color: Color, child: impl IntoElement) -> Self {
        Self::with_decoration(BoxDecoration::with_color(color), child)
    }

    /// Creates a DecoratedBox with color and rounded corners.
    pub fn rounded(color: Color, radius: f32, child: impl IntoElement) -> Self {
        let decoration = BoxDecoration::with_color(color)
            .set_border_radius(Some(flui_types::styling::BorderRadius::circular(radius)));
        Self::with_decoration(decoration, child)
    }

    /// Creates a card-style DecoratedBox with elevation shadow.
    pub fn card(child: impl IntoElement) -> Self {
        use flui_types::{
            styling::{BorderRadius, BoxShadow},
            Color, Offset,
        };

        let shadow = BoxShadow::new(Color::rgba(0, 0, 0, 25), Offset::new(0.0, 2.0), 4.0, 0.0);

        let decoration = BoxDecoration::with_color(Color::WHITE)
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_box_shadow(Some(vec![shadow]));

        Self::with_decoration(decoration, child)
    }

    /// Creates a DecoratedBox with gradient background.
    pub fn gradient(gradient: flui_types::styling::Gradient, child: impl IntoElement) -> Self {
        Self::with_decoration(BoxDecoration::with_gradient(gradient), child)
    }

    /// Creates a DecoratedBox with foreground decoration.
    pub fn foreground_colored(color: Color, child: impl IntoElement) -> Self {
        Self::builder()
            .decoration(BoxDecoration::with_color(color))
            .position(DecorationPosition::Foreground)
            .child(child)
            .build()
    }

    /// Validates DecoratedBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Default for DecoratedBox {
    fn default() -> Self {
        Self::new()
    }
}

// DecoratedBox is a RenderObjectWidget - implements IntoElement directly
impl IntoElement for DecoratedBox {
    fn into_element(self) -> flui_core::Element {
        use flui_rendering::{BoxRenderWrapper, Optional};
        use flui_rendering::objects::RenderDecoratedBox;

        // Create render object
        let render = RenderDecoratedBox::with_position(self.decoration, self.position);

        // Wrap in BoxRenderWrapper and convert to Element
        // TODO: Handle child - need to figure out proper API for this
        BoxRenderWrapper::<Optional>::new(render).into_element()
    }
}

// bon Builder Extensions
use decorated_box_builder::{IsUnset, SetChild, State};

impl<S: State> DecoratedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl IntoElement) -> DecoratedBoxBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

impl<S: State> DecoratedBoxBuilder<S> {
    /// Builds the DecoratedBox widget with automatic validation in debug mode.
    pub fn build(self) -> DecoratedBox {
        let decorated_box = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = decorated_box.validate() {
            tracing::warn!("DecoratedBox validation warning: {}", e);
        }

        decorated_box
    }
}

/// Macro for creating DecoratedBox with declarative syntax.
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

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::objects::RenderEmpty;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl StatelessView for MockView {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
        }
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
    fn test_decorated_box_default() {
        let widget = DecoratedBox::default();
        assert_eq!(widget.decoration, BoxDecoration::default());
        assert_eq!(widget.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_validate() {
        let widget = DecoratedBox::default();
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_decorated_box_macro_empty() {
        let widget = decorated_box!();
        assert_eq!(widget.decoration, BoxDecoration::default());
    }
}
