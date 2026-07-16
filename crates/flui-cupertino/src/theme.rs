//! [`CupertinoTheme`] — publishes [`CupertinoThemeData`] to a subtree via
//! FLUI's inherited-data mechanism.
//!
//! Flutter parity: `cupertino/theme.dart` `CupertinoTheme` /
//! `InheritedCupertinoTheme` / `CupertinoThemeData` (oracle tag `3.44.0`).
//! Like `flui-material`'s `Theme`, this collapses the oracle's
//! `CupertinoTheme` (a `StatelessWidget`) + `InheritedCupertinoTheme` (the
//! actual `InheritedTheme`) pair into one `InheritedView` type — the split
//! exists in Dart only so `CupertinoTheme.build` can also imply an
//! `IconTheme`, which this crate's V1 doesn't yet wire (no icon-family
//! component consumes it).
//!
//! ## Material-interop seam (nothing owed here)
//!
//! At tag `3.44.0`, `CupertinoTheme.of` never reads Material — it is
//! Material's `ThemeData.cupertinoOverrideTheme` /
//! `MaterialBasedCupertinoThemeData` that inject an `InheritedCupertinoTheme`
//! from *its* side when a Material `Theme` wants to also drive Cupertino
//! widgets underneath it. Per ADR-0028, that injection seam belongs to a
//! future `flui-material` increment, not this crate — `flui-cupertino` has
//! no dependency on `flui-material` and nothing here needs to change to
//! support it later.

use flui_types::Color;
use flui_types::platform::Brightness;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};
use flui_widgets::{InheritedTheme, MediaQuery};

use crate::colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::text_theme::CupertinoTextThemeData;

/// The oracle's `_kDefaultTheme.barBackgroundColor` — `0xF0F9F9F9` /
/// `0xF01D1D1D` (navigation-bar translucent background; toolbar/tabbar use a
/// darker `0xF0161616` the oracle notes but does not use here — see
/// `theme.dart`'s comment on `_kDefaultTheme`).
fn default_bar_background_color() -> CupertinoDynamicColor {
    CupertinoDynamicColor::with_brightness(
        Color::rgba(0xF9, 0xF9, 0xF9, 0xF0),
        Color::rgba(0x1D, 0x1D, 0x1D, 0xF0),
    )
}

/// Wraps `color` as a [`CupertinoDynamicColor`] — a caller-supplied
/// [`CupertinoColor::Static`] override collapses to a degenerate dynamic
/// color whose 8 variants are all the same value, so downstream code that
/// expects a [`CupertinoDynamicColor`] (like
/// [`CupertinoTextThemeData::with_primary_color`]) never needs to branch on
/// which variant produced it.
fn as_dynamic(color: CupertinoColor) -> CupertinoDynamicColor {
    match color {
        CupertinoColor::Dynamic(dynamic) => dynamic,
        CupertinoColor::Static(concrete) => {
            CupertinoDynamicColor::with_brightness(concrete, concrete)
        }
    }
}

/// Styling specification for a [`CupertinoTheme`].
///
/// Every field is optional; an unset field falls back to the oracle's
/// `_kDefaultTheme` iOS defaults (systemBlue primary, white contrasting,
/// systemBackground scaffold, a translucent navigation-bar background).
///
/// Flutter parity: `CupertinoThemeData` (`cupertino/theme.dart`, oracle tag
/// `3.44.0`), scoped to this crate's V1 consumers. **Named deferral**:
/// `selectionHandleColor` and `applyThemeToAll` are dropped — no
/// `CupertinoTextField`/Material-interop consumer exists yet in this crate to
/// pin their shape against; add them alongside whichever component first
/// needs them, per Flutter's own component-slot pattern (see
/// `flui-material::ThemeData`'s equivalent note).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CupertinoThemeData {
    brightness: Option<Brightness>,
    primary_color: Option<CupertinoColor>,
    primary_contrasting_color: Option<CupertinoColor>,
    text_theme: Option<CupertinoTextThemeData>,
    bar_background_color: Option<CupertinoColor>,
    scaffold_background_color: Option<CupertinoColor>,
}

impl CupertinoThemeData {
    /// The default theme — Flutter parity: `CupertinoThemeData()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides [`Self::brightness`] instead of falling back to the ambient
    /// [`MediaQueryData::platform_brightness`](flui_widgets::MediaQueryData::platform_brightness).
    #[must_use]
    pub fn with_brightness(mut self, brightness: Brightness) -> Self {
        self.brightness = Some(brightness);
        self
    }

    /// Overrides [`Self::primary_color`].
    #[must_use]
    pub fn with_primary_color(mut self, primary_color: impl Into<CupertinoColor>) -> Self {
        self.primary_color = Some(primary_color.into());
        self
    }

