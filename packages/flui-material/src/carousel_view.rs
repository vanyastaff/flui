//! [`CarouselView`]: Material's carousel, a [`Carousel`] whose items are M3 cards.
//!
//! # M3 tokens
//!
//! | Token | Value |
//! |---|---|
//! | `backgroundColor` | `ColorScheme.surface` |
//! | `elevation` | `0.0` |
//! | `shape` | rounded rectangle, radius `28.0` (extra large) |
//! | `overlayColor` | `ColorScheme.onSurface` at the pressed, hovered and focused state-layer alphas |
//! | `padding` | `EdgeInsets::all(4.0)` |
//! | small item | 40–56 dp ([`CarouselLayout::default`]) |
//!
//! Each field falls back from the widget to [`ThemeData::carousel_view_theme`] to the token.

use std::fmt;
use std::rc::Rc;

use flui_sdk::geometry::{EdgeInsets, Radius};
use flui_sdk::painting::{BorderRadius, Clip, Color};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{EventCx, EventOutcome};
use flui_sdk::widgets::{
    AnimatedBuilder, Carousel, CarouselAutoplay, CarouselController, CarouselItemScope,
    CarouselLayout, KeylineSnap, Positioned, Semantics, SemanticsRole, Stack, WidgetState,
    WidgetStateProperty,
};

use crate::event_callback::{ValueCallback, value_callback};
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::ripple::StateLayerAlpha;
use crate::shape::MaterialShape;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// M3's extra-large corner, the cards' radius.
const DEFAULT_CORNER_RADIUS: f64 = 28.0;
/// The inset of a card inside its item's mask on every side.
const DEFAULT_PADDING: f64 = 4.0;

/// Builds item `index`'s content with its scope.
type ContentBuilder = Rc<dyn Fn(&dyn BuildContext, &CarouselItemScope, usize) -> BoxedView>;

/// Whether [`CarouselView::snap`] chose the snap, or the layout implies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapChoice {
    Implied,
    Set(Option<KeylineSnap>),
}

/// A Material carousel: a [`Carousel`] whose items are M3 cards.
///
/// A card covers the part of its item the carousel shows, inset by the padding and clipped
/// to the shape, so a shrinking item keeps its rounded corners on what is visible. Its content
/// stays laid out at the full item extent behind the card and does not reflow. With
/// [`on_tap`](Self::on_tap) each card is a button with a ripple, focus and activation; without
/// it a card is a plain surface.
///
/// The snap follows the layout unless [`snap`](Self::snap) sets it:
/// [`KeylineSnap::for_layout`], one item per swipe for every layout but
/// [`CarouselLayout::uncontained`], which scrolls freely.
///
/// ```rust,ignore
/// CarouselView::builder(photos.len(), move |_ctx, _scope, index| Image::new(photos[index].clone()))
///     .on_tap(move |cx, index| open.update(cx, |open| *open = Some(index)))
/// ```
#[derive(Clone, StatelessView)]
pub struct CarouselView {
    count: usize,
    builder: ContentBuilder,
    layout: CarouselLayout,
    snap: SnapChoice,
    controller: Option<CarouselController>,
    autoplay: Option<CarouselAutoplay>,
    semantic_label: Option<String>,
    on_tap: Option<ValueCallback<usize>>,
    background_color: Option<Color>,
    elevation: Option<f64>,
    shape: Option<MaterialShape>,
    overlay_color: Option<WidgetStateProperty<Option<Color>>>,
    padding: Option<EdgeInsets>,
}

impl fmt::Debug for CarouselView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselView")
            .field("count", &self.count)
            .field("layout", &self.layout)
            .field("snap", &self.snap)
            .field("autoplay", &self.autoplay)
            .field("on_tap", &self.on_tap.is_some())
            .finish_non_exhaustive()
    }
}

