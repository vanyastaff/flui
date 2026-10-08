//! [`CupertinoDynamicColor`] and [`CupertinoColors`] — the iOS
//! brightness/contrast/elevation-adaptive color system.

use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::BuildContext;
use flui_sdk::widgets::MediaQuery;

// =============================================================================
// CupertinoColor — a literal color or a dynamic one
// =============================================================================

/// A color that is either a concrete, already-resolved [`Color`] or a
/// [`CupertinoDynamicColor`] still waiting to be resolved against a
/// [`BuildContext`].
///
/// Fields such as `CupertinoButton::color` and
/// `CupertinoThemeData::primary_color` must be able to hold either a literal
/// color or a dynamic one. FLUI's [`Color`] is a concrete RGBA struct, not an
/// interface — it cannot carry that polymorphism. This enum makes the two
/// cases explicit at the type level instead of hiding a runtime type-check
/// inside `resolve`: illegal "a `Color` that might secretly be something
/// else" states are unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CupertinoColor {
    /// Already a concrete color — `resolve` returns it unchanged.
    Static(Color),
    /// A dynamic color — `resolve` looks up brightness and contrast from the
    /// ambient context. Interface elevation remains at its base value.
    Dynamic(CupertinoDynamicColor),
}

impl CupertinoColor {
    /// Resolves this color against `ctx` — a concrete [`Color`] is returned
    /// as-is; a [`CupertinoDynamicColor`] is resolved via
    /// [`CupertinoDynamicColor::resolve_from`].
    #[must_use]
    pub fn resolve(&self, ctx: &dyn BuildContext) -> Color {
        ColorResolver::new(ctx, None).resolve(*self)
    }
}

impl From<Color> for CupertinoColor {
    fn from(color: Color) -> Self {
        Self::Static(color)
    }
}

impl From<CupertinoDynamicColor> for CupertinoColor {
    fn from(dynamic: CupertinoDynamicColor) -> Self {
        Self::Dynamic(dynamic)
    }
}

// =============================================================================
// CupertinoDynamicColor
// =============================================================================

/// A color that adapts to the ambient brightness and contrast of the
/// [`BuildContext`] it is resolved against.
///
/// The data is the full 8-variant struct: light/dark, each at normal or high
/// contrast, each at base or elevated interface level.
///
/// ## Resolution scope
///
/// [`resolve_from`](Self::resolve_from) resolves brightness from
/// `CupertinoTheme`'s ambient `brightness` field,
/// falling back to `MediaQuery::platform_brightness` when no `CupertinoTheme`
/// ancestor sets one. Contrast follows [`MediaQuery::high_contrast_of`],
/// defaulting to normal contrast without an ancestor. Explicit theme brightness
/// does not mask the contrast preference. Interface elevation remains at base
/// elevation because no interface-level ambient is implemented yet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CupertinoDynamicColor {
    /// Light mode, normal contrast, base elevation.
    pub color: Color,
    /// Dark mode, normal contrast, base elevation.
    pub dark_color: Color,
    /// Light mode, high contrast, base elevation.
    pub high_contrast_color: Color,
    /// Dark mode, high contrast, base elevation.
    pub dark_high_contrast_color: Color,
    /// Light mode, normal contrast, elevated interface level.
    pub elevated_color: Color,
    /// Dark mode, normal contrast, elevated interface level.
    pub dark_elevated_color: Color,
    /// Light mode, high contrast, elevated interface level.
    pub high_contrast_elevated_color: Color,
    /// Dark mode, high contrast, elevated interface level.
    pub dark_high_contrast_elevated_color: Color,
}