    /// Overrides [`Self::primary_contrasting_color`].
    #[must_use]
    pub fn with_primary_contrasting_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.primary_contrasting_color = Some(color.into());
        self
    }

    /// Overrides [`Self::text_theme`].
    #[must_use]
    pub fn with_text_theme(mut self, text_theme: CupertinoTextThemeData) -> Self {
        self.text_theme = Some(text_theme);
        self
    }

    /// Overrides [`Self::bar_background_color`].
    #[must_use]
    pub fn with_bar_background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.bar_background_color = Some(color.into());
        self
    }

    /// Overrides [`Self::scaffold_background_color`].
    #[must_use]
    pub fn with_scaffold_background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.scaffold_background_color = Some(color.into());
        self
    }

    /// The explicit brightness override, if any — `None` means "follow
    /// `MediaQuery::platform_brightness`", resolved by
    /// [`CupertinoTheme::maybe_brightness_of`]. Flutter parity: `brightness`.
    #[must_use]
    pub fn brightness(&self) -> Option<Brightness> {
        self.brightness
    }

    /// The theme's primary interactive color — `CupertinoButton`'s default
    /// fill/foreground. Flutter parity: `primaryColor`, default
    /// [`CupertinoColors::SYSTEM_BLUE`].
    #[must_use]
    pub fn primary_color(&self) -> CupertinoColor {
        self.primary_color
            .unwrap_or(CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BLUE))
    }

    /// The color placed on top of [`Self::primary_color`] (e.g.
    /// `CupertinoButton.filled`'s text). Flutter parity:
    /// `primaryContrastingColor`, default [`CupertinoColors::WHITE`].
    #[must_use]
    pub fn primary_contrasting_color(&self) -> CupertinoColor {
        self.primary_contrasting_color
            .unwrap_or(CupertinoColor::Static(CupertinoColors::WHITE))
    }

    /// The type-style roles for this theme. Flutter parity: `textTheme`,
    /// default a [`CupertinoTextThemeData`] whose `primary_color` follows
    /// [`Self::primary_color`].
    #[must_use]
    pub fn text_theme(&self) -> CupertinoTextThemeData {
        self.text_theme.clone().unwrap_or_else(|| {
            CupertinoTextThemeData::default().with_primary_color(as_dynamic(self.primary_color()))
        })
    }

    /// The background color for opaque bars (navigation/tab bars). Flutter
    /// parity: `barBackgroundColor`.
    #[must_use]
    pub fn bar_background_color(&self) -> CupertinoColor {
        self.bar_background_color
            .unwrap_or(CupertinoColor::Dynamic(default_bar_background_color()))
    }

    /// The background color for a full-screen Cupertino scaffold. Flutter
    /// parity: `scaffoldBackgroundColor`, default
    /// [`CupertinoColors::SYSTEM_BACKGROUND`].
    #[must_use]
    pub fn scaffold_background_color(&self) -> CupertinoColor {
        self.scaffold_background_color
            .unwrap_or(CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BACKGROUND))
    }

    /// Returns a copy with every color resolved against `ctx` — see
    /// [`CupertinoTheme::of`]'s doc for why ordinary consumers never call
    /// this directly.
    ///
    /// **Named simplification** vs. the oracle: `CupertinoThemeData.resolveFrom`
    /// keeps the override/default distinction alive internally (so a
    /// still-unset field's *default* also resolves, without materializing an
    /// override). This port instead resolves each getter's current effective
    /// value once and stores it as the new override — externally equivalent
    /// (every getter reads the same resolved color either way), simpler
    /// internally, at the cost of `PartialEq`-visible "was this explicitly
    /// set" round-tripping this crate has no consumer for yet.
    ///
    /// Flutter parity: `CupertinoThemeData.resolveFrom`.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Self {
        Self {
            brightness: self.brightness,
            primary_color: Some(CupertinoColor::Static(self.primary_color().resolve(ctx))),
            primary_contrasting_color: Some(CupertinoColor::Static(
                self.primary_contrasting_color().resolve(ctx),
            )),
            text_theme: Some(self.text_theme().resolve_from(ctx)),
            bar_background_color: Some(CupertinoColor::Static(
                self.bar_background_color().resolve(ctx),
            )),
            scaffold_background_color: Some(CupertinoColor::Static(
                self.scaffold_background_color().resolve(ctx),
            )),
        }
    }
}

/// Provides [`CupertinoThemeData`] to its subtree via FLUI's inherited-data
/// mechanism.
///
/// Flutter parity: `CupertinoTheme` (`cupertino/theme.dart`, oracle tag
/// `3.44.0`).
///
/// # Example
///
/// ```rust
/// use flui_cupertino::{CupertinoTheme, CupertinoThemeData};
/// use flui_widgets::SizedBox;
///
/// let _themed = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
/// ```
#[derive(Clone)]
pub struct CupertinoTheme {
    data: CupertinoThemeData,
    child: BoxedView,
}

