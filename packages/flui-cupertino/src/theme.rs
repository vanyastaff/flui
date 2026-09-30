//! [`CupertinoTheme`] — publishes [`CupertinoThemeData`] to a subtree via
//! FLUI's inherited-data mechanism.
//!
//! Like `flui-material`'s `Theme`, this is one `InheritedView` type. It does
//! not also imply an `IconTheme`, which this crate's V1 doesn't yet wire (no
//! icon-family component consumes it).
//!
//! ## Material-interop seam (nothing owed here)
//!
//! `CupertinoTheme::of` never reads Material. A Material theme that wants to
//! also drive Cupertino widgets underneath it would have to inject a
//! `CupertinoTheme` from *its* side. Per ADR-0028, that injection seam belongs
//! to a future `flui-material` increment, not this crate — `flui-cupertino`
//! has no dependency on `flui-material` and nothing here needs to change to
//! support it later.

use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};
use flui_sdk::widgets::{InheritedTheme, MediaQuery};

use crate::colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::text_theme::CupertinoTextThemeData;

/// The default bar background — `0xF0F9F9F9` / `0xF01D1D1D` (navigation-bar
/// translucent background; toolbars and tab bars could use a darker
/// `0xF0161616`, which is not used here).
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
/// Every field is optional; an unset field falls back to the iOS defaults
/// (system blue primary, white contrasting, system background scaffold, a
/// translucent navigation-bar background).
///
/// Scoped to this crate's V1 consumers. **Named deferral**: a selection
/// handle color and an apply-theme-to-all flag are dropped — no
/// `CupertinoTextField`/Material-interop consumer exists yet in this crate to
/// pin their shape against; add them alongside whichever component first
/// needs them (see `flui-material::ThemeData`'s equivalent note).
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
    /// The default theme.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides [`Self::brightness`] instead of falling back to the ambient
    /// [`MediaQueryData::platform_brightness`](flui_sdk::widgets::MediaQueryData::platform_brightness).
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
    /// [`CupertinoTheme::maybe_brightness_of`].
    #[must_use]
    pub fn brightness(&self) -> Option<Brightness> {
        self.brightness
    }

    /// The theme's primary interactive color — `CupertinoButton`'s default
    /// fill/foreground. Default [`CupertinoColors::SYSTEM_BLUE`].
    #[must_use]
    pub fn primary_color(&self) -> CupertinoColor {
        self.primary_color
            .unwrap_or(CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BLUE))
    }

    /// The color placed on top of [`Self::primary_color`] (e.g.
    /// `CupertinoButton::filled`'s text). Default [`CupertinoColors::WHITE`].
    #[must_use]
    pub fn primary_contrasting_color(&self) -> CupertinoColor {
        self.primary_contrasting_color
            .unwrap_or(CupertinoColor::Static(CupertinoColors::WHITE))
    }

    /// The type-style roles for this theme. Default a
    /// [`CupertinoTextThemeData`] whose `primary_color` follows
    /// [`Self::primary_color`].
    #[must_use]
    pub fn text_theme(&self) -> CupertinoTextThemeData {
        self.text_theme.clone().unwrap_or_else(|| {
            CupertinoTextThemeData::default().with_primary_color(as_dynamic(self.primary_color()))
        })
    }

    /// The background color for opaque bars (navigation/tab bars).
    #[must_use]
    pub fn bar_background_color(&self) -> CupertinoColor {
        self.bar_background_color
            .unwrap_or(CupertinoColor::Dynamic(default_bar_background_color()))
    }

    /// The background color for a full-screen Cupertino scaffold. Default
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
    /// **Named simplification**: this resolves each getter's current effective
    /// value once and stores it as the new override, rather than keeping the
    /// override/default distinction alive (so a still-unset field's *default*
    /// also resolves, without materializing an override). Externally
    /// equivalent (every getter reads the same resolved color either way),
    /// simpler internally, at the cost of `PartialEq`-visible "was this
    /// explicitly set" round-tripping this crate has no consumer for yet.
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
/// # Example
///
/// ```rust
/// use flui_cupertino::{CupertinoTheme, CupertinoThemeData};
/// use flui_sdk::widgets::SizedBox;
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
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> CupertinoThemeData {
        let ambient = ctx.depend_on::<Self, _>(|theme| theme.data.clone());
        ambient.unwrap_or_default().resolve_from(ctx)
    }

    /// The brightness that descendant Cupertino widgets should use: the
    /// nearest ancestor [`CupertinoTheme`]'s explicit
    /// [`CupertinoThemeData::brightness`], falling back to
    /// [`MediaQueryData::platform_brightness`](flui_sdk::widgets::MediaQueryData::platform_brightness). Returns `None` if neither is
    /// available.
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
    /// This follows the same light-default fallback
    /// [`crate::colors::CupertinoDynamicColor::resolve_from`] already uses for
    /// the identical missing-context case, rather than introducing a panic
    /// path a caller has to specifically avoid.
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