impl CarouselView {
    /// `count` items; `builder` builds an item's content, which the view puts on its card.
    pub fn builder<F, V>(count: usize, builder: F) -> Self
    where
        F: Fn(&dyn BuildContext, &CarouselItemScope, usize) -> V + 'static,
        V: IntoView,
    {
        Self {
            count,
            builder: Rc::new(move |ctx, scope, index| {
                BoxedView::new(builder(ctx, scope, index).into_view())
            }),
            layout: CarouselLayout::default(),
            snap: SnapChoice::Implied,
            controller: None,
            autoplay: None,
            semantic_label: None,
            on_tap: None,
            background_color: None,
            elevation: None,
            shape: None,
            overlay_color: None,
            padding: None,
        }
    }

    /// The keyline layout; [`CarouselLayout::default`], the M3 multi-browse layout, unless set.
    #[must_use]
    pub fn layout(mut self, layout: CarouselLayout) -> Self {
        self.layout = layout;
        self
    }

    /// How a release settles; `None` scrolls freely. Unset, it is
    /// [`KeylineSnap::for_layout`] of the layout.
    #[must_use]
    pub fn snap(mut self, snap: impl Into<Option<KeylineSnap>>) -> Self {
        self.snap = SnapChoice::Set(snap.into());
        self
    }

    /// The controller of the active item, its moves and its autoplay.
    #[must_use]
    pub fn controller(mut self, controller: CarouselController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Moves on by itself, with the pause control [`Carousel::autoplay`] draws.
    #[must_use]
    pub fn autoplay(mut self, autoplay: CarouselAutoplay) -> Self {
        self.autoplay = Some(autoplay);
        self
    }

    /// The label of the scroll view assistive technology announces.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Calls `callback` with the index of the card tapped or activated; with it every card is
    /// a button.
    #[must_use]
    pub fn on_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, usize) -> R + 'static,
        R: EventOutcome,
    {
        self.on_tap = Some(value_callback(callback));
        self
    }

    /// Overrides the cards' fill color.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the cards' elevation; it must not be negative, as [`Material::elevation`]
    /// requires.
    #[must_use]
    pub fn elevation(mut self, elevation: f64) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Overrides the cards' shape.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Overrides the cards' state layer.
    #[must_use]
    pub fn overlay_color(mut self, color: WidgetStateProperty<Option<Color>>) -> Self {
        self.overlay_color = Some(color);
        self
    }

    /// Overrides the inset of each card inside its item's mask.
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// The snap this view settles with.
    fn resolved_snap(&self) -> Option<KeylineSnap> {
        match self.snap {
            SnapChoice::Implied => KeylineSnap::for_layout(&self.layout),
            SnapChoice::Set(snap) => snap,
        }
    }
}

/// The cards' style after the widget → theme → token cascade.
#[derive(Clone)]
struct CardStyle {
    color: Color,
    elevation: f64,
    shape: MaterialShape,
    overlay: WidgetStateProperty<Option<Color>>,
    padding: EdgeInsets,
}

fn resolve_style(theme: &ThemeData, view: &CarouselView) -> CardStyle {
    let themed = theme.carousel_view_theme.as_ref();
    let on_surface = theme.color_scheme.on_surface;
    CardStyle {
        color: view
            .background_color
            .or_else(|| themed.and_then(|t| t.background_color))
            .unwrap_or(theme.color_scheme.surface),
        elevation: view
            .elevation
            .or_else(|| themed.and_then(|t| t.elevation))
            .unwrap_or(0.0),
        shape: view
            .shape
            .or_else(|| themed.and_then(|t| t.shape))
            .unwrap_or_else(|| {
                MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(
                    DEFAULT_CORNER_RADIUS,
                )))
            }),
        overlay: view
            .overlay_color
            .clone()
            .or_else(|| themed.and_then(|t| t.overlay_color.clone()))
            .unwrap_or_else(|| {
                WidgetStateProperty::resolve_with(move |states| {
                    let alpha = StateLayerAlpha::M3;
                    if states.contains_state(WidgetState::Pressed) {
                        Some(on_surface.with_opacity(alpha.pressed))
                    } else if states.contains_state(WidgetState::Hovered) {
                        Some(on_surface.with_opacity(alpha.hovered))
                    } else if states.contains_state(WidgetState::Focused) {
                        Some(on_surface.with_opacity(alpha.focused))
                    } else {
                        None
                    }
                })
            }),
        padding: view
            .padding
            .or_else(|| themed.and_then(|t| t.padding))
            .unwrap_or_else(|| EdgeInsets::all(DEFAULT_PADDING)),
    }
}

