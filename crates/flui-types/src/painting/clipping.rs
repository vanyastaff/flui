//! Clipping types for painting.

use crate::geometry::{Offset, Pixels, Rect, Size, px};

/// How a new clip region combines with the current clip.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipOp {
    /// The new region is intersected with the current clip.
    /// Only pixels inside both regions remain visible.
    #[default]
    Intersect,
    /// The new region is subtracted from the current clip.
    /// Pixels inside the new region become invisible (creates a "hole").
    Difference,
}

/// The quality (and cost) with which content is clipped.
///
/// Ordered from cheapest to most expensive; mirrors Flutter's `Clip` enum.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Clip {
    /// No clipping whatsoever.
    ///
    /// This is the most efficient option. If you know that your content
    /// will not exceed the bounds of the box, use this.
    None,

    /// Clip, but do not apply anti-aliasing.
    ///
    /// Faster than `AntiAlias`, but jagged on non-axis-aligned edges. This is
    /// the default and is appropriate for rectangular clips.
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
    /// content that needs to be clipped with anti-aliasing AND has
    /// transparency. In most cases, `AntiAlias` is sufficient.
    AntiAliasWithSaveLayer,
}

impl Clip {
    /// Returns `true` if this mode applies anti-aliasing to clip edges.
    #[must_use]
    #[inline]
    pub const fn is_anti_aliased(&self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    /// Returns `true` if this mode saves a layer in addition to clipping.
    #[must_use]
    #[inline]
    pub const fn saves_layer(&self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }

    /// Returns `true` if this mode performs any clipping at all.
    #[must_use]
    #[inline]
    pub const fn clips(&self) -> bool {
        !matches!(self, Clip::None)
    }

    /// Returns `true` for the cheap modes (`None` and `HardEdge`) that need
    /// neither anti-aliasing nor a saved layer.
    #[must_use]
    #[inline]
    pub const fn is_efficient(&self) -> bool {
        matches!(self, Clip::None | Clip::HardEdge)
    }
}

/// Widget-level clipping behavior, convertible to a low-level [`Clip`] mode.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipBehavior {
    /// No clipping.
    None,

    /// Clip without anti-aliasing; the default.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing applied.
    ///
    /// This is appropriate for shapes with smooth curves or diagonal edges.
    AntiAlias,

    /// Clip with anti-aliasing and save a layer immediately following the clip.
    ///
    /// This is rarely needed, but can be used when a clip is applied to a
    /// widget with transparent children.
    AntiAliasWithSaveLayer,
}

impl ClipBehavior {
    /// Converts this behavior to the equivalent low-level [`Clip`] mode.
    #[must_use]
    #[inline]
    pub const fn to_clip(self) -> Clip {
        match self {
            ClipBehavior::None => Clip::None,
            ClipBehavior::HardEdge => Clip::HardEdge,
            ClipBehavior::AntiAlias => Clip::AntiAlias,
            ClipBehavior::AntiAliasWithSaveLayer => Clip::AntiAliasWithSaveLayer,
        }
    }

    /// Returns `true` if this behavior performs any clipping at all.
    #[must_use]
    #[inline]
    pub const fn clips(self) -> bool {
        !matches!(self, ClipBehavior::None)
    }

    /// Returns `true` if this behavior applies anti-aliasing to clip edges.
    #[must_use]
    #[inline]
    pub const fn is_anti_aliased(self) -> bool {
        matches!(
            self,
            ClipBehavior::AntiAlias | ClipBehavior::AntiAliasWithSaveLayer
        )
    }
}

impl From<ClipBehavior> for Clip {
    #[inline]
    fn from(behavior: ClipBehavior) -> Self {
        behavior.to_clip()
    }
}

/// A shape with a notch in its outline.
///
/// Similar to Flutter's `NotchedShape`.
///
/// Typically used with `BottomAppBar` to create a notch for a
/// `FloatingActionButton`.
pub trait NotchedShape: std::fmt::Debug {
    /// Creates a path for the outer edge of the shape.
    ///
    /// The `host` is the bounding rectangle of the shape.
    /// The `guest` is the bounding rectangle of the notch.
    ///
    /// Returns a path that describes the outer edge of the shape with the
    /// notch.
    fn get_outer_path(
        &self,
        host: Rect<Pixels>,
        guest: Option<Rect<Pixels>>,
    ) -> Vec<Offset<Pixels>>;
}

/// A rectangle with a semi-circular notch cut out of its top edge.
///
/// Similar to Flutter's `CircularNotchedRectangle`; used by a bottom app bar
/// to make room for a circular floating action button.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CircularNotchedRectangle {
    /// The margin around the guest rectangle.
    pub margin: f32,
}

impl CircularNotchedRectangle {
    /// Creates a circular notched rectangle with the default margin (4.0).
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self { margin: 4.0 }
    }

    /// Creates a circular notched rectangle with the given margin around the
    /// guest.
    #[must_use]
    #[inline]
    pub const fn with_margin(margin: f32) -> Self {
        Self { margin }
    }
}

impl Default for CircularNotchedRectangle {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NotchedShape for CircularNotchedRectangle {
    #[inline]
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

/// A [`NotchedShape`] wrapper that scales the guest rectangle around its
/// center before delegating to the inner shape.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AutomaticNotchedShape<T: NotchedShape> {
    /// The inner notched shape.
    pub inner: T,

    /// Scale factor for the notch size.
    pub scale: f32,
}

impl<T: NotchedShape> AutomaticNotchedShape<T> {
    /// Wraps `inner` without scaling the guest (scale factor 1.0).
    #[must_use]
    #[inline]
    pub const fn new(inner: T) -> Self {
        Self { inner, scale: 1.0 }
    }

    /// Wraps `inner`, scaling the guest rectangle by `scale` around its
    /// center before computing the notch.
    #[must_use]
    #[inline]
    pub const fn with_scale(inner: T, scale: f32) -> Self {
        Self { inner, scale }
    }
}

impl<T: NotchedShape> NotchedShape for AutomaticNotchedShape<T> {
    #[inline]
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
