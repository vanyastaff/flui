//! [`FlexibleSpaceBar`] — the expanding background-and-title treatment a
//! collapsing [`SliverAppBar`](crate::SliverAppBar) paints across its box.
//!
//! # How it learns the collapse state
//!
//! The bar itself is extent-blind: [`FlexibleSpaceBarSettings`] — an
//! inherited scope the `SliverAppBar` delegate wraps around its child on
//! every seam rebuild — carries `(min_extent, max_extent, current_extent,
//! toolbar_opacity)`, and this widget derives everything from those four
//! numbers. Flutter's shape exactly (`FlexibleSpaceBar.createSettings`);
//! there, as here, it is what lets the same widget serve any
//! persistent-header delegate, not only the app bar's.
//!
//! # The oracle's formulas, ported verbatim
//!
//! With `t = clamp(1 − (current − min) / (max − min), 0, 1)` (0 = expanded,
//! 1 = collapsed) — `material/flexible_space_bar.dart:237-241`:
//!
//! - **background opacity** = `1 − Interval(fade_start, 1).transform(t)`
//!   where `fade_start = max(0, 1 − kToolbarHeight / delta)`; equal extents
//!   cannot collapse, so opacity stays 1 (`:244-252`).
//! - **parallax offset** = `−lerp(0, delta / 4, t)` applied as the
//!   background's `top` (`:220`, `CollapseMode.parallax`, the default).
//! - **title scale** = `lerp(expanded_title_scale, 1, t)` about the title's
//!   own corner (`:330-335`).
//! - **toolbar opacity** fades the title (`:316-321`). Named divergence:
//!   Flutter rewrites the title style's COLOR alpha; FLUI wraps the title
//!   layer in an `Opacity`, which is visually equivalent for the title
//!   layer and directly assertable in tests.
//!
//! # Deferred, deliberately
//!
//! `CollapseMode::pin`/`none`, `StretchMode`s (need the stretched box),
//! `titlePadding` overrides, and the M3 `isScrolledUnder` tint. Each is an
//! additive knob on this same skeleton.

use flui_types::{Alignment, EdgeInsets};
use flui_view::BuildContextExt as _;
use flui_view::prelude::StatelessView;
use flui_view::{
    BoxedView, BuildContext, InheritedView, IntoView, View, ViewExt, impl_inherited_view,
};
use flui_widgets::{
    Align, DefaultTextStyle, Directionality, Opacity, Padding, Positioned, SizedBox, Stack,
    Transform,
};

/// The collapse state a [`FlexibleSpaceBar`] interpolates over — provided by
/// the enclosing `SliverAppBar` delegate on every build-during-layout
/// rebuild.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexibleSpaceBarData {
    /// The extent the bar collapses to.
    pub min_extent: f32,
    /// The extent the bar expands to.
    pub max_extent: f32,
    /// The bar's extent right now, `min_extent..=max_extent`.
    pub current_extent: f32,
    /// The toolbar content's opacity, for delegates that fade it late.
    pub toolbar_opacity: f32,
}

/// Inherited scope carrying [`FlexibleSpaceBarData`] — Flutter's
/// `FlexibleSpaceBarSettings`.
#[derive(Clone)]
pub struct FlexibleSpaceBarSettings {
    data: FlexibleSpaceBarData,
    child: BoxedView,
}

impl FlexibleSpaceBarSettings {
    /// Wrap `child` in a scope carrying `data`.
    #[must_use]
    pub fn new(data: FlexibleSpaceBarData, child: impl IntoView) -> Self {
        Self {
            data,
            child: BoxedView(Box::new(child.into_view())),
        }
    }
}

impl std::fmt::Debug for FlexibleSpaceBarSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexibleSpaceBarSettings")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for FlexibleSpaceBarSettings {
    type Data = FlexibleSpaceBarData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // The whole point: every extent change re-interpolates the bar.
        self.data != old.data
    }
}

impl_inherited_view!(FlexibleSpaceBarSettings);

/// The expanding treatment for a collapsing app bar: a `background` that
/// fades and parallaxes away as the bar collapses, and a `title` that grows
/// from toolbar size to [`expanded_title_scale`](Self::expanded_title_scale)
/// as it expands.
///
/// Hand it to [`SliverAppBar::flexible_space`](crate::SliverAppBar::flexible_space);
/// the delegate provides the [`FlexibleSpaceBarSettings`] it reads. Without
/// a settings ancestor the bar renders fully expanded (`t = 0`) — a
/// stand-alone preview rather than a panic.
///
/// Flutter parity: `material/flexible_space_bar.dart` `FlexibleSpaceBar`
/// (formulas cited on the module).
#[derive(Clone, StatelessView)]
pub struct FlexibleSpaceBar {
    title: Option<BoxedView>,
    background: Option<BoxedView>,
    center_title: bool,
    expanded_title_scale: f32,
}

impl FlexibleSpaceBar {
    /// An empty flexible space: no title, no background.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: None,
            background: None,
            center_title: false,
            expanded_title_scale: 1.5,
        }
    }

    /// The title shown at the bar's bottom edge, scaled up while expanded.
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.title = Some(title.into_view().boxed());
        self
    }

    /// The content painted behind the title, filling the expanded box and
    /// fading out as the bar collapses.
    #[must_use]
    pub fn background(mut self, background: impl IntoView) -> Self {
        self.background = Some(background.into_view().boxed());
        self
    }

    /// Center the title horizontally instead of the leading-aligned default.
    #[must_use]
    pub fn center_title(mut self, center_title: bool) -> Self {
        self.center_title = center_title;
        self
    }

    /// How much larger the title renders fully expanded (Flutter default
    /// 1.5).
    #[must_use]
    pub fn expanded_title_scale(mut self, scale: f32) -> Self {
        self.expanded_title_scale = scale;
        self
    }
}