impl CupertinoDynamicColor {
    /// Full 8-variant constructor.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "the full 8-variant constructor — a builder would obscure \
                  the const-table call sites below, which are meant to read as a direct table"
    )]
    pub const fn new(
        color: Color,
        dark_color: Color,
        high_contrast_color: Color,
        dark_high_contrast_color: Color,
        elevated_color: Color,
        dark_elevated_color: Color,
        high_contrast_elevated_color: Color,
        dark_high_contrast_elevated_color: Color,
    ) -> Self {
        Self {
            color,
            dark_color,
            high_contrast_color,
            dark_high_contrast_color,
            elevated_color,
            dark_elevated_color,
            high_contrast_elevated_color,
            dark_high_contrast_elevated_color,
        }
    }

    /// A color that varies by brightness and contrast, but not interface
    /// elevation (the elevated variants mirror the base ones).
    #[must_use]
    pub const fn with_brightness_and_contrast(
        color: Color,
        dark_color: Color,
        high_contrast_color: Color,
        dark_high_contrast_color: Color,
    ) -> Self {
        Self::new(
            color,
            dark_color,
            high_contrast_color,
            dark_high_contrast_color,
            color,
            dark_color,
            high_contrast_color,
            dark_high_contrast_color,
        )
    }

    /// A color that varies by brightness only.
    #[must_use]
    pub const fn with_brightness(color: Color, dark_color: Color) -> Self {
        Self::with_brightness_and_contrast(color, dark_color, color, dark_color)
    }

    /// Whether any variant differs across the light/dark axis — gates whether
    /// `resolve_from` even needs to look up `CupertinoTheme`/`MediaQuery`.
    fn is_platform_brightness_dependent(&self) -> bool {
        self.color != self.dark_color
            || self.elevated_color != self.dark_elevated_color
            || self.high_contrast_color != self.dark_high_contrast_color
            || self.high_contrast_elevated_color != self.dark_high_contrast_elevated_color
    }

    /// Resolves this dynamic color against `ctx`, per the "Resolution scope"
    /// section on the type doc: brightness and contrast resolved, elevation
    /// at its base variant. Only fields that affect the color are dependencies.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Color {
        ColorResolver::new(ctx, None).dynamic(*self)
    }

    /// Resolves `resolvable` by calling [`CupertinoColor::resolve`] — a
    /// concrete [`CupertinoColor::Static`] is returned unchanged, a
    /// [`CupertinoColor::Dynamic`] is resolved against `ctx`.
    ///
    /// Kept on this type for discoverability — see [`CupertinoColor`]'s doc
    /// for why the parameter is an enum rather than a plain `Color`.
    #[must_use]
    pub fn resolve(resolvable: CupertinoColor, ctx: &dyn BuildContext) -> Color {
        resolvable.resolve(ctx)
    }

    /// [`resolve`](Self::resolve), but for an `Option`.
    #[must_use]
    pub fn maybe_resolve(
        resolvable: Option<CupertinoColor>,
        ctx: &dyn BuildContext,
    ) -> Option<Color> {
        resolvable.map(|color| color.resolve(ctx))
    }
}

/// Shares precedence and selective inherited reads between standalone colors,
/// theme materialization and text roles. A theme being resolved need not already
/// be installed as an ancestor for its explicit brightness to take effect.
pub(crate) struct ColorResolver<'a> {
    context: &'a dyn BuildContext,
    brightness: Option<Brightness>,
}

impl<'a> ColorResolver<'a> {
    pub(crate) fn new(context: &'a dyn BuildContext, brightness: Option<Brightness>) -> Self {
        Self {
            context,
            brightness,
        }
    }

    pub(crate) fn resolve(&self, color: CupertinoColor) -> Color {
        match color {
            CupertinoColor::Static(color) => color,
            CupertinoColor::Dynamic(color) => self.dynamic(color),
        }
    }

    pub(crate) fn dynamic(&self, color: CupertinoDynamicColor) -> Color {
        let brightness = if color.is_platform_brightness_dependent() {
            self.brightness
                .or_else(|| crate::theme::CupertinoTheme::maybe_brightness_of(self.context))
                .unwrap_or(Brightness::Light)
        } else {
            Brightness::Light
        };
        let (normal, contrast) = match brightness {
            Brightness::Light => (color.color, color.high_contrast_color),
            Brightness::Dark => (color.dark_color, color.dark_high_contrast_color),
        };
        if normal != contrast && MediaQuery::high_contrast_of(self.context).unwrap_or(false) {
            contrast
        } else {
            normal
        }
    }
}

