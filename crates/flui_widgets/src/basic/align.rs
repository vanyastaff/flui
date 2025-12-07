//! Align widget - aligns its child within itself
//!
//! A widget that aligns its child within the available space.
//! Similar to Flutter's Align widget.

use bon::Builder;
use flui_core::view::children::Child;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::objects::RenderAlign;
use flui_types::Alignment;

/// A widget that aligns its child within the available space.
///
/// Align positions its child at a specific position within itself using an Alignment.
///
/// ## Layout Behavior
///
/// - The child is positioned according to the alignment
/// - Alignment coordinates: (-1, -1) = top-left, (0, 0) = center, (1, 1) = bottom-right
/// - If `width_factor` or `height_factor` are specified, the Align sizes itself
///   as a multiple of the child's size
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Align {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to align the child within the available space.
    ///
    /// Defaults to Alignment::CENTER if not specified.
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// Multiplier for child width to determine Align width.
    pub width_factor: Option<f32>,

    /// Multiplier for child height to determine Align height.
    pub height_factor: Option<f32>,

    /// The child widget to align.
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

impl std::fmt::Debug for Align {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Align")
            .field("key", &self.key)
            .field("alignment", &self.alignment)
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
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

impl Align {
    /// Creates a new empty Align widget with center alignment.
    pub fn new() -> Self {
        Self {
            key: None,
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
            child: Child::none(),
        }
    }

    /// Creates an Align with custom alignment and child.
    pub fn with_alignment(alignment: Alignment, child: impl IntoElement) -> Self {
        Self::builder().alignment(alignment).child(child).build()
    }

    // ========== 9 Standard Alignment Presets ==========

    /// Aligns child to top-left corner.
    pub fn top_left(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::TOP_LEFT, child)
    }

    /// Aligns child to top-center.
    pub fn top_center(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::TOP_CENTER, child)
    }

    /// Aligns child to top-right corner.
    pub fn top_right(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::TOP_RIGHT, child)
    }

    /// Aligns child to center-left.
    pub fn center_left(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::CENTER_LEFT, child)
    }

    /// Aligns child to center.
    pub fn center(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::CENTER, child)
    }

    /// Aligns child to center-right.
    pub fn center_right(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::CENTER_RIGHT, child)
    }

    /// Aligns child to bottom-left corner.
    pub fn bottom_left(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::BOTTOM_LEFT, child)
    }

    /// Aligns child to bottom-center.
    pub fn bottom_center(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::BOTTOM_CENTER, child)
    }

    /// Aligns child to bottom-right corner.
    pub fn bottom_right(child: impl IntoElement) -> Self {
        Self::with_alignment(Alignment::BOTTOM_RIGHT, child)
    }

    /// Validates Align configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width_factor) = self.width_factor {
            if width_factor <= 0.0 || width_factor.is_nan() || width_factor.is_infinite() {
                return Err(format!(
                    "Invalid width_factor: {}. Must be positive and finite.",
                    width_factor
                ));
            }
        }

        if let Some(height_factor) = self.height_factor {
            if height_factor <= 0.0 || height_factor.is_nan() || height_factor.is_infinite() {
                return Err(format!(
                    "Invalid height_factor: {}. Must be positive and finite.",
                    height_factor
                ));
            }
        }

        Ok(())
    }
}

impl Default for Align {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use align_builder::{IsUnset, SetChild, State};

impl<S: State> AlignBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl IntoElement) -> AlignBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

impl<S: State> AlignBuilder<S> {
    /// Builds the Align widget with automatic validation in debug mode.
    pub fn build(self) -> Align {
        let align = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = align.validate() {
            tracing::warn!("Align validation warning: {}", e);
        }

        align
    }
}

/// Macro for creating Align with declarative syntax.
#[macro_export]
macro_rules! align {
    // Empty align
    () => {
        $crate::Align::new()
    };

    // With child only (center alignment)
    (child: $child:expr) => {
        $crate::Align::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::Align::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::Align {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

// Implement View for Align
impl StatelessView for Align {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderAlign::with_factors(self.alignment, self.width_factor, self.height_factor)
            .maybe_child(self.child)
    }
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
    fn test_align_new() {
        let align = Align::new();
        assert!(align.key.is_none());
        assert_eq!(align.alignment, Alignment::CENTER);
        assert!(align.width_factor.is_none());
        assert!(align.height_factor.is_none());
        assert!(align.child.is_none());
    }

    #[test]
    fn test_align_default() {
        let align = Align::default();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_align_builder() {
        let align = Align::builder().alignment(Alignment::TOP_LEFT).build();
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_align_builder_with_factors() {
        let align = Align::builder()
            .width_factor(2.0)
            .height_factor(1.5)
            .build();
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));
    }

    #[test]
    fn test_align_validate_ok() {
        let align = Align::builder().width_factor(1.5).build();
        assert!(align.validate().is_ok());
    }

    #[test]
    fn test_align_validate_invalid_width_factor() {
        let align = Align {
            width_factor: Some(-1.0),
            ..Default::default()
        };
        assert!(align.validate().is_err());
    }

    #[test]
    fn test_align_macro_empty() {
        let align = align!();
        assert_eq!(align.alignment, Alignment::CENTER);
    }
}