impl Default for FlexibleSpaceBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FlexibleSpaceBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexibleSpaceBar")
            .field("center_title", &self.center_title)
            .field("expanded_title_scale", &self.expanded_title_scale)
            .finish_non_exhaustive()
    }
}

/// `Interval(fade_start, 1).transform(t)` for the background fade — the
/// only piece of Flutter's `Interval` curve this widget needs.
fn interval_transform(fade_start: f32, t: f32) -> f32 {
    if fade_start >= 1.0 {
        return if t >= 1.0 { 1.0 } else { 0.0 };
    }
    ((t - fade_start) / (1.0 - fade_start)).clamp(0.0, 1.0)
}

impl StatelessView for FlexibleSpaceBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let data = ctx
            .depend_on::<FlexibleSpaceBarSettings, _>(|scope| *InheritedView::data(scope))
            .unwrap_or(FlexibleSpaceBarData {
                min_extent: 0.0,
                max_extent: 1.0,
                current_extent: 1.0,
                toolbar_opacity: 1.0,
            });

        let delta_extent = data.max_extent - data.min_extent;
        // 0.0 -> expanded, 1.0 -> collapsed to the toolbar.
        let t = if delta_extent > 0.0 {
            (1.0 - (data.current_extent - data.min_extent) / delta_extent).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let mut layers: Vec<BoxedView> = Vec::new();

        if let Some(background) = &self.background {
            // Equal extents cannot collapse; the content stays visible.
            let opacity = if delta_extent <= 0.0 {
                1.0
            } else {
                let fade_start =
                    (1.0 - crate::app_bar::DEFAULT_TOOLBAR_HEIGHT / delta_extent).max(0.0);
                1.0 - interval_transform(fade_start, t)
            };
            // CollapseMode::parallax (the default): the background drifts
            // up at a quarter of the collapse distance.
            let parallax = -(delta_extent / 4.0) * t;
            layers.push(
                Positioned::new(
                    Opacity::new(opacity)
                        .child(SizedBox::height(data.max_extent).child(background.clone())),
                )
                .top(parallax)
                .left(0.0)
                .right(0.0)
                .height(data.max_extent)
                .boxed(),
            );
        }

        if let Some(title) = &self.title {
            let scale = self.expanded_title_scale + (1.0 - self.expanded_title_scale) * t;
            // Start/end resolve through the ambient Directionality — a
            // leading-aligned title sits at the RIGHT edge under RTL, and
            // its 72px leading inset moves with it.
            let rtl = Directionality::maybe_of(ctx)
                .is_some_and(|direction| direction == flui_types::typography::TextDirection::Rtl);
            let alignment = if self.center_title {
                Alignment::BOTTOM_CENTER
            } else if rtl {
                Alignment::BOTTOM_RIGHT
            } else {
                Alignment::BOTTOM_LEFT
            };
            // Flutter's default title padding: bottom 16, and a 72px START
            // inset when leading-aligned (past the leading slot).
            let start_inset = if self.center_title { 0.0 } else { 72.0 };
            let padding = EdgeInsets {
                top: px_f(0.0),
                right: px_f(if rtl { start_inset } else { 0.0 }),
                bottom: px_f(16.0),
                left: px_f(if rtl { 0.0 } else { start_inset }),
            };
            // The Material title style, faded by the delegate's toolbar
            // opacity (see the module doc's named divergence).
            let styled_title: BoxedView = match crate::Theme::maybe_of(ctx) {
                Some(theme) => {
                    let style = theme
                        .text_theme
                        .title_large
                        .clone()
                        .unwrap_or_default()
                        .with_color(theme.color_scheme.on_surface);
                    DefaultTextStyle::new(style, title.clone()).boxed()
                }
                None => title.clone(),
            };
            layers.push(
                Opacity::new(data.toolbar_opacity)
                    .child(
                        Padding::new(padding).child(
                            Transform::scale(scale, scale)
                                .alignment(alignment)
                                .child(Align::new(alignment).child(styled_title)),
                        ),
                    )
                    .boxed(),
            );
        }

        Stack::new(layers)
    }
}

/// Local shorthand: `EdgeInsets` is pixel-typed.
fn px_f(value: f32) -> flui_types::geometry::Pixels {
    flui_types::geometry::px(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oracle's fade window: opacity 1 until `t` enters
    /// `(fade_start, 1]`, then linear to 0 — with `fade_start =
    /// max(0, 1 − kToolbarHeight/delta)`.
    #[test]
    fn interval_transform_matches_the_fade_window() {
        // fade_start 0.5: below it → 0; midpoint of the window → 0.5; end → 1.
        assert_eq!(interval_transform(0.5, 0.25), 0.0);
        assert_eq!(interval_transform(0.5, 0.75), 0.5);
        assert_eq!(interval_transform(0.5, 1.0), 1.0);
        // Degenerate window (fade_start == 1): a step at t == 1.
        assert_eq!(interval_transform(1.0, 0.99), 0.0);
        assert_eq!(interval_transform(1.0, 1.0), 1.0);
    }
}
