//! AspectRatio widget - sizes child to a specific aspect ratio
//!
//! A widget that attempts to size its child to a specific aspect ratio.
//! Similar to Flutter's AspectRatio widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! AspectRatio {
//!     aspect_ratio: 16.0 / 9.0,
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! AspectRatio::builder()
//!     .aspect_ratio(16.0 / 9.0)
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! aspect_ratio! {
//!     aspect_ratio: 1.0,
//! }
//! ```

use bon::Builder;
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Child;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::RenderAspectRatio;

/// A widget that sizes its child to a specific aspect ratio.
///
/// AspectRatio attempts to size its child to match a specific aspect ratio (width / height).
/// This is commonly used for images, videos, and other media where maintaining proportions
/// is important.
///
/// ## Layout Behavior
///
/// The aspect ratio is expressed as the ratio of width to height:
/// - 16/9 = 1.777... (widescreen)
/// - 4/3 = 1.333... (classic TV)
/// - 1/1 = 1.0 (square)
/// - 9/16 = 0.5625 (vertical video)
///
/// ### Sizing Algorithm
///
/// 1. If width is constrained (finite max width): height = width / aspectRatio
/// 2. If height is constrained (finite max height): width = height * aspectRatio
/// 3. If both are constrained: choose the smaller size that fits
/// 4. If neither is constrained: error (cannot size to aspect ratio)
///
/// ## Examples
///
/// ```rust,ignore
/// // 16:9 widescreen video
/// AspectRatio::builder()
///     .aspect_ratio(16.0 / 9.0)
///     .child(VideoPlayer::new(url))
///     .build()
///
/// // Square image
/// AspectRatio::builder()
///     .aspect_ratio(1.0)
///     .child(Image::network(url))
///     .build()
///
/// // 3:2 photo aspect ratio
/// AspectRatio::builder()
///     .aspect_ratio(3.0 / 2.0)
///     .child(photo_widget)
///     .build()
/// ```
///
/// ## Common Aspect Ratios
///
/// - **1.0** - Square (1:1)
/// - **1.333** - Classic TV (4:3)
/// - **1.5** - 3:2 (common photo)
/// - **1.777** - Widescreen (16:9)
/// - **2.35** - Cinemascope (21:9)
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct AspectRatio {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The aspect ratio to maintain (width / height).
    ///
    /// Must be positive and finite.
    /// - Values > 1.0 are landscape (wider than tall)
    /// - Values < 1.0 are portrait (taller than wide)
    /// - Value = 1.0 is square
    #[builder(default = 1.0)]
    pub aspect_ratio: f32,

    /// The child widget.
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

// Manual Debug implementation since  doesn't implement Debug
impl std::fmt::Debug for AspectRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AspectRatio")
            .field("key", &self.key)
            .field("aspect_ratio", &self.aspect_ratio)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
    }
}

impl AspectRatio {
    /// Creates a new AspectRatio widget.
    ///
    /// # Arguments
    ///
    /// * `aspect_ratio` - The aspect ratio (width / height) to maintain
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 16:9 widescreen
    /// let widget = AspectRatio::new(16.0 / 9.0);
    ///
    /// // Square
    /// let widget = AspectRatio::new(1.0);
    /// ```
    pub fn new(aspect_ratio: f32) -> Self {
        Self {
            key: None,
            aspect_ratio,
            child: Child::none(),
        }
    }

    /// Creates an AspectRatio with widescreen 16:9 ratio.
    ///
    /// Common for videos and modern displays.
    pub fn widescreen() -> Self {
        Self::new(16.0 / 9.0)
    }

    /// Creates an AspectRatio with square 1:1 ratio.
    ///
    /// Common for profile pictures and icons.
    pub fn square() -> Self {
        Self::new(1.0)
    }

    /// Creates an AspectRatio with 4:3 classic TV ratio.
    pub fn classic_tv() -> Self {
        Self::new(4.0 / 3.0)
    }

    /// Creates an AspectRatio with 3:2 photo ratio.
    ///
    /// Common for DSLR cameras.
    pub fn photo() -> Self {
        Self::new(3.0 / 2.0)
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = AspectRatio::new(16.0 / 9.0);
    /// widget.set_child(Image::network(url));
    /// ```
    pub fn set_child(&mut self, child: impl IntoElement) {
        self.child = Child::new(child);
    }

    /// Validates AspectRatio configuration.
    ///
    /// Returns an error if aspect_ratio is not positive and finite.
    pub fn validate(&self) -> Result<(), String> {
        if self.aspect_ratio <= 0.0 {
            return Err(format!(
                "Invalid aspect_ratio: {}. Must be positive.",
                self.aspect_ratio
            ));
        }

        if !self.aspect_ratio.is_finite() {
            return Err(format!(
                "Invalid aspect_ratio: {}. Must be finite (not NaN or infinity).",
                self.aspect_ratio
            ));
        }

        Ok(())
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::square()
    }
}

// Implement View for AspectRatio - New architecture
impl StatelessView for AspectRatio {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderAspectRatio::new(self.aspect_ratio).child_opt(self.child)
    }
}

