//! PositionedDirectional widget - RTL-aware positioning in Stack
//!
//! A widget that controls where a child of a Stack is positioned,
//! with support for text direction (LTR/RTL).
//!
//! Similar to Flutter's PositionedDirectional widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! PositionedDirectional::builder()
//!     .start(16.0)
//!     .top(24.0)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::element::Element;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_types::prelude::TextDirection;

/// A widget that controls where a child of a Stack is positioned,
/// using directional (start/end) instead of left/right coordinates.
///
/// **MUST be a descendant of a Stack widget.**
///
/// ## Directional Positioning
///
/// - **start**: Left edge in LTR, right edge in RTL
/// - **end**: Right edge in LTR, left edge in RTL
/// - **top**: Distance from top edge
/// - **bottom**: Distance from bottom edge
///
/// ## Layout Rules
///
/// 1. Must provide either `start` or `end`, not both
/// 2. Must provide either `width` or both `start` and `end`
/// 3. Must provide either `height` or both `top` and `bottom`
/// 4. Cannot provide `start`, `end`, and `width` simultaneously
/// 5. Cannot provide `top`, `bottom`, and `height` simultaneously
///
/// ## Examples
///
/// ```rust,ignore
/// // Position at start (left in LTR, right in RTL)
/// Stack::builder()
///     .children(vec![
///         Box::new(PositionedDirectional::builder()
///             .start(16.0)
///             .top(24.0)
///             .child(Text::new("Start"))
///             .build()
///         ),
///     ])
///     .build()
///
/// // Fill from start to end
/// PositionedDirectional::builder()
///     .start(0.0)
///     .end(0.0)
///     .top(0.0)
///     .bottom(0.0)
///     .child(Container::new())
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct PositionedDirectional {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Distance from the start edge (left in LTR, right in RTL).
    pub start: Option<f32>,

    /// Distance from the top edge.
    pub top: Option<f32>,

    /// Distance from the end edge (right in LTR, left in RTL).
    pub end: Option<f32>,

    /// Distance from the bottom edge.
    pub bottom: Option<f32>,

    /// Width of the positioned widget.
    ///
    /// Cannot be provided if both `start` and `end` are provided.
    pub width: Option<f32>,

    /// Height of the positioned widget.
    ///
    /// Cannot be provided if both `top` and `bottom` are provided.
    pub height: Option<f32>,

    /// Text direction to use for resolving start/end.
    ///
    /// If not provided, inherits from BuildContext.
    pub text_direction: Option<TextDirection>,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Element>,
}

impl std::fmt::Debug for PositionedDirectional {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PositionedDirectional")
            .field("key", &self.key)
            .field("start", &self.start)
            .field("top", &self.top)
            .field("end", &self.end)
            .field("bottom", &self.bottom)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("text_direction", &self.text_direction)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
    }
}

impl PositionedDirectional {
    /// Creates a new PositionedDirectional widget.
    pub fn new() -> Self {
        Self {
            key: None,
            start: None,
            top: None,
            end: None,
            bottom: None,
            width: None,
            height: None,
            text_direction: None,
            child: None,
        }
    }

    /// Creates a PositionedDirectional that fills the entire Stack.
    pub fn fill(child: impl View + 'static) -> Self {
        Self {
            key: None,
            start: Some(0.0),
            top: Some(0.0),
            end: Some(0.0),
            bottom: Some(0.0),
            width: None,
            height: None,
            text_direction: None,
            child: Some(child.into_element()),
        }
    }

    /// Creates a PositionedDirectional positioned from start.
    pub fn from_start(start: f32, top: f32, child: impl View + 'static) -> Self {
        Self {
            key: None,
            start: Some(start),
            top: Some(top),
            end: None,
            bottom: None,
            width: None,
            height: None,
            text_direction: None,
            child: Some(child.into_element()),
        }
    }

    /// Creates a PositionedDirectional positioned from end.
    pub fn from_end(end: f32, top: f32, child: impl View + 'static) -> Self {
        Self {
            key: None,
            start: None,
            top: Some(top),
            end: Some(end),
            bottom: None,
            width: None,
            height: None,
            text_direction: None,
            child: Some(child.into_element()),
        }
    }

    /// Converts into Positioned using the given text direction.
    ///
    /// This resolves start/end to left/right based on text direction.
    fn into_positioned(self, text_direction: TextDirection) -> crate::Positioned {
        let (left, right) = match text_direction {
            TextDirection::Ltr => (self.start, self.end),
            TextDirection::Rtl => (self.end, self.start),
        };

        crate::Positioned {
            key: self.key,
            left,
            top: self.top,
            right,
            bottom: self.bottom,
            width: self.width,
            height: self.height,
            child: self.child,
        }
    }

