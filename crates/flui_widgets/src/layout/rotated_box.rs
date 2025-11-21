//! RotatedBox widget - rotates child by quarter turns (90°, 180°, 270°)
//!
//! A widget that rotates its child by multiples of 90 degrees.
//! Similar to Flutter's RotatedBox widget.

use bon::Builder;
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::{IntoElement, View};

use flui_core::BuildContext;
use flui_rendering::RenderRotatedBox;
use flui_types::geometry::QuarterTurns;

// Re-export QuarterTurns for convenience
pub use flui_types::geometry::QuarterTurns as WidgetQuarterTurns;

/// A widget that rotates its child by a integral number of quarter turns.
///
/// RotatedBox rotates its child in 90-degree increments (quarter turns).
/// Unlike Transform which applies visual rotation only, RotatedBox properly
/// adjusts layout constraints, swapping width and height for 90° and 270° rotations.
///
/// ## Layout Behavior
///
/// - **0 turns (0°)**: No rotation, normal layout
/// - **1 turn (90°)**: Clockwise rotation, width ↔ height swapped
/// - **2 turns (180°)**: Upside down, normal dimensions
/// - **3 turns (270°)**: Counter-clockwise, width ↔ height swapped
///
/// The parent sees the rotated dimensions, so layout is affected.
///
/// ## QuarterTurns
///
/// ```rust,ignore
/// QuarterTurns::Zero   // 0°
/// QuarterTurns::One    // 90° clockwise
/// QuarterTurns::Two    // 180°
/// QuarterTurns::Three  // 270° clockwise (90° counter-clockwise)
/// ```
///
/// ## Common Use Cases
///
/// ### Rotate text label
/// ```rust,ignore
/// RotatedBox::builder()
///     .quarter_turns(QuarterTurns::One)
///     .child(Text::new("Vertical Label"))
///     .build()
/// ```
///
/// ### Flip widget upside down
/// ```rust,ignore
/// RotatedBox::rotate_180(widget)
/// ```
///
/// ### Landscape to portrait icon
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Icon::new("landscape"),
///         RotatedBox::rotate_90(Icon::new("landscape")),
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // 90° rotation
/// RotatedBox::rotate_90(Text::new("→ becomes ↓"))
///
/// // 180° rotation
/// RotatedBox::rotate_180(widget)
///
/// // Using builder
/// RotatedBox::builder()
///     .quarter_turns(QuarterTurns::Three)
///     .child(my_widget)
///     .build()
///
/// // From integer (modulo 4)
/// RotatedBox::new(QuarterTurns::from_int(5), widget)  // Same as One
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(QuarterTurns, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct RotatedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Number of quarter turns to rotate clockwise.
    /// Default: QuarterTurns::Zero (no rotation)
    #[builder(default = QuarterTurns::Zero)]
    pub quarter_turns: QuarterTurns,

    /// The child widget to rotate.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Element>,
}

impl std::fmt::Debug for RotatedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotatedBox")
            .field("key", &self.key)
            .field("quarter_turns", &self.quarter_turns)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
    }
}

impl RotatedBox {
    /// Creates a new RotatedBox with the given rotation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = RotatedBox::new(QuarterTurns::One, child);
    /// ```
    pub fn new(quarter_turns: QuarterTurns, child: impl View + 'static) -> Self {
        Self {
            key: None,
            quarter_turns,
            child: Some(child.into_element()),
        }
    }

    /// Creates a RotatedBox with 90° clockwise rotation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = RotatedBox::rotate_90(Text::new("Vertical"));
    /// ```
    pub fn rotate_90(child: impl View + 'static) -> Self {
        Self::new(QuarterTurns::One, child)
    }

    /// Creates a RotatedBox with 180° rotation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = RotatedBox::rotate_180(child);
    /// ```
    pub fn rotate_180(child: impl View + 'static) -> Self {
        Self::new(QuarterTurns::Two, child)
    }

    /// Creates a RotatedBox with 270° clockwise (90° counter-clockwise) rotation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = RotatedBox::rotate_270(child);
    /// ```
    pub fn rotate_270(child: impl View + 'static) -> Self {
        Self::new(QuarterTurns::Three, child)
    }

    /// Sets the child widget.
    #[deprecated(note = "Use builder pattern with .child() instead")]
    pub fn set_child(&mut self, child: Element) {
        self.child = Some(child);
    }
}

impl Default for RotatedBox {
    fn default() -> Self {
        Self {
            key: None,
            quarter_turns: QuarterTurns::Zero,
            child: None,
        }
    }
}

// bon Builder Extensions
use rotated_box_builder::{IsUnset, SetChild, State};

impl<S: State> RotatedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RotatedBox::builder()
    ///     .quarter_turns(QuarterTurns::One)
    ///     .child(Text::new("Rotated"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> RotatedBoxBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

impl<S: State> RotatedBoxBuilder<S> {
    /// Builds the RotatedBox.
    pub fn build(self) -> RotatedBox {
        self.build_internal()
    }
}

// Implement View trait - Simplified API
impl View for RotatedBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderRotatedBox::new(self.quarter_turns).child_opt(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotated_box_new() {
        let widget = RotatedBox::new(QuarterTurns::One, crate::SizedBox::new());
        assert_eq!(widget.quarter_turns, QuarterTurns::One);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_rotated_box_rotate_90() {
        let widget = RotatedBox::rotate_90(crate::SizedBox::new());
        assert_eq!(widget.quarter_turns, QuarterTurns::One);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_rotated_box_rotate_180() {
        let widget = RotatedBox::rotate_180(crate::SizedBox::new());
        assert_eq!(widget.quarter_turns, QuarterTurns::Two);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_rotated_box_rotate_270() {
        let widget = RotatedBox::rotate_270(crate::SizedBox::new());
        assert_eq!(widget.quarter_turns, QuarterTurns::Three);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_rotated_box_builder() {
        let widget = RotatedBox::builder()
            .quarter_turns(QuarterTurns::Two)
            .build();
        assert_eq!(widget.quarter_turns, QuarterTurns::Two);
    }

    #[test]
    fn test_rotated_box_default() {
        let widget = RotatedBox::default();
        assert_eq!(widget.quarter_turns, QuarterTurns::Zero);
        assert!(widget.child.is_none());
    }

    #[test]
    #[allow(deprecated)]
    fn test_rotated_box_set_child() {
        let mut widget = RotatedBox::default();
        assert!(widget.child.is_none());

        widget.set_child(crate::SizedBox::new().into_element());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_quarter_turns_from_int() {
        let widget = RotatedBox::new(QuarterTurns::from_int(5), crate::SizedBox::new());
        assert_eq!(widget.quarter_turns, QuarterTurns::One); // 5 % 4 = 1
    }
}
