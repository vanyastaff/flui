//! Clipping types for painting.

use crate::geometry::{Offset, Rect, Size};
use std::f32::consts::PI;

/// Different ways to clip a widget's content.
///
/// Similar to Flutter's `Clip`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::Clip;
///
/// let clip = Clip::AntiAlias;
/// assert_eq!(clip, Clip::AntiAlias);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Clip {
    /// No clipping whatsoever.
    ///
    /// This is the most efficient option. If you know that your content
    /// will not exceed the bounds of the box, use this.
    None,

    /// Clip to the bounding box, but without anti-aliasing.
    ///
    /// This mode is faster than anti-aliased clipping, but may produce
    /// aliasing artifacts. This is a good default for rectangular clips.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing.
    ///
    /// This mode is more expensive than `HardEdge` but produces smoother
    /// edges. Use this for non-rectangular clips or when you need smooth edges.
    AntiAlias,

    /// Clip with anti-aliasing and save a layer.
    ///
    /// This is the most expensive option and is only necessary when you have
    /// content that needs to be clipped with anti-aliasing AND has transparency.
    /// In most cases, `AntiAlias` is sufficient.
    AntiAliasWithSaveLayer,
}

impl Clip {
    /// Returns true if this clip mode requires anti-aliasing.
    #[inline]
    #[must_use]
    pub const fn is_anti_aliased(&self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    /// Returns true if this clip mode saves a layer.
    #[inline]
    #[must_use]
    pub const fn saves_layer(&self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }

    /// Returns true if this clip mode performs clipping.
    #[inline]
    #[must_use]
    pub const fn clips(&self) -> bool {
        !matches!(self, Clip::None)
    }

    /// Returns true if this is the most efficient clip mode.
    #[inline]
    #[must_use]
    pub const fn is_efficient(&self) -> bool {
        matches!(self, Clip::None | Clip::HardEdge)
    }
}

/// How to clip content.
///
/// Similar to Flutter's `ClipBehavior`. This is a more semantic version of `Clip`
/// that is used in higher-level widgets.
///
/// # Examples
///
/// ```
/// use flui_types::painting::ClipBehavior;
///
/// let behavior = ClipBehavior::AntiAlias;
/// assert_eq!(behavior.to_clip(), flui_types::painting::Clip::AntiAlias);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipBehavior {
    /// No clipping.
    None,

    /// Clip, but without applying anti-aliasing.
    ///
    /// This is the default and is appropriate for rectangles and
    /// other shapes that do not have diagonal edges.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing applied.
    ///
    /// This is appropriate for shapes with smooth curves or diagonal edges.
    AntiAlias,

    /// Clip with anti-aliasing and save a layer immediately following the clip.
    ///
    /// This is rarely needed, but can be used when a clip is applied to a widget
    /// with transparent children.
    AntiAliasWithSaveLayer,
}

impl ClipBehavior {
    /// Converts this clip behavior to a `Clip` mode.
    #[inline]
    #[must_use]
    pub const fn to_clip(self) -> Clip {
        match self {
            ClipBehavior::None => Clip::None,
            ClipBehavior::HardEdge => Clip::HardEdge,
            ClipBehavior::AntiAlias => Clip::AntiAlias,
            ClipBehavior::AntiAliasWithSaveLayer => Clip::AntiAliasWithSaveLayer,
        }
    }

    /// Returns true if this behavior performs clipping.
    #[inline]
    #[must_use]
    pub const fn clips(self) -> bool {
        !matches!(self, ClipBehavior::None)
    }

    /// Returns true if this behavior uses anti-aliasing.
    #[inline]
    #[must_use]
    pub const fn is_anti_aliased(self) -> bool {
        matches!(
            self,
            ClipBehavior::AntiAlias | ClipBehavior::AntiAliasWithSaveLayer
        )
    }
}

impl From<ClipBehavior> for Clip {
    fn from(behavior: ClipBehavior) -> Self {
        behavior.to_clip()
    }
}