impl StatelessView for CarouselView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let style = resolve_style(&Theme::of(ctx), self);
        let builder = Rc::clone(&self.builder);
        let on_tap = self.on_tap.clone();
        let mut carousel = Carousel::builder(self.count, move |ctx, scope, index| {
            let content = builder(ctx, scope, index);
            CarouselCard {
                scope: scope.clone(),
                content,
                style: style.clone(),
                on_tap: on_tap.clone(),
            }
        })
        .layout(self.layout.clone())
        .item_spacing(0.0);
        if let Some(snap) = self.resolved_snap() {
            carousel = carousel.snap(snap);
        }
        if let Some(controller) = &self.controller {
            carousel = carousel.controller(controller.clone());
        }
        if let Some(autoplay) = self.autoplay {
            carousel = carousel.autoplay(autoplay);
        }
        if let Some(label) = &self.semantic_label {
            carousel = carousel.semantic_label(label.clone());
        }
        carousel
    }
}

/// One item's card, placed on the item's mask whenever the carousel scrolls.
#[derive(Clone, StatelessView)]
struct CarouselCard {
    scope: CarouselItemScope,
    content: BoxedView,
    style: CardStyle,
    on_tap: Option<ValueCallback<usize>>,
}

impl fmt::Debug for CarouselCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselCard")
            .field("index", &self.scope.index())
            .finish_non_exhaustive()
    }
}

impl StatelessView for CarouselCard {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let card = self.clone();
        AnimatedBuilder::new(self.scope.listenable(), move || card.placed())
    }
}

impl CarouselCard {
    /// The card on the item's current mask, the content behind it at the item's own place.
    fn placed(&self) -> Stack {
        let CardStyle {
            color,
            elevation,
            shape,
            overlay,
            padding,
        } = self.style.clone();
        // The mask is centred on the item, so it also gives the item's extent. Before its first
        // layout an item has no mask, and the card covers the whole item.
        let mask = self.scope.mask().map(|mask| (mask.left(), mask.width()));
        let start = mask.map_or(0.0, |(left, _)| left) + padding.left;
        let content = Positioned::new(self.content.clone())
            .left(-start)
            .top(0.0)
            .bottom(0.0);
        let content = match mask {
            Some((left, width)) => content.width(2.0 * left + width),
            None => content.right(-padding.right),
        };
        let surface = Stack::new(vec![content.boxed()]);
        let index = self.scope.index();
        let body: BoxedView = match &self.on_tap {
            Some(on_tap) => {
                let on_tap = Rc::clone(on_tap);
                InkWell::new(surface)
                    .shape(shape)
                    .overlay_color(overlay)
                    .on_tap(move |cx| on_tap(cx, index))
                    .boxed()
            }
            None => surface.boxed(),
        };
        let card = Material::new(color)
            .elevation(elevation)
            .shape(shape)
            .clip_behavior(Clip::AntiAlias)
            .child(body);
        // A tappable card is a button to assistive technology, as a Material button is.
        let card: BoxedView = if self.on_tap.is_some() {
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .child(card)
                .boxed()
        } else {
            card.boxed()
        };
        let card = Positioned::new(card)
            .left(start)
            .top(padding.top)
            .bottom(padding.bottom);
        let card = match mask {
            Some((_, width)) => card.width((width - padding.left - padding.right).max(0.0)),
            None => card.right(padding.right),
        };
        Stack::new(vec![card.boxed()])
    }
}
