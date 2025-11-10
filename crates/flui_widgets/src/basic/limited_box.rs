//! LimitedBox widget - limits max dimensions when unconstrained
//!
//! A widget that limits its maximum size, but only when unconstrained.
//! Similar to Flutter's LimitedBox widget.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderLimitedBox;

/// A widget that limits its maximum size when unconstrained.
///
/// LimitedBox is useful when you want to constrain a child that might become
/// infinitely large in an unbounded context (like ListView or Row).
///
/// ## Layout Behavior
///
/// LimitedBox only applies its limits when the incoming constraints are infinite:
/// - If width constraint is infinite: Uses maxWidth
/// - If height constraint is infinite: Uses maxHeight
/// - If constraints are already bounded: Passes them through unchanged
///
/// This makes LimitedBox different from ConstrainedBox, which always applies its constraints.
///
/// ## Common Use Cases
///
/// ### Limit unbounded children
/// ```rust,ignore
/// // In a Row (unbounded width), prevent child from becoming infinitely wide
/// Row::new()
///     .children(vec![
///         LimitedBox::builder()
///             .max_width(200.0)
///             .child(UnboundedWidget::new())
///             .build()
///     ])
/// ```
///
/// ### ListView items
/// ```rust,ignore
/// // Ensure list items have reasonable max size
/// ListView::builder()
///     .item_builder(|index| {
///         LimitedBox::builder()
///             .max_height(100.0)
///             .child(ListItem::new(index))
///             .build()
///     })
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Limit both dimensions
/// LimitedBox::builder()
///     .max_width(300.0)
///     .max_height(200.0)
///     .child(flexible_widget)
///     .build()
///
/// // Limit only width
/// LimitedBox::builder()
///     .max_width(400.0)
///     .child(some_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct LimitedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Maximum width when unconstrained.
    /// Default: f32::INFINITY (no limit)
    #[builder(default = f32::INFINITY)]
    pub max_width: f32,

    /// Maximum height when unconstrained.
    /// Default: f32::INFINITY (no limit)
    #[builder(default = f32::INFINITY)]
    pub max_height: f32,

    /// The child widget to limit.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for LimitedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LimitedBox")
            .field("key", &self.key)
            .field("max_width", &self.max_width)
            .field("max_height", &self.max_height)
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

impl Clone for LimitedBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            max_width: self.max_width,
            max_height: self.max_height,
            child: self.child.clone(),
        }
    }
}

impl LimitedBox {
    /// Creates a new LimitedBox with the given max dimensions.
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            key: None,
            max_width,
            max_height,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates LimitedBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_width < 0.0 || self.max_width.is_nan() {
            return Err(format!("Invalid max_width: {}", self.max_width));
        }
        if self.max_height < 0.0 || self.max_height.is_nan() {
            return Err(format!("Invalid max_height: {}", self.max_height));
        }
        Ok(())
    }
}

// bon Builder Extensions
use limited_box_builder::{IsUnset, SetChild, State};

impl<S: State> LimitedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> LimitedBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> LimitedBoxBuilder<S> {
    /// Builds the LimitedBox widget.
    pub fn build(self) -> LimitedBox {
        self.build_internal()
    }
}

// Implement View for LimitedBox - New architecture
impl View for LimitedBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        (RenderLimitedBox::new(self.max_width, self.max_height), self.child)
    }
}

// LimitedBox now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limited_box_new() {
        let widget = LimitedBox::new(100.0, 200.0);
        assert_eq!(widget.max_width, 100.0);
        assert_eq!(widget.max_height, 200.0);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_limited_box_builder() {
        let widget = LimitedBox::builder()
            .max_width(150.0)
            .max_height(250.0)
            .build();
        assert_eq!(widget.max_width, 150.0);
        assert_eq!(widget.max_height, 250.0);
    }

    #[test]
    fn test_limited_box_default_infinity() {
        let widget = LimitedBox::builder().build();
        assert!(widget.max_width.is_infinite());
        assert!(widget.max_height.is_infinite());
    }

    #[test]
    fn test_limited_box_validate() {
        let widget = LimitedBox::new(100.0, 200.0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_limited_box_validate_invalid_width() {
        let widget = LimitedBox::new(-1.0, 200.0);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_limited_box_validate_invalid_height() {
        let widget = LimitedBox::new(100.0, f32::NAN);
        assert!(widget.validate().is_err());
    }
}