/// A shape with a notch in its outline.
///
/// Similar to Flutter's `NotchedShape`.
///
/// Typically used with `BottomAppBar` to create a notch for a `FloatingActionButton`.
pub trait NotchedShape: std::fmt::Debug {
    /// Creates a path for the outer edge of the shape.
    ///
    /// The `host` is the bounding rectangle of the shape.
    /// The `guest` is the bounding rectangle of the notch.
    ///
    /// Returns a path that describes the outer edge of the shape with the notch.
    fn get_outer_path(&self, host: Rect, guest: Option<Rect>) -> Vec<Offset>;
}

/// A rectangle with smooth circular notches.
///
/// Similar to Flutter's `CircularNotchedRectangle`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{CircularNotchedRectangle, NotchedShape};
/// use flui_types::geometry::Rect;
///
/// let shape = CircularNotchedRectangle::new();
/// let host = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
/// let guest = Rect::from_xywh(80.0, -20.0, 40.0, 40.0);
///
/// let path = shape.get_outer_path(host, Some(guest));
/// assert!(!path.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CircularNotchedRectangle {
    /// The margin around the guest rectangle.
    pub margin: f32,
}

impl CircularNotchedRectangle {
    /// Creates a new circular notched rectangle.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self { margin: 4.0 }
    }

    /// Creates a circular notched rectangle with the given margin.
    #[inline]
    #[must_use]
    pub const fn with_margin(margin: f32) -> Self {
        Self { margin }
    }
}

impl Default for CircularNotchedRectangle {
    fn default() -> Self {
        Self::new()
    }
}

impl NotchedShape for CircularNotchedRectangle {
    fn get_outer_path(&self, host: Rect, guest: Option<Rect>) -> Vec<Offset> {
        let Some(guest) = guest else {
            // No notch, just return the rectangle corners
            return vec![
                Offset::new(host.left(), host.top()),
                Offset::new(host.right(), host.top()),
                Offset::new(host.right(), host.bottom()),
                Offset::new(host.left(), host.bottom()),
            ];
        };

        let mut path = Vec::new();

        // Start from top-left
        path.push(Offset::new(host.left(), host.top()));

        // Check if the guest intersects with the top edge
        let guest_center_x = guest.left() + guest.width() / 2.0;
        let guest_center_y = guest.top() + guest.height() / 2.0;

        // Only create notch if guest is near the top edge
        if (guest_center_y - host.top()).abs() < guest.height() / 2.0 + self.margin {
            let notch_radius = guest.width() / 2.0 + self.margin;

            // Left part of top edge (before notch)
            path.push(Offset::new(guest_center_x - notch_radius, host.top()));

            // Create circular notch (simplified - in reality would use bezier curves)
            let steps = 16;
            for i in 0..=steps {
                let angle = PI + (i as f32 / steps as f32) * PI;
                let x = guest_center_x + notch_radius * angle.cos();
                let y = host.top() + notch_radius * (1.0 + angle.sin());
                path.push(Offset::new(x, y));
            }

            // Right part of top edge (after notch)
            path.push(Offset::new(guest_center_x + notch_radius, host.top()));
        }

        // Top-right corner
        path.push(Offset::new(host.right(), host.top()));

        // Right edge
        path.push(Offset::new(host.right(), host.bottom()));

        // Bottom edge
        path.push(Offset::new(host.left(), host.bottom()));

        path
    }
}

/// A `NotchedShape` that automatically scales the notch based on the guest size.
///
/// Similar to Flutter's `AutomaticNotchedShape`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{AutomaticNotchedShape, CircularNotchedRectangle};
///
/// let inner_shape = CircularNotchedRectangle::new();
/// let shape = AutomaticNotchedShape::new(inner_shape);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutomaticNotchedShape<T: NotchedShape> {
    /// The inner notched shape.
    pub inner: T,

    /// Scale factor for the notch size.
    pub scale: f32,
}

impl<T: NotchedShape> AutomaticNotchedShape<T> {
    /// Creates a new automatic notched shape.
    #[inline]
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self { inner, scale: 1.0 }
    }

    /// Creates a new automatic notched shape with a custom scale.
    #[inline]
    #[must_use]
    pub const fn with_scale(inner: T, scale: f32) -> Self {
        Self { inner, scale }
    }
}

