//! Clipping types for painting.

use crate::geometry::{px, Offset, Pixels, Rect, Size};

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipOp {
    #[default]
    Intersect,
    /// The new region is subtracted from the current clip.
    /// Pixels inside the new region become invisible (creates a "hole").
    Difference,
}

#[derive(Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Clip {
    /// No clipping whatsoever.
    ///
    /// This is the most efficient option. If you know that your content
    /// will not exceed the bounds of the box, use this.
    None,

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
    #[must_use]
    pub const fn is_anti_aliased(&self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    #[must_use]
    pub const fn saves_layer(&self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }

    #[must_use]
    pub const fn clips(&self) -> bool {
        !matches!(self, Clip::None)
    }

    #[must_use]
    pub const fn is_efficient(&self) -> bool {
        matches!(self, Clip::None | Clip::HardEdge)
    }
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipBehavior {
    /// No clipping.
    None,

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
    #[must_use]
    pub const fn to_clip(self) -> Clip {
        match self {
            ClipBehavior::None => Clip::None,
            ClipBehavior::HardEdge => Clip::HardEdge,
            ClipBehavior::AntiAlias => Clip::AntiAlias,
            ClipBehavior::AntiAliasWithSaveLayer => Clip::AntiAliasWithSaveLayer,
        }
    }

    #[must_use]
    pub const fn clips(self) -> bool {
        !matches!(self, ClipBehavior::None)
    }

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
    fn get_outer_path(
        &self,
        host: Rect<Pixels>,
        guest: Option<Rect<Pixels>>,
    ) -> Vec<Offset<Pixels>>;
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CircularNotchedRectangle {
    /// The margin around the guest rectangle.
    pub margin: f32,
}

impl CircularNotchedRectangle {
    #[must_use]
    pub const fn new() -> Self {
        Self { margin: 4.0 }
    }

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
    fn get_outer_path(
        &self,
        host: Rect<Pixels>,
        guest: Option<Rect<Pixels>>,
    ) -> Vec<Offset<Pixels>> {
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
        if (guest_center_y - host.top()).abs() < guest.height() / 2.0 + px(self.margin) {
            let notch_radius = guest.width() / 2.0 + px(self.margin);

            // Left part of top edge (before notch)
            path.push(Offset::new(guest_center_x - notch_radius, host.top()));

            // Create circular notch (simplified - in reality would use bezier curves)
            let steps = 16;
            for i in 0..=steps {
                let angle = std::f32::consts::PI + (i as f32 / steps as f32) * std::f32::consts::PI;
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

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutomaticNotchedShape<T: NotchedShape> {
    /// The inner notched shape.
    pub inner: T,

    /// Scale factor for the notch size.
    pub scale: f32,
}

impl<T: NotchedShape> AutomaticNotchedShape<T> {
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self { inner, scale: 1.0 }
    }

    #[must_use]
    pub const fn with_scale(inner: T, scale: f32) -> Self {
        Self { inner, scale }
    }
}

impl<T: NotchedShape> NotchedShape for AutomaticNotchedShape<T> {
    fn get_outer_path(
        &self,
        host: Rect<Pixels>,
        guest: Option<Rect<Pixels>>,
    ) -> Vec<Offset<Pixels>> {
        let scaled_guest = guest.map(|g| {
            let center = g.center();
            let scaled_size = Size::new(g.width() * self.scale, g.height() * self.scale);
            Rect::from_center_size(center, scaled_size)
        });

        self.inner.get_outer_path(host, scaled_guest)
    }
}
