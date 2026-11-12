//! [`RawMagnifier`]: a loupe that shows what is painted behind it, enlarged.

use flui_foundation::geometry::{Offset, Point, Radius, Rect, Size};
use flui_objects::{Oval, StackFit};
use flui_painting::BoxShape;
use flui_painting::paint::{ImageFilter, ImageSampling, ImageTransform};
use flui_painting::styling::{Border, BorderRadius, BorderSide, BoxDecoration, BoxShadow};
use flui_view::prelude::{BuildContext, StatelessView};
use flui_view::{BoxedView, Child, IntoView, ViewExt};

use crate::{BackdropFilter, ClipOval, ClipRRect, DecoratedBox, SizedBox, Stack};

/// How many times a [`RawMagnifier`] enlarges: finite and above zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagnifierScale(f64);

/// A [`MagnifierScale`] that is not finite or not above zero.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("a magnifier's scale must be finite and above zero, not {0}")]
pub struct InvalidMagnifierScale(pub f64);

impl MagnifierScale {
    /// `scale` times.
    ///
    /// # Errors
    ///
    /// [`InvalidMagnifierScale`] for a `scale` that is NaN, infinite, zero
    /// or negative.
    pub fn try_new(scale: f64) -> Result<Self, InvalidMagnifierScale> {
        if scale.is_finite() && scale > 0.0 {
            Ok(Self(scale))
        } else {
            Err(InvalidMagnifierScale(scale))
        }
    }

    /// The factor.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

/// The outline of a [`RawMagnifier`]: what it clips the enlarged backdrop
/// to, and what its [`MagnifierDecoration`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum MagnifierShape {
    /// The circle centred in the box, as wide as its shorter side.
    #[default]
    Circle,
    /// The box with its corners rounded by `radius`.
    RoundedRect {
        /// The corner radius.
        radius: f64,
    },
    /// The box with fully rounded ends.
    Capsule,
}

/// What a [`RawMagnifier`] draws over its edge, in its shape: a border and
/// shadows.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct MagnifierDecoration {
    /// The line along the outline.
    pub border: Option<BorderSide>,
    /// The shadows the magnifier casts.
    pub shadows: Vec<BoxShadow>,
}

impl MagnifierDecoration {
    /// No border and no shadow.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The line along the outline.
    #[must_use]
    pub fn border(mut self, side: BorderSide) -> Self {
        self.border = Some(side);
        self
    }

    /// Adds a shadow.
    #[must_use]
    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }
}

/// A loupe of a fixed size that shows the painting behind it enlarged.
///
/// The point [`focal_offset`](Self::focal_offset) away from its centre is
/// shown at its centre, [`MagnifierScale`] times larger, read with linear
/// sampling. The enlarged backdrop is clipped to its [`MagnifierShape`], and
/// its child, if any, paints over it. It takes hits and shows to assistive
/// technology as its child does: a selection's loupe wraps it in
/// [`IgnorePointer`](crate::IgnorePointer) and
/// [`ExcludeSemantics`](crate::ExcludeSemantics).
///
/// A size or an offset that makes the enlargement non-finite draws the
/// shape without the backdrop, and logs an error.
#[derive(Clone, Debug, StatelessView)]
pub struct RawMagnifier {
    size: Size<f64>,
    scale: MagnifierScale,
    focal_offset: Offset<f64>,
    shape: MagnifierShape,
    decoration: MagnifierDecoration,
    child: Child,
}

impl RawMagnifier {
    /// A circular loupe of `size`, enlarging `scale` times what is under
    /// its centre.
    #[must_use]
    pub fn new(size: Size<f64>, scale: MagnifierScale) -> Self {
        Self {
            size,
            scale,
            focal_offset: Offset::ZERO,
            shape: MagnifierShape::Circle,
            decoration: MagnifierDecoration::new(),
            child: Child::empty(),
        }
    }

    /// Shows the point `offset` away from the centre, in place of the one
    /// under it: a loupe above a finger shows what the finger covers.
    #[must_use]
    pub fn focal_offset(mut self, offset: Offset<f64>) -> Self {
        self.focal_offset = offset;
        self
    }

    /// The outline. Defaults to [`MagnifierShape::Circle`].
    #[must_use]
    pub fn shape(mut self, shape: MagnifierShape) -> Self {
        self.shape = shape;
        self
    }

    /// The border and shadows over the edge.
    #[must_use]
    pub fn decoration(mut self, decoration: MagnifierDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Painted over the enlarged backdrop, inside the outline.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// The transform that shows the point `focal_offset` from the centre at
    /// the centre, `scale` times larger: `p ↦ c + s·(p − (c + f))`.
    fn filter(&self) -> Option<ImageFilter> {
        let s = self.scale.get();
        let centre = Point::new(self.size.width / 2.0, self.size.height / 2.0);
        let focal = Point::new(
            centre.x + self.focal_offset.dx,
            centre.y + self.focal_offset.dy,
        );
        match ImageTransform::try_new(
            s,
            0.0,
            0.0,
            s,
            centre.x - s * focal.x,
            centre.y - s * focal.y,
        ) {
            Ok(transform) => Some(ImageFilter::Transform {
                transform,
                sampling: ImageSampling::Linear,
            }),
            Err(error) => {
                tracing::error!(
                    size = ?self.size,
                    focal_offset = ?self.focal_offset,
                    %error,
                    "a RawMagnifier cannot enlarge"
                );
                None
            }
        }
    }

    fn clip(&self, content: BoxedView) -> BoxedView {
        let Size { width, height } = self.size;
        match self.shape {
            MagnifierShape::Circle => {
                let side = width.min(height);
                ClipOval::new()
                    .clipper(Oval::from_rect(Rect::from_xywh(
                        (width - side) / 2.0,
                        (height - side) / 2.0,
                        side,
                        side,
                    )))
                    .child(content)
                    .boxed()
            }
            MagnifierShape::RoundedRect { radius } => {
                ClipRRect::circular(radius).child(content).boxed()
            }
            MagnifierShape::Capsule => ClipRRect::circular(width.min(height) / 2.0)
                .child(content)
                .boxed(),
        }
    }

    fn outline(&self) -> Option<BoxedView> {
        let MagnifierDecoration { border, shadows } = &self.decoration;
        if border.is_none() && shadows.is_empty() {
            return None;
        }
        let mut decoration = BoxDecoration::new();
        decoration.border = border.map(Border::all);
        decoration.box_shadow = (!shadows.is_empty()).then(|| shadows.clone());
        match self.shape {
            MagnifierShape::Circle => decoration.shape = BoxShape::Circle,
            MagnifierShape::RoundedRect { radius } => {
                decoration.border_radius = Some(BorderRadius::all(Radius::circular(radius)));
            }
            MagnifierShape::Capsule => {
                let radius = self.size.width.min(self.size.height) / 2.0;
                decoration.border_radius = Some(BorderRadius::all(Radius::circular(radius)));
            }
        }
        Some(
            DecoratedBox::new(decoration)
                .child(SizedBox::expand())
                .boxed(),
        )
    }
}

impl StatelessView for RawMagnifier {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let content = match self.child.clone().into_inner() {
            Some(child) => child,
            None => SizedBox::expand().boxed(),
        };
        let content = match self.filter() {
            Some(filter) => BackdropFilter::new(filter).child(content).boxed(),
            None => content,
        };
        let mut layers = vec![self.clip(content)];
        layers.extend(self.outline());
        SizedBox::new(self.size.width, self.size.height)
            .child(Stack::new(layers).fit(StackFit::Expand))
    }
}