    /// Sets the child widget.
    #[deprecated(note = "Use builder pattern with .child() instead")]
    pub fn set_child(&mut self, child: Element) {
        self.child = Some(child);
    }
}

impl Default for PositionedDirectional {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use positioned_directional_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> PositionedDirectionalBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// PositionedDirectional::builder()
    ///     .start(16.0)
    ///     .top(24.0)
    ///     .child(Text::new("Content"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> PositionedDirectionalBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

// Public build() wrapper
impl<S: State> PositionedDirectionalBuilder<S> {
    /// Builds the PositionedDirectional.
    pub fn build(self) -> PositionedDirectional {
        self.build_internal()
    }
}

// Implement View trait
impl StatelessView for PositionedDirectional {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Get text direction from context or use default
        let text_direction = self.text_direction.unwrap_or(TextDirection::Ltr);

        // Convert to regular Positioned and build
        self.into_positioned(text_direction)
    }
}

/// Macro for creating PositionedDirectional with declarative syntax.
#[macro_export]
macro_rules! positioned_directional {
    () => {
        $crate::PositionedDirectional::new()
    };
    (fill: $child:expr) => {
        $crate::PositionedDirectional::fill($child)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positioned_directional_new() {
        let widget = PositionedDirectional::new();
        assert!(widget.key.is_none());
        assert!(widget.start.is_none());
        assert!(widget.top.is_none());
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_positioned_directional_fill() {
        let widget = PositionedDirectional::fill(crate::SizedBox::new());
        assert_eq!(widget.start, Some(0.0));
        assert_eq!(widget.top, Some(0.0));
        assert_eq!(widget.end, Some(0.0));
        assert_eq!(widget.bottom, Some(0.0));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_directional_from_start() {
        let widget = PositionedDirectional::from_start(16.0, 24.0, crate::SizedBox::new());
        assert_eq!(widget.start, Some(16.0));
        assert_eq!(widget.top, Some(24.0));
        assert!(widget.end.is_none());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_directional_from_end() {
        let widget = PositionedDirectional::from_end(16.0, 24.0, crate::SizedBox::new());
        assert_eq!(widget.end, Some(16.0));
        assert_eq!(widget.top, Some(24.0));
        assert!(widget.start.is_none());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_directional_into_positioned_ltr() {
        let widget = PositionedDirectional {
            key: None,
            start: Some(10.0),
            top: Some(20.0),
            end: Some(30.0),
            bottom: Some(40.0),
            width: None,
            height: None,
            text_direction: None,
            child: None,
        };

        let positioned = widget.into_positioned(TextDirection::Ltr);
        assert_eq!(positioned.left, Some(10.0));
        assert_eq!(positioned.top, Some(20.0));
        assert_eq!(positioned.right, Some(30.0));
        assert_eq!(positioned.bottom, Some(40.0));
    }

    #[test]
    fn test_positioned_directional_into_positioned_rtl() {
        let widget = PositionedDirectional {
            key: None,
            start: Some(10.0),
            top: Some(20.0),
            end: Some(30.0),
            bottom: Some(40.0),
            width: None,
            height: None,
            text_direction: None,
            child: None,
        };

        let positioned = widget.into_positioned(TextDirection::Rtl);
        // In RTL: start becomes right, end becomes left
        assert_eq!(positioned.left, Some(30.0));
        assert_eq!(positioned.top, Some(20.0));
        assert_eq!(positioned.right, Some(10.0));
        assert_eq!(positioned.bottom, Some(40.0));
    }

    #[test]
    fn test_positioned_directional_builder() {
        let widget = PositionedDirectional::builder().build();
        assert!(widget.start.is_none());
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_positioned_directional_builder_full() {
        let widget = PositionedDirectional::builder()
            .start(10.0)
            .top(20.0)
            .child(crate::SizedBox::new())
            .build();
        assert_eq!(widget.start, Some(10.0));
        assert_eq!(widget.top, Some(20.0));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_directional_macro() {
        let widget = positioned_directional!();
        assert!(widget.start.is_none());
    }

    #[test]
    fn test_positioned_directional_macro_fill() {
        let widget = positioned_directional!(fill: crate::SizedBox::new());
        assert_eq!(widget.start, Some(0.0));
        assert!(widget.child.is_some());
    }
}
