//! [`IconThemeData`] and [`IconTheme`] — ambient defaults for [`Icon`](crate::Icon).

use std::fmt;

use flui_painting::styling::Color;
use flui_painting::typography::TextShadow;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

use super::SymbolEffect;

/// The size, color, opacity, font-variation axes, shadows, text-scaling
/// policy and symbol effect an [`Icon`](crate::Icon) resolves from its nearest [`IconTheme`]
/// ancestor.
///
/// Every field is optional: `None` means "not specified at this level, defer
/// to whatever is further up" (per-field inheritance).
/// [`IconTheme::of`] resolves the ambient theme down to
/// [`IconThemeData::fallback`] so callers always get a fully-populated value.
///
/// Built from [`default`](Self::default) with one method per field, so a new field never
/// breaks a caller:
///
/// ```rust
/// # use flui_widgets::IconThemeData;
/// let theme = IconThemeData::default().size(20.0).apply_text_scaling(true);
/// assert_eq!(theme.size, Some(20.0));
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct IconThemeData {
    /// Icon side length in logical pixels (icons are square: this sizes both
    /// width and height).
    pub size: Option<f64>,

    /// Icon color, before `opacity` is applied.
    pub color: Option<Color>,

    /// Opacity multiplier applied to `color` (and to a caller-supplied
    /// [`Icon::color`](crate::Icon::color) override).
    ///
    /// **Deferred:** [`Icon`](crate::Icon)'s `build` does not yet fold this
    /// into the resolved color; the field is carried for forward
    /// compatibility with that follow-up.
    pub opacity: Option<f64>,

    /// `FILL` font-variation axis (`0.0` unfilled .. `1.0` filled). No effect
    /// if the icon font doesn't expose the axis.
    pub fill: Option<f64>,

    /// `wght` font-variation axis (stroke weight). No effect if the icon
    /// font doesn't expose the axis.
    pub weight: Option<f64>,

    /// `GRAD` font-variation axis (granular stroke weight; can be negative).
    /// No effect if the icon font doesn't expose the axis.
    pub grade: Option<f64>,

    /// `opsz` font-variation axis (optical size). No effect if the icon font
    /// doesn't expose the axis.
    pub optical_size: Option<f64>,

    /// Shadows painted underneath the icon.
    pub shadows: Option<Vec<TextShadow>>,

    /// Whether to scale `size` by the ambient `MediaQuery` text-scale
    /// factor.
    ///
    /// Applies to the square's side and the glyph size together, including an
    /// explicit [`Icon::size`](crate::Icon::size) override. Defaults to false.
    pub apply_text_scaling: Option<bool>,

    /// The motion every [`Icon`](crate::Icon) and [`AnimatedIcon`](crate::AnimatedIcon)
    /// below plays when it has no [`SymbolEffect`] of its own. No fallback: without one
    /// icons are still.
    pub symbol_effect: Option<SymbolEffect>,
}

impl IconThemeData {
    /// Reasonable defaults for an unstyled subtree: 24px, opaque black,
    /// unfilled/regular-weight/neutral-grade glyphs at optical size 48, no
    /// shadows, no text-scaling.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            size: Some(24.0),
            color: Some(Color::BLACK),
            opacity: Some(1.0),
            fill: Some(0.0),
            weight: Some(400.0),
            grade: Some(0.0),
            optical_size: Some(48.0),
            shadows: None,
            apply_text_scaling: Some(false),
            symbol_effect: None,
        }
    }

    /// Sets the `size` field.
    #[must_use]
    pub fn size(mut self, size: f64) -> Self {
        self.size = Some(size);
        self
    }

    /// Sets the `color` field.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the `opacity` field.
    #[must_use]
    pub fn opacity(mut self, opacity: f64) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Sets the `fill` field.
    #[must_use]
    pub fn fill(mut self, fill: f64) -> Self {
        self.fill = Some(fill);
        self
    }

    /// Sets the `weight` field.
    #[must_use]
    pub fn weight(mut self, weight: f64) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets the `grade` field.
    #[must_use]
    pub fn grade(mut self, grade: f64) -> Self {
        self.grade = Some(grade);
        self
    }

    /// Sets the `optical_size` field.
    #[must_use]
    pub fn optical_size(mut self, optical_size: f64) -> Self {
        self.optical_size = Some(optical_size);
        self
    }

    /// Sets the `shadows` field.
    #[must_use]
    pub fn shadows(mut self, shadows: Vec<TextShadow>) -> Self {
        self.shadows = Some(shadows);
        self
    }

    /// Sets the `apply_text_scaling` field.
    #[must_use]
    pub fn apply_text_scaling(mut self, apply: bool) -> Self {
        self.apply_text_scaling = Some(apply);
        self
    }

    /// Sets the `symbol_effect` field.
    #[must_use]
    pub fn symbol_effect(mut self, effect: SymbolEffect) -> Self {
        self.symbol_effect = Some(effect);
        self
    }

    /// Fill every `None` field in `self` from `fallback`.
    #[must_use]
    fn resolved_against(&self, fallback: &Self) -> Self {
        Self {
            size: self.size.or(fallback.size),
            color: self.color.or(fallback.color),
            opacity: self.opacity.or(fallback.opacity),
            fill: self.fill.or(fallback.fill),
            weight: self.weight.or(fallback.weight),
            grade: self.grade.or(fallback.grade),
            optical_size: self.optical_size.or(fallback.optical_size),
            shadows: self.shadows.clone().or_else(|| fallback.shadows.clone()),
            apply_text_scaling: self.apply_text_scaling.or(fallback.apply_text_scaling),
            symbol_effect: self
                .symbol_effect
                .clone()
                .or_else(|| fallback.symbol_effect.clone()),
        }
    }
}

/// Provides [`IconThemeData`] to its subtree via FLUI's inherited-data
/// mechanism.
///
/// `IconTheme` does not rewrap itself across `Navigator` route boundaries:
/// the `InheritedView` mechanism has no route-boundary concept yet, so only
/// the plain ambient-lookup behavior ([`IconTheme::of`]) exists.
#[derive(Clone)]
pub struct IconTheme {
    /// The data this node provides to descendants.
    data: IconThemeData,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl IconTheme {
    /// Wrap `child` in an `IconTheme` that provides `data` to all
    /// descendants.
    #[must_use]
    pub fn new(data: IconThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            child: child.into_view().boxed(),
        }
    }

    /// Resolve the effective [`IconThemeData`] for `ctx`: the nearest
    /// ancestor [`IconTheme`]'s data with every unset field filled in from
    /// [`IconThemeData::fallback`], or `fallback()` outright when there is no
    /// ancestor. Registers a dependency so this element rebuilds when the
    /// ambient theme changes.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> IconThemeData {
        Self::maybe_of(ctx)
            .unwrap_or_default()
            .resolved_against(&IconThemeData::fallback())
    }

    /// Look up the nearest ancestor [`IconTheme`]'s raw data (not resolved
    /// against the fallback), registering a dependency. Returns `None` if
    /// there is no [`IconTheme`] ancestor.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<IconThemeData> {
        ctx.depend_on::<Self, _>(|theme| theme.data.clone())
    }
}

impl fmt::Debug for IconTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconTheme")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for IconTheme {
    type Data = IconThemeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.data != old.data
    }
}

impl_inherited_view!(IconTheme);