impl CupertinoTheme {
    /// Wrap `child` in a `CupertinoTheme` that provides `data` to all
    /// descendants.
    #[must_use]
    pub fn new(data: CupertinoThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            child: child.into_view().boxed(),
        }
    }

    /// Retrieves the [`CupertinoThemeData`] from the closest ancestor
    /// [`CupertinoTheme`], or [`CupertinoThemeData::default`] if there is no
    /// ancestor — resolved against `ctx` either way, so ordinary consumers
    /// always see concrete colors (see [`CupertinoThemeData::resolve_from`]).
    ///
    /// Flutter parity: `CupertinoTheme.of`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> CupertinoThemeData {
        let ambient = ctx.depend_on::<Self, _>(|theme| theme.data.clone());
        ambient.unwrap_or_default().resolve_from(ctx)
    }

    /// The brightness that descendant Cupertino widgets should use: the
    /// nearest ancestor [`CupertinoTheme`]'s explicit
    /// [`CupertinoThemeData::brightness`], falling back to
    /// [`MediaQueryData::platform_brightness`](flui_widgets::MediaQueryData::platform_brightness). Returns `None` if neither is
    /// available.
    ///
    /// Flutter parity: `CupertinoTheme.maybeBrightnessOf`.
    #[must_use]
    pub fn maybe_brightness_of(ctx: &dyn BuildContext) -> Option<Brightness> {
        match ctx.depend_on::<Self, _>(|theme| theme.data.brightness) {
            Some(Some(brightness)) => Some(brightness),
            _ => MediaQuery::maybe_of(ctx).map(|data| data.platform_brightness),
        }
    }

    /// [`Self::maybe_brightness_of`], defaulting to [`Brightness::Light`]
    /// when neither a [`CupertinoTheme`] nor a [`MediaQuery`] ancestor is
    /// present.
    ///
    /// **Documented divergence from Flutter**: the oracle's `brightnessOf`
    /// throws when both are missing; this crate follows the same
    /// light-default fallback [`crate::colors::CupertinoDynamicColor::resolve_from`]
    /// already uses for the identical missing-context case, rather than
    /// introducing a panic path a caller has to specifically avoid.
    ///
    /// Flutter parity: `CupertinoTheme.brightnessOf`.
    #[must_use]
    pub fn brightness_of(ctx: &dyn BuildContext) -> Brightness {
        Self::maybe_brightness_of(ctx).unwrap_or(Brightness::Light)
    }
}

impl std::fmt::Debug for CupertinoTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTheme")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for CupertinoTheme {
    type Data = CupertinoThemeData;

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

impl_inherited_view!(CupertinoTheme);

impl InheritedTheme for CupertinoTheme {
    fn wrap(&self, _ctx: &dyn BuildContext, child: BoxedView) -> BoxedView {
        CupertinoTheme::new(self.data.clone(), child).boxed()
    }
}

#[cfg(test)]
mod tests {
    use flui_widgets::SizedBox;

    use super::*;

    #[test]
    fn default_theme_data_matches_the_oracle_kdefaulttheme() {
        let data = CupertinoThemeData::default();
        assert_eq!(
            data.primary_color(),
            CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BLUE)
        );
        assert_eq!(
            data.primary_contrasting_color(),
            CupertinoColor::Static(CupertinoColors::WHITE)
        );
        assert_eq!(
            data.scaffold_background_color(),
            CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BACKGROUND)
        );
        assert_eq!(
            data.bar_background_color(),
            CupertinoColor::Dynamic(default_bar_background_color())
        );
    }

    #[test]
    fn text_theme_default_follows_primary_color() {
        let data = CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
        assert_eq!(
            data.text_theme().action_text_style().color,
            Some(CupertinoColors::SYSTEM_RED.color)
        );
    }

    #[test]
    fn explicit_text_theme_override_wins() {
        let overridden =
            CupertinoTextThemeData::default().with_primary_color(CupertinoColors::SYSTEM_GREY);
        let data = CupertinoThemeData::default().with_text_theme(overridden.clone());
        assert_eq!(data.text_theme(), overridden);
    }

    #[test]
    fn new_stores_data_and_child() {
        let theme = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
        assert_eq!(theme.data, CupertinoThemeData::default());
    }

    #[test]
    fn update_should_notify_true_when_data_differs() {
        let a = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
        let b = CupertinoTheme::new(
            CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED),
            SizedBox::shrink(),
        );
        assert!(a.update_should_notify(&b));
    }

    #[test]
    fn update_should_notify_false_when_data_equal() {
        let a = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
        let b = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
        assert!(!a.update_should_notify(&b));
    }

    #[test]
    fn debug_format_does_not_panic() {
        let theme = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
        let rendered = format!("{theme:?}");
        assert!(rendered.contains("CupertinoTheme"));
    }
}