impl<T: NotchedShape> NotchedShape for AutomaticNotchedShape<T> {
    fn get_outer_path(&self, host: Rect, guest: Option<Rect>) -> Vec<Offset> {
        let scaled_guest = guest.map(|g| {
            let center = g.center();
            let scaled_size = Size::new(g.width() * self.scale, g.height() * self.scale);
            Rect::from_center_size(center, scaled_size)
        });

        self.inner.get_outer_path(host, scaled_guest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_default() {
        assert_eq!(Clip::default(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_is_anti_aliased() {
        assert!(!Clip::None.is_anti_aliased());
        assert!(!Clip::HardEdge.is_anti_aliased());
        assert!(Clip::AntiAlias.is_anti_aliased());
        assert!(Clip::AntiAliasWithSaveLayer.is_anti_aliased());
    }

    #[test]
    fn test_clip_saves_layer() {
        assert!(!Clip::None.saves_layer());
        assert!(!Clip::HardEdge.saves_layer());
        assert!(!Clip::AntiAlias.saves_layer());
        assert!(Clip::AntiAliasWithSaveLayer.saves_layer());
    }

    #[test]
    fn test_clip_behavior_default() {
        assert_eq!(ClipBehavior::default(), ClipBehavior::HardEdge);
    }

    #[test]
    fn test_clip_behavior_to_clip() {
        assert_eq!(ClipBehavior::None.to_clip(), Clip::None);
        assert_eq!(ClipBehavior::HardEdge.to_clip(), Clip::HardEdge);
        assert_eq!(ClipBehavior::AntiAlias.to_clip(), Clip::AntiAlias);
        assert_eq!(
            ClipBehavior::AntiAliasWithSaveLayer.to_clip(),
            Clip::AntiAliasWithSaveLayer
        );
    }

    #[test]
    fn test_clip_behavior_from() {
        assert_eq!(Clip::from(ClipBehavior::None), Clip::None);
        assert_eq!(Clip::from(ClipBehavior::AntiAlias), Clip::AntiAlias);
    }

    #[test]
    fn test_circular_notched_rectangle_new() {
        let shape = CircularNotchedRectangle::new();
        assert_eq!(shape.margin, 4.0);
    }

    #[test]
    fn test_circular_notched_rectangle_with_margin() {
        let shape = CircularNotchedRectangle::with_margin(8.0);
        assert_eq!(shape.margin, 8.0);
    }

    #[test]
    fn test_circular_notched_rectangle_no_guest() {
        let shape = CircularNotchedRectangle::new();
        let host = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);

        let path = shape.get_outer_path(host, None);

        // Should return 4 corners
        assert_eq!(path.len(), 4);
        assert_eq!(path[0], Offset::new(0.0, 0.0));
        assert_eq!(path[1], Offset::new(200.0, 0.0));
    }

    #[test]
    fn test_circular_notched_rectangle_with_guest() {
        let shape = CircularNotchedRectangle::new();
        let host = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
        let guest = Rect::from_xywh(80.0, -20.0, 40.0, 40.0);

        let path = shape.get_outer_path(host, Some(guest));

        // Should have more points due to the notch
        assert!(path.len() > 4);
    }

    #[test]
    fn test_automatic_notched_shape_new() {
        let inner = CircularNotchedRectangle::new();
        let shape = AutomaticNotchedShape::new(inner);

        assert_eq!(shape.scale, 1.0);
        assert_eq!(shape.inner.margin, 4.0);
    }

    #[test]
    fn test_automatic_notched_shape_with_scale() {
        let inner = CircularNotchedRectangle::new();
        let shape = AutomaticNotchedShape::with_scale(inner, 1.5);

        assert_eq!(shape.scale, 1.5);
    }

    #[test]
    fn test_automatic_notched_shape_path() {
        let inner = CircularNotchedRectangle::new();
        let shape = AutomaticNotchedShape::new(inner);
        let host = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);

        let path = shape.get_outer_path(host, None);
        assert!(!path.is_empty());
    }
}