// =============================================================================
// CupertinoColors — the named palette
// =============================================================================

/// The System, Label, Fill, and Background color palettes from the iOS 13+
/// UIKit `UIColor` catalog, scoped to this crate's V1 consumers
/// ([`crate::theme`]'s defaults, [`crate::text_theme`]'s label/action colors,
/// [`crate::button`]'s fill/disabled/foreground colors).
///
/// Mounted resolution is exercised in `tests/colors.rs`; application-level
/// publication of the blue variants is exercised in `tests/cupertino_app.rs`.
#[derive(Debug)]
#[non_exhaustive]
pub struct CupertinoColors;

impl CupertinoColors {
    /// Pure opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Pure opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Fully transparent.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// The disabled-button gray. Not the same gray as
    /// [`Self::SYSTEM_GREY`].
    pub const INACTIVE_GRAY: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::rgb(0x99, 0x99, 0x99),
        Color::rgb(0x75, 0x75, 0x75),
    );

    /// A blue that can adapt to the given context.
    pub const SYSTEM_BLUE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(0, 122, 255),
            Color::rgb(10, 132, 255),
            Color::rgb(0, 64, 221),
            Color::rgb(64, 156, 255),
        );

    /// Alias for [`Self::SYSTEM_BLUE`].
    pub const ACTIVE_BLUE: CupertinoDynamicColor = Self::SYSTEM_BLUE;

    /// A red used for destructive actions.
    pub const SYSTEM_RED: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 59, 48),
            Color::rgb(255, 69, 58),
            Color::rgb(215, 0, 21),
            Color::rgb(255, 105, 97),
        );

    /// Alias for [`Self::SYSTEM_RED`].
    pub const DESTRUCTIVE_RED: CupertinoDynamicColor = Self::SYSTEM_RED;

    /// The base gray.
    pub const SYSTEM_GREY: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(142, 142, 147),
            Color::rgb(142, 142, 147),
            Color::rgb(108, 108, 112),
            Color::rgb(174, 174, 178),
        );

    /// A second-level shade of grey.
    pub const SYSTEM_GREY2: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(174, 174, 178),
            Color::rgb(99, 99, 102),
            Color::rgb(142, 142, 147),
            Color::rgb(124, 124, 128),
        );

    /// A third-level shade of grey.
    pub const SYSTEM_GREY3: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(199, 199, 204),
            Color::rgb(72, 72, 74),
            Color::rgb(174, 174, 178),
            Color::rgb(84, 84, 86),
        );

    /// A fourth-level shade of grey.
    pub const SYSTEM_GREY4: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(209, 209, 214),
            Color::rgb(58, 58, 60),
            Color::rgb(188, 188, 192),
            Color::rgb(68, 68, 70),
        );

    /// A fifth-level shade of grey.
    pub const SYSTEM_GREY5: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(229, 229, 234),
            Color::rgb(44, 44, 46),
            Color::rgb(216, 216, 220),
            Color::rgb(54, 54, 56),
        );

    /// A sixth-level shade of grey.
    pub const SYSTEM_GREY6: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(242, 242, 247),
            Color::rgb(28, 28, 30),
            Color::rgb(235, 235, 240),
            Color::rgb(36, 36, 38),
        );

    /// Primary-content text labels.
    pub const LABEL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
    );

    /// Secondary-content text labels.
    pub const SECONDARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(60, 60, 67, 153),
        Color::rgba(235, 235, 245, 153),
        Color::rgba(60, 60, 67, 173),
        Color::rgba(235, 235, 245, 173),
        Color::rgba(60, 60, 67, 153),
        Color::rgba(235, 235, 245, 153),
        Color::rgba(60, 60, 67, 173),
        Color::rgba(235, 235, 245, 173),
    );

    /// Tertiary-content text labels.
    pub const TERTIARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(60, 60, 67, 76),
        Color::rgba(235, 235, 245, 76),
        Color::rgba(60, 60, 67, 96),
        Color::rgba(235, 235, 245, 96),
        Color::rgba(60, 60, 67, 76),
        Color::rgba(235, 235, 245, 76),
        Color::rgba(60, 60, 67, 96),
        Color::rgba(235, 235, 245, 96),
    );

    /// The default background for a screen.
    pub const SYSTEM_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgb(255, 255, 255),
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
        Color::rgb(0, 0, 0),
        Color::rgb(255, 255, 255),
        Color::rgb(28, 28, 30),
        Color::rgb(255, 255, 255),
        Color::rgb(36, 36, 38),
    );

    /// Grouped-content background, one level up from
    /// [`Self::SYSTEM_BACKGROUND`].
    pub const SECONDARY_SYSTEM_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgb(242, 242, 247),
        Color::rgb(28, 28, 30),
        Color::rgb(235, 235, 240),
        Color::rgb(36, 36, 38),
        Color::rgb(242, 242, 247),
        Color::rgb(44, 44, 46),
        Color::rgb(235, 235, 240),
        Color::rgb(54, 54, 56),
    );

    /// The color for thin separator lines between content.
    pub const SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(60, 60, 67, 73),
        Color::rgba(84, 84, 88, 153),
        Color::rgba(60, 60, 67, 94),
        Color::rgba(84, 84, 88, 173),
        Color::rgba(60, 60, 67, 73),
        Color::rgba(210, 210, 210, 153),
        Color::rgba(60, 60, 67, 94),
        Color::rgba(84, 84, 88, 173),
    );

    /// An opaque separator, for when translucency is undesirable.
    pub const OPAQUE_SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgb(198, 198, 200),
        Color::rgb(56, 56, 58),
        Color::rgb(198, 198, 200),
        Color::rgb(56, 56, 58),
        Color::rgb(198, 198, 200),
        Color::rgb(56, 56, 58),
        Color::rgb(198, 198, 200),
        Color::rgb(56, 56, 58),
    );

    /// An overlay fill for thin and small shapes.
    pub const SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(120, 120, 128, 51),
        Color::rgba(120, 120, 128, 91),
        Color::rgba(120, 120, 128, 71),
        Color::rgba(120, 120, 128, 112),
        Color::rgba(120, 120, 128, 51),
        Color::rgba(120, 120, 128, 91),
        Color::rgba(120, 120, 128, 71),
        Color::rgba(120, 120, 128, 112),
    );

    /// An overlay fill for medium-size shapes.
    pub const SECONDARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(120, 120, 128, 40),
        Color::rgba(120, 120, 128, 81),
        Color::rgba(120, 120, 128, 61),
        Color::rgba(120, 120, 128, 102),
        Color::rgba(120, 120, 128, 40),
        Color::rgba(120, 120, 128, 81),
        Color::rgba(120, 120, 128, 61),
        Color::rgba(120, 120, 128, 102),
    );

    /// An overlay fill for large shapes.
    pub const TERTIARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(118, 118, 128, 30),
        Color::rgba(118, 118, 128, 61),
        Color::rgba(118, 118, 128, 51),
        Color::rgba(118, 118, 128, 81),
        Color::rgba(118, 118, 128, 30),
        Color::rgba(118, 118, 128, 61),
        Color::rgba(118, 118, 128, 51),
        Color::rgba(118, 118, 128, 81),
    );

    /// An overlay fill for the largest shapes — `CupertinoButton`'s default
    /// `disabledColor` for its plain (no-background) style.
    pub const QUATERNARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::new(
        Color::rgba(116, 116, 128, 20),
        Color::rgba(118, 118, 128, 45),
        Color::rgba(116, 116, 128, 40),
        Color::rgba(118, 118, 128, 66),
        Color::rgba(116, 116, 128, 20),
        Color::rgba(118, 118, 128, 45),
        Color::rgba(116, 116, 128, 40),
        Color::rgba(118, 118, 128, 66),
    );
}
