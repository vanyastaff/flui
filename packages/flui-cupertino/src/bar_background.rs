//! What the navigation and tab bars paint behind their content: the fill,
//! the hairline border and the backdrop blur a translucent fill enables.

use flui_sdk::painting::paint::ImageFilter;
use flui_sdk::painting::styling::{Border, BoxDecoration, Color};
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{BackdropFilter, DecoratedBox};

/// The Gaussian sigma, in logical pixels on each axis, of the blur behind a
/// translucent bar: `ImageFilter.blur(sigmaX: 10.0, sigmaY: 10.0)` in
/// `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:255`
/// and `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/bottom_tab_bar.dart:201`.
const BAR_BLUR_SIGMA: f64 = 10.0;

/// The border a bar paints: its default hairline, one the author set, or
/// none.
///
/// The default is a variant rather than a `Border` built at construction
/// because its width (one device pixel) and, for the tab bar, its colour
/// (light or dark) depend on the context the bar builds in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BarBorder {
    /// The bar's own hairline, built in `build` for the ambient context.
    Hairline,
    /// A border the author set.
    Custom(Border),
    /// The author removed the border.
    Removed,
}

impl BarBorder {
    /// The variant an author's `border(Option<Border>)` call selects.
    pub(crate) fn authored(border: Option<Border>) -> Self {
        border.map_or(Self::Removed, Self::Custom)
    }

    /// The border to paint: `hairline()` for the default, the custom border
    /// as set, or nothing.
    pub(crate) fn resolve(&self, hairline: impl FnOnce() -> Border) -> Option<Border> {
        match self {
            Self::Hairline => Some(hairline()),
            Self::Custom(border) => Some(*border),
            Self::Removed => None,
        }
    }
}

/// The width, in logical pixels, of one device pixel under `ctx`: `1.0`
/// divided by the ambient `MediaQuery`'s device pixel ratio, or `1.0`
/// without one.
///
/// Flutter spells a one-device-pixel side as width `0.0`, which its border
/// painter strokes as a hairline; `flui-painting` paints nothing for a side
/// of width `0.0`, so the bars state the width the hairline covers. A ratio
/// that is not finite and positive gives `1.0` rather than an infinite or
/// negative stroke.
pub(crate) fn hairline_width(ctx: &dyn BuildContext) -> f64 {
    let ratio = MediaQuery::device_pixel_ratio_of(ctx).unwrap_or(1.0);
    if ratio.is_finite() && ratio > 0.0 {
        1.0 / ratio
    } else {
        1.0
    }
}

/// `child` over a `background` fill and `border`, inside a backdrop blur that
/// is enabled while the fill is not fully opaque.
///
/// The filter stays mounted under an opaque fill, switched off, so a bar whose
/// background crosses full opacity keeps its subtree's shape and its slots
/// keep their elements and state; Flutter's navigation bar keeps a disabled
/// `BackdropFilter` for the same shape
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:252`).
/// Flutter wraps the filter in a `ClipRect` to keep the blur inside the bar;
/// FLUI's filter already covers only its child's laid-out box
/// (`crates/flui-widgets/src/paint/backdrop_filter.rs:19`), so no clip is
/// added.
pub(crate) fn bar_background(
    background: Color,
    border: Option<Border>,
    child: impl IntoView,
) -> impl IntoView {
    BackdropFilter::new(ImageFilter::blur(BAR_BLUR_SIGMA))
        .enabled(!background.is_opaque())
        .child(
            DecoratedBox::new(BoxDecoration::with_color(background).set_border(border))
                .child(child),
        )
}
