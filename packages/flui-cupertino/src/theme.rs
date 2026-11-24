//! The inherited theme, the data an author writes and the theme widgets read.
//!
//! `CupertinoTheme::of` never reads Material: ADR-0028 keeps the two design
//! systems independent, so a Material theme that should also drive Cupertino
//! widgets installs a `CupertinoTheme` from its own side.

use std::cell::RefCell;
use std::rc::Rc;

use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};
use flui_sdk::widgets::InheritedTheme;

use crate::colors::{ColorResolver, CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::interface_level::CupertinoUserInterfaceLevel;
use crate::text_theme::{CupertinoTextThemeData, ResolvedCupertinoTextTheme};

/// The default bar background — `0xF0F9F9F9` / `0xF01D1D1D` (navigation-bar
/// translucent background; toolbars and tab bars could use a darker
/// `0xF0161616`, which is not used here).
const DEFAULT_BAR_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::rgba(0xF9, 0xF9, 0xF9, 0xF0),
    Color::rgba(0x1D, 0x1D, 0x1D, 0xF0),
);

/// How many resolutions of one [`CupertinoTheme`] its cache keeps: the eight
/// brightness, contrast and interface-level combinations. A key under another
/// system accent evicts the oldest entry, so flipping settings never grows the
/// cache.
const RESOLUTION_CACHE_CAPACITY: usize = 8;

/// Styling specification for a [`CupertinoTheme`]: what an author writes.
///
/// Every field is public and every `None` follows the iOS default: start from
/// [`Default::default`] and assign the fields to change. Widgets never read
/// this type; they read the [`ResolvedCupertinoTheme`] that
/// [`CupertinoTheme::of`] returns, whose colours are concrete for their
/// context.
///
/// The theme has no selection handle color and no apply-theme-to-all flag,
/// and it does not imply an `IconTheme`.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/theme.dart:175`.
///
/// # Example
///
/// ```rust
/// use flui_cupertino::{CupertinoColors, CupertinoThemeData};
/// use flui_sdk::platform::Brightness;
///
/// let mut theme = CupertinoThemeData::default();
/// theme.brightness = Some(Brightness::Dark);
/// theme.primary_color = Some(CupertinoColors::SYSTEM_RED.into());
/// assert_eq!(theme.text_theme.primary_color, None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CupertinoThemeData {
    /// The brightness every colour resolves for. `None`: an ancestor
    /// [`CupertinoTheme`]'s brightness, else the ambient
    /// [`MediaQueryData::platform_brightness`](flui_sdk::view::environment::MediaQueryData::platform_brightness),
    /// else light.
    pub brightness: Option<Brightness>,
    /// The primary interactive colour: `CupertinoButton`'s default
    /// foreground and fill, the active tab. `None`: the system accent when
    /// the host observes one ([`MediaQuery::accent_color_of`]: macOS's
    /// control accent, Windows' accent, the desktop portal's) and the
    /// theme's brightness is the platform's, as AppKit tints its controls;
    /// otherwise [`CupertinoColors::SYSTEM_BLUE`]. The host publishes the
    /// accent resolved for the platform's appearance, so a theme forced to
    /// the other brightness keeps system blue with its dark and
    /// high-contrast variants (ADR-0359 §6).
    pub primary_color: Option<CupertinoColor>,
    /// The colour drawn on the primary colour (`CupertinoButton::filled`'s
    /// text). `None`: [`CupertinoColors::WHITE`], or on the system accent
    /// whichever of white and black contrasts more with it.
    pub primary_contrasting_color: Option<CupertinoColor>,
    /// The text-style roles. A role left `None` takes its iOS default, and
    /// an unset [`CupertinoTextThemeData::primary_color`] follows this
    /// theme's resolved primary colour.
    pub text_theme: CupertinoTextThemeData,
    /// The background of navigation and tab bars. `None`: a translucent
    /// `0xF0F9F9F9` in light, `0xF01D1D1D` in dark.
    pub bar_background_color: Option<CupertinoColor>,
    /// The background of a full-screen Cupertino scaffold. `None`:
    /// [`CupertinoColors::SYSTEM_BACKGROUND`].
    pub scaffold_background_color: Option<CupertinoColor>,
}