// bon Builder Extensions
use aspect_ratio_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> AspectRatioBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// AspectRatio::builder()
    ///     .aspect_ratio(16.0 / 9.0)
    ///     .child(video_player)
    ///     .build()
    /// ```
    pub fn child(self, child: impl IntoElement) -> AspectRatioBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

// Public build() wrapper
impl<S: State> AspectRatioBuilder<S> {
    /// Builds the AspectRatio widget.
    pub fn build(self) -> AspectRatio {
        self.build_internal()
    }
}

/// Macro for creating AspectRatio with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Simple square
/// aspect_ratio! {
///     aspect_ratio: 1.0,
/// }
///
/// // 16:9 widescreen
/// aspect_ratio! {
///     aspect_ratio: 16.0 / 9.0,
/// }
/// ```
#[macro_export]
macro_rules! aspect_ratio {
    () => {
        $crate::AspectRatio::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::AspectRatio {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

// AspectRatio now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aspect_ratio_new() {
        let widget = AspectRatio::new(16.0 / 9.0);
        assert!(widget.key.is_none());
        assert_eq!(widget.aspect_ratio, 16.0 / 9.0);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_aspect_ratio_widescreen() {
        let widget = AspectRatio::widescreen();
        assert_eq!(widget.aspect_ratio, 16.0 / 9.0);
    }

    #[test]
    fn test_aspect_ratio_square() {
        let widget = AspectRatio::square();
        assert_eq!(widget.aspect_ratio, 1.0);
    }

    #[test]
    fn test_aspect_ratio_classic_tv() {
        let widget = AspectRatio::classic_tv();
        assert_eq!(widget.aspect_ratio, 4.0 / 3.0);
    }

    #[test]
    fn test_aspect_ratio_photo() {
        let widget = AspectRatio::photo();
        assert_eq!(widget.aspect_ratio, 3.0 / 2.0);
    }

    #[test]
    fn test_aspect_ratio_default() {
        let widget = AspectRatio::default();
        assert_eq!(widget.aspect_ratio, 1.0);
    }

    #[test]
    fn test_aspect_ratio_builder() {
        let widget = AspectRatio::builder().aspect_ratio(2.0).build();
        assert_eq!(widget.aspect_ratio, 2.0);
    }

    #[test]
    fn test_aspect_ratio_struct_literal() {
        let widget = AspectRatio {
            aspect_ratio: 1.5,
            ..Default::default()
        };
        assert_eq!(widget.aspect_ratio, 1.5);
    }

    #[test]
    fn test_aspect_ratio_validate_ok() {
        let widget = AspectRatio::new(1.0);
        assert!(widget.validate().is_ok());

        let widget = AspectRatio::new(16.0 / 9.0);
        assert!(widget.validate().is_ok());

        let widget = AspectRatio::new(0.5);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_aspect_ratio_validate_zero() {
        let widget = AspectRatio {
            aspect_ratio: 0.0,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_aspect_ratio_validate_negative() {
        let widget = AspectRatio {
            aspect_ratio: -1.0,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_aspect_ratio_validate_nan() {
        let widget = AspectRatio {
            aspect_ratio: f32::NAN,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_aspect_ratio_validate_infinity() {
        let widget = AspectRatio {
            aspect_ratio: f32::INFINITY,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_aspect_ratio_macro_empty() {
        let widget = aspect_ratio!();
        assert_eq!(widget.aspect_ratio, 1.0);
    }

    #[test]
    fn test_aspect_ratio_macro_with_ratio() {
        let widget = aspect_ratio! {
            aspect_ratio: 16.0 / 9.0,
        };
        assert_eq!(widget.aspect_ratio, 16.0 / 9.0);
    }

    #[test]
    fn test_aspect_ratio_landscape() {
        let widget = AspectRatio::new(2.0);
        assert!(widget.aspect_ratio > 1.0); // landscape
    }

    #[test]
    fn test_aspect_ratio_portrait() {
        let widget = AspectRatio::new(0.5);
        assert!(widget.aspect_ratio < 1.0); // portrait
    }
}
