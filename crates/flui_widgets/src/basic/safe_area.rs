//! SafeArea widget - insets child to avoid system UI
//!
//! A widget that insets its child by sufficient padding to avoid system UI.
//! Similar to Flutter's SafeArea widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! SafeArea::builder()
//!     .top(true)
//!     .bottom(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderPadding;
use flui_types::EdgeInsets;

/// A widget that insets its child by sufficient padding to avoid system UI intrusions.
///
/// SafeArea ensures that its child is not obscured by system UI elements like:
/// - Status bars
/// - Navigation bars
/// - Notches and camera cutouts
/// - Home indicators
///
/// ## Use Cases
///
/// - **Mobile Apps**: Avoid status bars and navigation bars
/// - **Full-Screen Content**: Keep content within safe bounds
/// - **Custom Layouts**: Respect system UI without manual padding
///
/// ## Customization
///
/// You can control which edges are inset:
/// - `top`: Avoid status bar and notches
/// - `bottom`: Avoid navigation bar and home indicator
/// - `left`: Avoid curved screen edges
/// - `right`: Avoid curved screen edges
///
/// ## Examples
///
/// ```rust,ignore
/// // Inset from all edges (default)
/// SafeArea::builder()
///     .child(MyContent::new())
///     .build()
///
/// // Inset only from top and bottom
/// SafeArea::builder()
///     .top(true)
///     .bottom(true)
///     .left(false)
///     .right(false)
///     .child(MyContent::new())
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct SafeArea {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether to avoid system intrusions at the top (status bar, notch).
    #[builder(default = true)]
    pub top: bool,

    /// Whether to avoid system intrusions at the bottom (navigation bar, home indicator).
    #[builder(default = true)]
    pub bottom: bool,

    /// Whether to avoid system intrusions on the left (curved edges).
    #[builder(default = true)]
    pub left: bool,

    /// Whether to avoid system intrusions on the right (curved edges).
    #[builder(default = true)]
    pub right: bool,

    /// Minimum padding to apply regardless of system UI.
    #[builder(default = EdgeInsets::ZERO)]
    pub minimum: EdgeInsets,

    /// The child widget to inset
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for SafeArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeArea")
            .field("key", &self.key)
            .field("top", &self.top)
            .field("bottom", &self.bottom)
            .field("left", &self.left)
            .field("right", &self.right)
            .field("minimum", &self.minimum)
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

impl Clone for SafeArea {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            top: self.top,
            bottom: self.bottom,
            left: self.left,
            right: self.right,
            minimum: self.minimum,
            child: self.child.clone(),
        }
    }
}

impl SafeArea {
    /// Creates a new SafeArea widget that avoids all system UI.
    pub fn new() -> Self {
        Self {
            key: None,
            top: true,
            bottom: true,
            left: true,
            right: true,
            minimum: EdgeInsets::ZERO,
            child: None,
        }
    }

    /// Creates a SafeArea that only avoids vertical system UI (top and bottom).
    pub fn vertical(child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            top: true,
            bottom: true,
            left: false,
            right: false,
            minimum: EdgeInsets::ZERO,
            child: Some(child),
        }
    }

    /// Creates a SafeArea that only avoids horizontal system UI (left and right).
    pub fn horizontal(child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            top: false,
            bottom: false,
            left: true,
            right: true,
            minimum: EdgeInsets::ZERO,
            child: Some(child),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Box<dyn AnyView>) {
        self.child = Some(child);
    }

    /// Calculate the safe area insets.
    ///
    /// In a real implementation, this would query the platform for actual safe area insets.
    /// For now, we use placeholder values.
    fn calculate_insets(&self, _ctx: &BuildContext) -> EdgeInsets {
        // TODO: Query actual safe area insets from platform
        // For now, use common default values for mobile devices

        let top_inset: f32 = if self.top { 44.0 } else { 0.0 }; // Status bar height
        let bottom_inset: f32 = if self.bottom { 34.0 } else { 0.0 }; // Home indicator
        // Usually 0 unless curved display
        let left_inset: f32 = 0.0;
        let right_inset: f32 = 0.0;

        EdgeInsets {
            left: left_inset.max(self.minimum.left),
            top: top_inset.max(self.minimum.top),
            right: right_inset.max(self.minimum.right),
            bottom: bottom_inset.max(self.minimum.bottom),
        }
    }
}

impl Default for SafeArea {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use safe_area_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> SafeAreaBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> SafeAreaBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Implement View trait
impl View for SafeArea {
    fn build(&self, ctx: &BuildContext) -> impl IntoElement {
        let insets = self.calculate_insets(ctx);

        (RenderPadding::new(insets), self.child)
    }
}

/// Macro for creating SafeArea with declarative syntax.
#[macro_export]
macro_rules! safe_area {
    () => {
        $crate::SafeArea::new()
    };
    (vertical: $child:expr) => {
        $crate::SafeArea::vertical(Box::new($child))
    };
    (horizontal: $child:expr) => {
        $crate::SafeArea::horizontal(Box::new($child))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_area_new() {
        let widget = SafeArea::new();
        assert!(widget.key.is_none());
        assert!(widget.top);
        assert!(widget.bottom);
        assert!(widget.left);
        assert!(widget.right);
        assert_eq!(widget.minimum, EdgeInsets::ZERO);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_safe_area_vertical() {
        let child = Box::new(crate::SizedBox::new());
        let widget = SafeArea::vertical(child);
        assert!(widget.top);
        assert!(widget.bottom);
        assert!(!widget.left);
        assert!(!widget.right);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_safe_area_horizontal() {
        let child = Box::new(crate::SizedBox::new());
        let widget = SafeArea::horizontal(child);
        assert!(!widget.top);
        assert!(!widget.bottom);
        assert!(widget.left);
        assert!(widget.right);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_safe_area_default() {
        let widget = SafeArea::default();
        assert!(widget.top);
        assert!(widget.bottom);
    }

    #[test]
    fn test_safe_area_builder() {
        let widget = SafeArea::builder().build_safe_area();
        assert!(widget.top); // Default is true
    }

    #[test]
    fn test_safe_area_builder_custom() {
        let widget = SafeArea::builder()
            .top(false)
            .bottom(true)
            .child(crate::SizedBox::new())
            .build_safe_area();
        assert!(!widget.top);
        assert!(widget.bottom);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_safe_area_set_child() {
        let mut widget = SafeArea::new();
        widget.set_child(Box::new(crate::SizedBox::new()));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_safe_area_macro_default() {
        let widget = safe_area!();
        assert!(widget.top);
    }

    #[test]
    fn test_safe_area_macro_vertical() {
        let widget = safe_area!(vertical: crate::SizedBox::new());
        assert!(widget.top);
        assert!(!widget.left);
    }

    #[test]
    fn test_safe_area_macro_horizontal() {
        let widget = safe_area!(horizontal: crate::SizedBox::new());
        assert!(!widget.top);
        assert!(widget.left);
    }
}