/// A theme resolved for one context: what Cupertino widgets read.
///
/// Every colour is concrete and every text role is a [`TextStyle`](flui_sdk::painting::typography::TextStyle)
/// built for the brightness, interface level, contrast and system accent the
/// context had.
/// Flutter's `CupertinoTheme.of` returns the same data type the author wrote,
/// passed through `resolveFrom(context)`
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/theme.dart:72`).
/// Here the resolved theme is a type of its own, because a concrete colour is
/// a different type from a dynamic one: a widget reads a [`Color`] with no
/// `Option` and nothing left to resolve, and a resolved theme can neither be
/// resolved a second time nor compared with the data it came from by mistake.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCupertinoTheme {
    /// The brightness the colours were resolved for.
    pub brightness: Brightness,
    /// See [`CupertinoThemeData::primary_color`].
    pub primary_color: Color,
    /// See [`CupertinoThemeData::primary_contrasting_color`].
    pub primary_contrasting_color: Color,
    /// See [`CupertinoThemeData::bar_background_color`].
    pub bar_background_color: Color,
    /// See [`CupertinoThemeData::scaffold_background_color`].
    pub scaffold_background_color: Color,
    /// The text-style roles, each resolved.
    pub text_theme: ResolvedCupertinoTextTheme,
}

/// Everything a theme's resolution reads from its context, so that a
/// resolution is a function of the data and this value alone and a cached
/// one is reused only under the inputs it was built from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ResolutionInputs {
    /// The effective brightness, the interface level and the high-contrast
    /// preference.
    colors: ColorResolver,
    /// The system accent when it applies to this theme (an unset primary
    /// colour, the theme's brightness the platform's), else `None`.
    system_accent: Option<Color>,
}

impl CupertinoThemeData {
    /// This theme with every colour resolved against `ctx` and every text
    /// role built. [`CupertinoTheme::of`] returns the resolution of the
    /// nearest theme, shared; call this for data that is not installed, such
    /// as an app shell's own theme before it publishes it.
    ///
    /// Registers the dependencies the resolution reads: the effective
    /// brightness (an unset one reads an ancestor theme's, then the
    /// platform's), the interface level, the high-contrast preference and,
    /// for an unset primary colour, the system accent.
    #[must_use]
    pub fn resolve(&self, ctx: &dyn BuildContext) -> ResolvedCupertinoTheme {
        self.resolve_for(self.inputs(ctx, CupertinoTheme::maybe_brightness_of))
    }

    /// Reads the resolution inputs from `ctx`; an unset brightness falls
    /// back to `ambient_brightness`, then light.
    fn inputs(
        &self,
        ctx: &dyn BuildContext,
        ambient_brightness: impl FnOnce(&dyn BuildContext) -> Option<Brightness>,
    ) -> ResolutionInputs {
        let brightness = self
            .brightness
            .or_else(|| ambient_brightness(ctx))
            .unwrap_or(Brightness::Light);
        let level = CupertinoUserInterfaceLevel::of(ctx);
        let high_contrast = MediaQuery::high_contrast_of(ctx).unwrap_or(false);
        let system_accent = self
            .primary_color
            .is_none()
            .then(|| MediaQuery::accent_color_of(ctx))
            .flatten()
            .filter(|_| {
                self.brightness.is_none_or(|brightness| {
                    Some(brightness) == MediaQuery::platform_brightness_of(ctx)
                })
            });
        ResolutionInputs {
            colors: ColorResolver {
                brightness,
                level,
                high_contrast,
            },
            system_accent,
        }
    }

    /// The resolution under `inputs`; reads no context.
    fn resolve_for(&self, inputs: ResolutionInputs) -> ResolvedCupertinoTheme {
        let colors = inputs.colors;
        let primary_color = match (self.primary_color, inputs.system_accent) {
            (Some(color), _) => colors.resolve(color),
            (None, Some(accent)) => accent,
            (None, None) => colors.dynamic(CupertinoColors::SYSTEM_BLUE),
        };
        let primary_contrasting_color = match (self.primary_contrasting_color, inputs.system_accent)
        {
            (Some(color), _) => colors.resolve(color),
            (None, Some(accent)) => accent.contrasting_text_color(),
            (None, None) => CupertinoColors::WHITE,
        };
        ResolvedCupertinoTheme {
            brightness: colors.brightness,
            primary_color,
            primary_contrasting_color,
            bar_background_color: colors.resolve(
                self.bar_background_color
                    .unwrap_or(CupertinoColor::Dynamic(DEFAULT_BAR_BACKGROUND)),
            ),
            scaffold_background_color: colors.resolve(
                self.scaffold_background_color
                    .unwrap_or(CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BACKGROUND)),
            ),
            text_theme: self.text_theme.resolve_with(colors, primary_color),
        }
    }
}

/// Provides [`CupertinoThemeData`] to its subtree, and its resolution through
/// [`Self::of`].
///
/// The theme keeps the resolutions its readers asked for, keyed by the
/// inputs each was built from (the effective brightness, the interface
/// level, the contrast preference and the system accent that applies), so
/// every reader under the same inputs shares one [`ResolvedCupertinoTheme`]
/// and a reader under an elevated level gets a resolution of its own. The
/// cache belongs to the view value: a clone of the view, or the copy
/// [`InheritedTheme::wrap`] installs elsewhere, shares it, and a new view
/// built with [`Self::new`] starts empty. It holds at most eight entries.
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
    source: Rc<ThemeSource>,
    child: BoxedView,
}

/// A theme's data and the resolutions of it already built, shared by every
/// copy of the view.
#[derive(Debug)]
struct ThemeSource {
    data: CupertinoThemeData,
    /// Oldest first, at most [`RESOLUTION_CACHE_CAPACITY`] entries, no two
    /// with equal inputs.
    resolutions: RefCell<Vec<(ResolutionInputs, Rc<ResolvedCupertinoTheme>)>>,
}

impl ThemeSource {
    /// The resolution under `inputs`, built and kept on a miss.
    fn resolution(&self, inputs: ResolutionInputs) -> Rc<ResolvedCupertinoTheme> {
        let cached = self
            .resolutions
            .borrow()
            .iter()
            .find(|(key, _)| *key == inputs)
            .map(|(_, resolved)| Rc::clone(resolved));
        if let Some(resolved) = cached {
            return resolved;
        }
        // Resolution reads no context and runs no code of the caller's, but
        // it runs with the cache unborrowed all the same.
        let resolved = Rc::new(self.data.resolve_for(inputs));
        let evicted = {
            let mut resolutions = self.resolutions.borrow_mut();
            let evicted =
                (resolutions.len() >= RESOLUTION_CACHE_CAPACITY).then(|| resolutions.remove(0));
            resolutions.push((inputs, Rc::clone(&resolved)));
            evicted
        };
        drop(evicted);
        resolved
    }
}

impl CupertinoTheme {
    /// Wrap `child` in a `CupertinoTheme` that provides `data` to all
    /// descendants.
    #[must_use]
    pub fn new(data: CupertinoThemeData, child: impl IntoView) -> Self {
        Self {
            source: Rc::new(ThemeSource {
                data,
                resolutions: RefCell::new(Vec::with_capacity(RESOLUTION_CACHE_CAPACITY)),
            }),
            child: child.into_view().boxed(),
        }
    }

    /// The nearest [`CupertinoTheme`]'s data resolved for `ctx`, shared:
    /// two reads under the same brightness, level, contrast and accent return the
    /// same `Rc`, and a change of any of them rebuilds the reader, which
    /// then reads the resolution for the new inputs. Without an ancestor
    /// theme, the default data is resolved afresh on each call.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Rc<ResolvedCupertinoTheme> {
        // The theme read is the nearest one, whose own brightness `inputs`
        // already tried, so an unset brightness falls back to the platform's.
        if let Some(source) = ctx.depend_on::<Self, _>(|theme| Rc::clone(&theme.source)) {
            return source.resolution(source.data.inputs(ctx, MediaQuery::platform_brightness_of));
        }
        let data = CupertinoThemeData::default();
        Rc::new(data.resolve_for(data.inputs(ctx, MediaQuery::platform_brightness_of)))
    }

    /// The brightness that descendant Cupertino widgets should use: the
    /// nearest ancestor [`CupertinoTheme`]'s explicit
    /// [`CupertinoThemeData::brightness`], falling back to
    /// [`MediaQueryData::platform_brightness`](flui_sdk::view::environment::MediaQueryData::platform_brightness). Returns `None` if neither is
    /// available.
    #[must_use]
    pub fn maybe_brightness_of(ctx: &dyn BuildContext) -> Option<Brightness> {
        match ctx.depend_on::<Self, _>(|theme| theme.source.data.brightness) {
            Some(Some(brightness)) => Some(brightness),
            _ => MediaQuery::platform_brightness_of(ctx),
        }
    }

    /// [`Self::maybe_brightness_of`], defaulting to [`Brightness::Light`]
    /// when neither a [`CupertinoTheme`] nor a [`MediaQuery`] ancestor is
    /// present.
    ///
    /// This follows the same light-default fallback
    /// [`crate::CupertinoDynamicColor::resolve_from`] already uses for
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
            .field("data", &self.source.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for CupertinoTheme {
    type Data = CupertinoThemeData;

    fn data(&self) -> &Self::Data {
        &self.source.data
    }

    fn child(&self) -> &BoxedView {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.source, &old.source) && self.source.data != old.source.data
    }
}

impl_inherited_view!(CupertinoTheme);

impl InheritedTheme for CupertinoTheme {
    fn wrap(&self, _ctx: &dyn BuildContext, child: BoxedView) -> BoxedView {
        Self {
            source: Rc::clone(&self.source),
            child,
        }
        .boxed()
    }
}
