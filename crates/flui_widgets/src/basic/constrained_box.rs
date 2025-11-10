//! ConstrainedBox widget - applies additional constraints to its child
//!
//! A widget that imposes additional constraints on its child.
//! Similar to Flutter's ConstrainedBox widget.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderConstrainedBox;
use flui_types::BoxConstraints;

/// A widget that imposes additional constraints on its child.
///
/// This widget applies its constraints to its child, combining them with any
/// constraints the parent widget provides.
///
/// ## Layout Behavior
///
/// ConstrainedBox takes the intersection of its constraints and the constraints
/// from its parent. The child is then laid out with these tightened constraints.
///
/// ## Examples
///
/// ```rust,ignore
/// // Ensure child is at least 100x100
/// ConstrainedBox::builder()
///     .constraints(BoxConstraints::new(100.0, f32::INFINITY, 100.0, f32::INFINITY))
///     .child(flexible_widget)
///     .build()
///
/// // Ensure child is exactly 200x200
/// ConstrainedBox::builder()
///     .constraints(BoxConstraints::tight(Size::new(200.0, 200.0)))
///     .child(some_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(BoxConstraints, into), finish_fn(name = build_internal, vis = ""))]
pub struct ConstrainedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The additional constraints to impose on the child.
    /// If None, uses unconstrained (equivalent to no ConstrainedBox).
    pub constraints: Option<BoxConstraints>,

    /// The child widget to constrain.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for ConstrainedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstrainedBox")
            .field("key", &self.key)
            .field("constraints", &self.constraints)
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

impl Clone for ConstrainedBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            constraints: self.constraints,
            child: self.child.clone(),
        }
    }
}

impl ConstrainedBox {
    /// Creates a new ConstrainedBox with the given constraints.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            key: None,
            constraints: Some(constraints),
            child: None,
        }
    }

    /// Sets the child widget.
    #[deprecated(note = "Use builder pattern with .child() instead")]
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates ConstrainedBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(constraints) = &self.constraints {
            if constraints.min_width < 0.0 || constraints.min_width.is_nan() {
                return Err(format!("Invalid min_width: {}", constraints.min_width));
            }
            if constraints.max_width < 0.0 || constraints.max_width.is_nan() {
                return Err(format!("Invalid max_width: {}", constraints.max_width));
            }
            if constraints.min_height < 0.0 || constraints.min_height.is_nan() {
                return Err(format!("Invalid min_height: {}", constraints.min_height));
            }
            if constraints.max_height < 0.0 || constraints.max_height.is_nan() {
                return Err(format!("Invalid max_height: {}", constraints.max_height));
            }
            if constraints.min_width > constraints.max_width {
                return Err("min_width cannot be greater than max_width".to_string());
            }
            if constraints.min_height > constraints.max_height {
                return Err("min_height cannot be greater than max_height".to_string());
            }
        }
        Ok(())
    }
}

// bon Builder Extensions
use constrained_box_builder::{IsUnset, SetChild, State};

impl<S: State> ConstrainedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> ConstrainedBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> ConstrainedBoxBuilder<S> {
    /// Builds the ConstrainedBox widget with optional validation.
    pub fn build(self) -> ConstrainedBox {
        let constrained_box = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = constrained_box.validate() {
            tracing::warn!("ConstrainedBox validation failed: {}", e);
        }

        constrained_box
    }
}

// Implement View for ConstrainedBox - New architecture
impl View for ConstrainedBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let constraints = self.constraints.unwrap_or(BoxConstraints::UNCONSTRAINED);
        RenderBuilder::new(RenderConstrainedBox::new(constraints))
            .maybe_child(self.child)
    }
}

// ConstrainedBox now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constrained_box_new() {
        let constraints = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
        let widget = ConstrainedBox::new(constraints);
        assert_eq!(widget.constraints, Some(constraints));
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_constrained_box_builder() {
        let constraints = BoxConstraints::tight_for(Some(100.0), Some(100.0));
        let widget = ConstrainedBox::builder().constraints(constraints).build();
        assert_eq!(widget.constraints, Some(constraints));
    }

    #[test]
    fn test_constrained_box_validate() {
        let widget = ConstrainedBox::new(BoxConstraints::new(100.0, 200.0, 100.0, 200.0));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_constrained_box_validate_invalid_min_width() {
        let widget = ConstrainedBox::new(BoxConstraints::new(-1.0, 200.0, 100.0, 200.0));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_constrained_box_validate_min_greater_than_max() {
        let widget = ConstrainedBox::new(BoxConstraints::new(300.0, 200.0, 100.0, 200.0));
        assert!(widget.validate().is_err());
    }
}
