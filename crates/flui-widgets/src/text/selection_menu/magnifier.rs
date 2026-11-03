//! The magnifier a touch drag of a selection handle shows over the line of
//! the dragged end: what it looks like ([`SelectionMagnifier`],
//! [`PlainMagnifier`]) and where FLUI puts it.

use std::rc::Rc;

use flui_foundation::geometry::{Offset, Size};
use flui_painting::styling::{BorderSide, BorderStyle, BoxShadow, Color};
use flui_painting::typography::TextDirection;
use flui_platform::TargetPlatform;
use flui_rendering::layer::LayerLink;
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};

#[cfg(doc)]
use crate::text::DefaultSelectionControls;
use crate::text::selection_area::HandleSide;
use crate::text::selection_area::handles::LinkFollower;
use crate::{
    ExcludeSemantics, IgnorePointer, MagnifierDecoration, MagnifierScale, MagnifierShape,
    Positioned, RawMagnifier,
};

/// How the magnifier of a selection handle drag looks.
///
/// While a finger drags a handle of a selection area or an editable text,
/// FLUI shows the magnifier this builds above the line of the dragged end:
/// its bottom centre [`MagnifierInfo::gap`] above the top of the line, at
/// the end. It moves with the end, takes no touch, and shows nothing to
/// assistive technology, which hears the end move already. It goes when
/// the drag ends or the handles hide.
///
/// A look usually builds a [`RawMagnifier`] focused on the end with
/// [`MagnifierInfo::focal_offset`].
pub trait SelectionMagnifier: 'static {
    /// The magnifier over `info`'s end.
    fn build(&self, info: &MagnifierInfo, ctx: &dyn BuildContext) -> BoxedView;
}

/// What a [`SelectionMagnifier`] builds a magnifier for.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MagnifierInfo {
    /// The end the dragged handle moves.
    pub side: HandleSide,
    /// The direction of the text the end is in.
    pub text_direction: TextDirection,
    /// The height of the line the end sits on, in logical pixels.
    pub line_height: f64,
    /// How far above the top of that line the magnifier's bottom sits.
    pub gap: f64,
}

impl MagnifierInfo {
    /// The space FLUI leaves between the line and the magnifier above it.
    pub const GAP: f64 = 8.0;

    pub(crate) fn new(side: HandleSide, text_direction: TextDirection, line_height: f64) -> Self {
        Self {
            side,
            text_direction,
            line_height,
            gap: Self::GAP,
        }
    }

    /// Where the middle of the end's line is from the centre of a magnifier
    /// of `size` placed over it: what [`RawMagnifier::focal_offset`] takes
    /// to show the end at the magnifier's centre.
    #[must_use]
    pub fn focal_offset(&self, size: Size<f64>) -> Offset<f64> {
        Offset::new(0.0, size.height / 2.0 + self.gap + self.line_height / 2.0)
    }
}

/// A plain magnifier: a [`RawMagnifier`] with a thin border and a soft
/// shadow, focused on the dragged end.
///
/// [`for_platform`](Self::for_platform) gives each platform's own: a
/// rounded rectangle on Android and Fuchsia, a capsule on iOS, none on the
/// desktops, whose text shows no magnifier.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::PlainMagnifier;
/// # use flui_platform::TargetPlatform;
/// assert!(PlainMagnifier::for_platform(TargetPlatform::Android).is_some());
/// assert!(PlainMagnifier::for_platform(TargetPlatform::Windows).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlainMagnifier {
    size: Size<f64>,
    scale: MagnifierScale,
    shape: MagnifierShape,
}

impl PlainMagnifier {
    /// A rounded rectangle 78 × 38, enlarging 1.25 times, as Android's.
    #[must_use]
    pub fn rounded() -> Self {
        Self {
            size: Size::new(78.0, 38.0),
            scale: MagnifierScale::try_new(1.25).expect("BUG: 1.25 is a valid scale"),
            shape: MagnifierShape::RoundedRect { radius: 10.0 },
        }
    }

    /// A capsule 80 × 48, enlarging 1.25 times, as iOS's.
    #[must_use]
    pub fn capsule() -> Self {
        Self {
            size: Size::new(80.0, 48.0),
            scale: MagnifierScale::try_new(1.25).expect("BUG: 1.25 is a valid scale"),
            shape: MagnifierShape::Capsule,
        }
    }

    /// The magnifier `platform`'s own text shows, if any.
    #[must_use]
    pub fn for_platform(platform: TargetPlatform) -> Option<Self> {
        match platform {
            TargetPlatform::Android | TargetPlatform::Fuchsia => Some(Self::rounded()),
            TargetPlatform::iOS => Some(Self::capsule()),
            _ => None,
        }
    }

    /// The same magnifier at `size`.
    #[must_use]
    pub fn size(mut self, size: Size<f64>) -> Self {
        self.size = size;
        self
    }

    /// The same magnifier enlarging `scale` times.
    #[must_use]
    pub fn scale(mut self, scale: MagnifierScale) -> Self {
        self.scale = scale;
        self
    }
}

impl SelectionMagnifier for PlainMagnifier {
    fn build(&self, info: &MagnifierInfo, _ctx: &dyn BuildContext) -> BoxedView {
        RawMagnifier::new(self.size, self.scale)
            .focal_offset(info.focal_offset(self.size))
            .shape(self.shape)
            .decoration(
                MagnifierDecoration::new()
                    .border(BorderSide::new(
                        Color::rgba(0, 0, 0, 40),
                        1.0,
                        BorderStyle::Solid,
                    ))
                    .shadow(BoxShadow::new(
                        Color::rgba(0, 0, 0, 60),
                        Offset::new(0.0, 2.0),
                        6.0,
                        0.0,
                    )),
            )
            .boxed()
    }
}

/// Whether a widget or scope set a magnifier or turned it off.
#[derive(Clone)]
pub(crate) enum MagnifierChoice {
    Show(Rc<dyn SelectionMagnifier>),
    Hide,
}

/// The magnifier `look` builds for `info`, placed above the end published
/// under `link`, taking no touch and hidden from assistive technology.
pub(crate) fn magnifier(
    look: &dyn SelectionMagnifier,
    info: &MagnifierInfo,
    link: LayerLink,
    ctx: &dyn BuildContext,
) -> BoxedView {
    let shown = ExcludeSemantics::new().child(IgnorePointer::new().child(look.build(info, ctx)));
    Positioned::new(LinkFollower::above(link, info.gap, shown))
        .left(0.0)
        .top(0.0)
        .boxed()
}
