//! ClipRect widget - clips child to a rectangle
//!
//! A widget that clips its child using a rectangle.
//! Similar to Flutter's ClipRect widget.
//!
//! # Usage Patterns
//!
//! ## Builder Pattern
//! ```rust,ignore
//! ClipRect::builder()
//!     .clip_behavior(Clip::AntiAlias)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::BuildContext;

use flui_core::view::{IntoElement, View};
use flui_rendering::{RectShape, RenderClipRect};
use flui_types::painting::Clip;

/// A widget that clips its child using a rectangle.
///
/// By default, ClipRect prevents its child from painting outside its bounds,
/// but the size and location of the clip rect match those of the child,
/// so it doesn't affect layout.
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - No effect on layout, only affects painting
///
/// ## Clipping Behavior
///
/// - **Clip::None**: No clipping (child paints normally)
/// - **Clip::HardEdge**: Fast clipping without anti-aliasing
/// - **Clip::AntiAlias**: Smooth clipping with anti-aliasing (slower)
/// - **Clip::AntiAliasWithSaveLayer**: Highest quality but slowest
///
/// ## Examples
///
/// ```rust,ignore
/// // Clip with anti-aliasing
/// ClipRect::builder()
///     .clip_behavior(Clip::AntiAlias)
///     .child(overflowing_content)
///     .build()
///
/// // Fast clipping without anti-aliasing
/// ClipRect::builder()
///     .clip_behavior(Clip::HardEdge)
///     .child(content)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct ClipRect {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to clip the child
    #[builder(default = Clip::AntiAlias)]
    pub clip_behavior: Clip,

    /// The child widget to clip
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn >>,
}

impl std::fmt::Debug for ClipRect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipRect")
            .field("key", &self.key)
            .field("clip_behavior", &self.clip_behavior)
            .field(
                "child",
                &if self.child.is_some() {
                    "<>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for ClipRect {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            clip_behavior: self.clip_behavior,
            child: self.child.clone(),
        }
    }
}

impl ClipRect {
    /// Creates a new ClipRect widget.
    ///
    /// # Parameters
    ///
    /// - `clip_behavior`: How to perform clipping (default: AntiAlias)
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            key: None,
            clip_behavior,
            child: None,
        }
    }

    /// Creates a ClipRect widget with a child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ClipRect::with_child(Clip::HardEdge, OverflowingWidget::new())
    /// ```
    pub fn with_child(clip_behavior: Clip, child: impl View + 'static) -> Self {
        Self::builder()
            .clip_behavior(clip_behavior)
            .child(child)
            .build()
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }
}

impl Default for ClipRect {
    fn default() -> Self {
        Self::new(Clip::AntiAlias)
    }
}

// Implement View for ClipRect - New architecture
impl View for ClipRect {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        (
            RenderClipRect::new(RectShape, self.clip_behavior),
            self.child,
        )
    }
}

// bon Builder Extensions
use clip_rect_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> ClipRectBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> ClipRectBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> ClipRectBuilder<S> {
    /// Builds the ClipRect widget.
    pub fn build(self) -> ClipRect {
        self.build_internal()
    }
}

// ClipRect now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_rect_new() {
        let widget = ClipRect::new(Clip::HardEdge);
        assert!(widget.key.is_none());
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_clip_rect_default() {
        let widget = ClipRect::default();
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_builder() {
        let widget = ClipRect::builder().build();
        assert_eq!(widget.clip_behavior, Clip::AntiAlias); // Default
    }

    #[test]
    fn test_clip_rect_builder_with_clip_behavior() {
        let widget = ClipRect::builder().clip_behavior(Clip::None).build();
        assert_eq!(widget.clip_behavior, Clip::None);
    }

    #[test]
    fn test_clip_rect_all_clip_behaviors() {
        // Test all clip behavior variants
        let widget_none = ClipRect::new(Clip::None);
        assert_eq!(widget_none.clip_behavior, Clip::None);

        let widget_hard = ClipRect::new(Clip::HardEdge);
        assert_eq!(widget_hard.clip_behavior, Clip::HardEdge);

        let widget_aa = ClipRect::new(Clip::AntiAlias);
        assert_eq!(widget_aa.clip_behavior, Clip::AntiAlias);

        let widget_save = ClipRect::new(Clip::AntiAliasWithSaveLayer);
        assert_eq!(widget_save.clip_behavior, Clip::AntiAliasWithSaveLayer);
    }
}
