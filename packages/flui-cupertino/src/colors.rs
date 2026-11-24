//! The iOS brightness-, contrast- and level-adaptive color system:
//! [`CupertinoColor`], [`CupertinoDynamicColor`] and [`CupertinoColors`].

use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::BuildContext;

use crate::interface_level::{CupertinoUserInterfaceLevel, CupertinoUserInterfaceLevelData};

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
    /// A dynamic color — `resolve` looks up brightness, contrast and
    /// interface level from the ambient context.
    Dynamic(CupertinoDynamicColor),
}

impl CupertinoColor {
    /// Resolves this color against `ctx` — a concrete [`Color`] is returned
    /// as-is; a [`CupertinoDynamicColor`] is resolved via
    /// [`CupertinoDynamicColor::resolve_from`].
    #[must_use]
    pub fn resolve(&self, ctx: &dyn BuildContext) -> Color {
        match *self {
            Self::Static(color) => color,
            Self::Dynamic(color) => color.resolve_from(ctx),
        }
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

/// A color that adapts to the ambient brightness, contrast and interface
/// level of the [`BuildContext`] it is resolved against.
///
/// The data is the full 8-variant struct: light/dark, each at normal or high
/// contrast, each at the base or the elevated interface level.
///
/// ## Construction
///
/// [`Self::with_brightness`] and [`Self::with_brightness_and_contrast`] build
/// a colour whose elevated variants equal its base ones. A colour whose
/// elevated variants differ is a struct literal naming all eight fields, as
/// the [`CupertinoColors`] tables are: each variant is set by its name, so a
/// swapped pair reads wrong instead of compiling unnoticed. The struct is not
/// `#[non_exhaustive]` so that an application can write that literal; the
/// eight fields are the whole of UIKit's trait space for a colour
/// (brightness, contrast, level).
///
/// ## Resolution
///
/// [`resolve_from`](Self::resolve_from) resolves brightness from
/// `CupertinoTheme`'s ambient `brightness` field, falling back to
/// `MediaQuery::platform_brightness` when no `CupertinoTheme` ancestor sets
/// one. The level follows the nearest [`CupertinoUserInterfaceLevel`],
/// defaulting to [`CupertinoUserInterfaceLevelData::Base`] without one.
/// Contrast follows [`MediaQuery::high_contrast_of`], defaulting to normal
/// contrast without an ancestor. Explicit theme brightness does not mask the
/// contrast preference.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/colors.dart:748`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CupertinoDynamicColor {
    /// Light mode, normal contrast, base level.
    pub color: Color,
    /// Dark mode, normal contrast, base level.
    pub dark_color: Color,
    /// Light mode, high contrast, base level.
    pub high_contrast_color: Color,
    /// Dark mode, high contrast, base level.
    pub dark_high_contrast_color: Color,
    /// Light mode, normal contrast, elevated level.
    pub elevated_color: Color,
    /// Dark mode, normal contrast, elevated level.
    pub dark_elevated_color: Color,
    /// Light mode, high contrast, elevated level.
    pub high_contrast_elevated_color: Color,
    /// Dark mode, high contrast, elevated level.
    pub dark_high_contrast_elevated_color: Color,
}

impl CupertinoDynamicColor {
    /// A color that varies by brightness and contrast, but not interface
    /// level (the elevated variants equal the base ones).
    #[must_use]
    pub const fn with_brightness_and_contrast(
        color: Color,
        dark_color: Color,
        high_contrast_color: Color,
        dark_high_contrast_color: Color,
    ) -> Self {
        Self {
            color,
            dark_color,
            high_contrast_color,
            dark_high_contrast_color,
            elevated_color: color,
            dark_elevated_color: dark_color,
            high_contrast_elevated_color: high_contrast_color,
            dark_high_contrast_elevated_color: dark_high_contrast_color,
        }
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

    /// Whether any variant differs across the base/elevated axis — gates
    /// whether `resolve_from` reads the [`CupertinoUserInterfaceLevel`].
    fn is_interface_level_dependent(&self) -> bool {
        self.color != self.elevated_color
            || self.dark_color != self.dark_elevated_color
            || self.high_contrast_color != self.high_contrast_elevated_color
            || self.dark_high_contrast_color != self.dark_high_contrast_elevated_color
    }

    /// The variants at `brightness` and `level`: normal contrast, then high
    /// contrast.
    fn variants(
        &self,
        brightness: Brightness,
        level: CupertinoUserInterfaceLevelData,
    ) -> (Color, Color) {
        match (brightness, level) {
            (Brightness::Light, CupertinoUserInterfaceLevelData::Base) => {
                (self.color, self.high_contrast_color)
            }
            (Brightness::Dark, CupertinoUserInterfaceLevelData::Base) => {
                (self.dark_color, self.dark_high_contrast_color)
            }
            (Brightness::Light, CupertinoUserInterfaceLevelData::Elevated) => {
                (self.elevated_color, self.high_contrast_elevated_color)
            }
            (Brightness::Dark, CupertinoUserInterfaceLevelData::Elevated) => (
                self.dark_elevated_color,
                self.dark_high_contrast_elevated_color,
            ),
        }
    }

    /// Resolves this dynamic color against `ctx`, per the "Resolution"
    /// section on the type doc. Only the ambient values that pick between
    /// this color's variants are dependencies.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Color {
        ColorResolver::ambient_for(ctx, *self).dynamic(*self)
    }
}

/// The ambient inputs a dynamic colour picks its variant by: the effective
/// brightness, the interface level and whether the user asked for high
/// contrast.
///
/// Standalone colours read these from the context selectively
/// ([`Self::ambient_for`]), and a theme reads them once for all its colours
/// and keys its resolution cache by them, so both paths select variants the
/// same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColorResolver {
    pub(crate) brightness: Brightness,
    pub(crate) level: CupertinoUserInterfaceLevelData,
    pub(crate) high_contrast: bool,
}

impl ColorResolver {
    /// The inputs `color` varies along, read from `ctx` and nothing else: a
    /// colour with no dark variant registers no brightness dependency, one
    /// with no elevated variant no level dependency, and one with no
    /// high-contrast variant at the picked brightness and level no contrast
    /// dependency, so an unrelated ambient change does not rebuild its
    /// reader. An input not read is fixed at its default, which picks the
    /// same variant.
    pub(crate) fn ambient_for(ctx: &dyn BuildContext, color: CupertinoDynamicColor) -> Self {
        let brightness = if color.is_platform_brightness_dependent() {
            crate::CupertinoTheme::brightness_of(ctx)
        } else {
            Brightness::Light
        };
        let level = if color.is_interface_level_dependent() {
            CupertinoUserInterfaceLevel::of(ctx)
        } else {
            CupertinoUserInterfaceLevelData::Base
        };
        let (normal, contrast) = color.variants(brightness, level);
        let high_contrast =
            normal != contrast && MediaQuery::high_contrast_of(ctx).unwrap_or(false);
        Self {
            brightness,
            level,
            high_contrast,
        }
    }

    /// `color` under these inputs.
    pub(crate) fn resolve(self, color: CupertinoColor) -> Color {
        match color {
            CupertinoColor::Static(color) => color,
            CupertinoColor::Dynamic(color) => self.dynamic(color),
        }
    }

    /// The variant of `color` these inputs pick.
    pub(crate) fn dynamic(self, color: CupertinoDynamicColor) -> Color {
        let (normal, contrast) = color.variants(self.brightness, self.level);
        if self.high_contrast { contrast } else { normal }
    }
}

// =============================================================================
// CupertinoColors — the named palette
// =============================================================================

/// The iOS 13+ UIKit `UIColor` catalog: the system colours, the label,
/// fill, background and separator colours, and the older fixed greys.
///
/// The values are Flutter's
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/colors.dart:46`).
/// A dynamic constant whose elevated variants are not listed equals its
/// base variants at the elevated level.
#[derive(Debug)]
#[non_exhaustive]
pub struct CupertinoColors;

impl CupertinoColors {
    /// Alias of [`Self::SYSTEM_BLUE`], which it equals: the blue of active elements such as
    /// buttons and selected tabs.
    pub const ACTIVE_BLUE: CupertinoDynamicColor = Self::SYSTEM_BLUE;

    /// Alias of [`Self::SYSTEM_GREEN`], which it equals: the green of active accents such as a
    /// switch that is on.
    pub const ACTIVE_GREEN: CupertinoDynamicColor = Self::SYSTEM_GREEN;

    /// Alias of [`Self::SYSTEM_ORANGE`], which it equals.
    pub const ACTIVE_ORANGE: CupertinoDynamicColor = Self::SYSTEM_ORANGE;

    /// Pure opaque white.
    pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

    /// Pure opaque black.
    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);

    /// Fully transparent.
    pub const TRANSPARENT: Color = Color::rgba(0x00, 0x00, 0x00, 0x00);

    /// The iOS 10 light background fill, such as a chat bubble's (`SystemLightGrayColor`).
    pub const LIGHT_BACKGROUND_GRAY: Color = Color::rgb(0xE5, 0xE5, 0xEA);

    /// The iOS 12 very light fill between the cell groups of a table
    /// (`SystemExtraLightGrayColor`).
    pub const EXTRA_LIGHT_BACKGROUND_GRAY: Color = Color::rgb(0xEF, 0xEF, 0xF4);

    /// The iOS 12 very dark fill between the cell groups of a table in dark mode, a value
    /// Flutter took from a screenshot of the dark Apple Watch app.
    pub const DARK_BACKGROUND_GRAY: Color = Color::rgb(0x17, 0x17, 0x17);

    /// The gray of unselected items, such as an inactive tab bar item, and of de-emphasized
    /// details text. Not the same gray as [`Self::SYSTEM_GREY`].
    pub const INACTIVE_GRAY: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::rgb(0x99, 0x99, 0x99),
        Color::rgb(0x75, 0x75, 0x75),
    );

    /// Alias of [`Self::SYSTEM_RED`], which it equals: the red of destructive actions.
    pub const DESTRUCTIVE_RED: CupertinoDynamicColor = Self::SYSTEM_RED;

    /// The system blue (UIKit's `systemBlue`).
    pub const SYSTEM_BLUE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(0, 122, 255),
            Color::rgb(10, 132, 255),
            Color::rgb(0, 64, 221),
            Color::rgb(64, 156, 255),
        );

    /// The system green (UIKit's `systemGreen`).
    pub const SYSTEM_GREEN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(52, 199, 89),
            Color::rgb(48, 209, 88),
            Color::rgb(36, 138, 61),
            Color::rgb(48, 219, 91),
        );

    /// The system mint (UIKit's `systemMint`).
    pub const SYSTEM_MINT: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(0, 199, 190),
            Color::rgb(99, 230, 226),
            Color::rgb(12, 129, 123),
            Color::rgb(102, 212, 207),
        );

    /// The system indigo (UIKit's `systemIndigo`).
    pub const SYSTEM_INDIGO: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(88, 86, 214),
            Color::rgb(94, 92, 230),
            Color::rgb(54, 52, 163),
            Color::rgb(125, 122, 255),
        );

    /// The system orange (UIKit's `systemOrange`).
    pub const SYSTEM_ORANGE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 149, 0),
            Color::rgb(255, 159, 10),
            Color::rgb(201, 52, 0),
            Color::rgb(255, 179, 64),
        );

    /// The system pink (UIKit's `systemPink`).
    pub const SYSTEM_PINK: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 45, 85),
            Color::rgb(255, 55, 95),
            Color::rgb(211, 15, 69),
            Color::rgb(255, 100, 130),
        );

    /// The system brown (UIKit's `systemBrown`).
    pub const SYSTEM_BROWN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(162, 132, 94),
            Color::rgb(172, 142, 104),
            Color::rgb(127, 101, 69),
            Color::rgb(181, 148, 105),
        );

    /// The system purple (UIKit's `systemPurple`).
    pub const SYSTEM_PURPLE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(175, 82, 222),
            Color::rgb(191, 90, 242),
            Color::rgb(137, 68, 171),
            Color::rgb(218, 143, 255),
        );

    /// The system red (UIKit's `systemRed`).
    pub const SYSTEM_RED: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 59, 48),
            Color::rgb(255, 69, 58),
            Color::rgb(215, 0, 21),
            Color::rgb(255, 105, 97),
        );

    /// The system teal (UIKit's `systemTeal`).
    pub const SYSTEM_TEAL: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(90, 200, 250),
            Color::rgb(100, 210, 255),
            Color::rgb(0, 113, 164),
            Color::rgb(112, 215, 255),
        );

    /// The system cyan (UIKit's `systemCyan`).
    pub const SYSTEM_CYAN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(50, 173, 230),
            Color::rgb(100, 210, 255),
            Color::rgb(0, 113, 164),
            Color::rgb(112, 215, 255),
        );

    /// The system yellow (UIKit's `systemYellow`).
    pub const SYSTEM_YELLOW: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 204, 0),
            Color::rgb(255, 214, 10),
            Color::rgb(160, 90, 0),
            Color::rgb(255, 212, 38),
        );

    /// The base gray (UIKit's `systemGray`).
    pub const SYSTEM_GREY: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(142, 142, 147),
            Color::rgb(142, 142, 147),
            Color::rgb(108, 108, 112),
            Color::rgb(174, 174, 178),
        );

    /// The second-level shade of gray (UIKit's `systemGray2`).
    pub const SYSTEM_GREY2: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(174, 174, 178),
            Color::rgb(99, 99, 102),
            Color::rgb(142, 142, 147),
            Color::rgb(124, 124, 128),
        );

    /// The third-level shade of gray (UIKit's `systemGray3`).
    pub const SYSTEM_GREY3: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(199, 199, 204),
            Color::rgb(72, 72, 74),
            Color::rgb(174, 174, 178),
            Color::rgb(84, 84, 86),
        );

    /// The fourth-level shade of gray (UIKit's `systemGray4`).
    pub const SYSTEM_GREY4: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(209, 209, 214),
            Color::rgb(58, 58, 60),
            Color::rgb(188, 188, 192),
            Color::rgb(68, 68, 70),
        );

    /// The fifth-level shade of gray (UIKit's `systemGray5`).
    pub const SYSTEM_GREY5: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(229, 229, 234),
            Color::rgb(44, 44, 46),
            Color::rgb(216, 216, 220),
            Color::rgb(54, 54, 56),
        );

    /// The sixth-level shade of gray (UIKit's `systemGray6`).
    pub const SYSTEM_GREY6: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(242, 242, 247),
            Color::rgb(28, 28, 30),
            Color::rgb(235, 235, 240),
            Color::rgb(36, 36, 38),
        );

    /// Text labels of primary content (UIKit's `label`).
    pub const LABEL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(0, 0, 0),
        dark_color: Color::rgb(255, 255, 255),
        high_contrast_color: Color::rgb(0, 0, 0),
        dark_high_contrast_color: Color::rgb(255, 255, 255),
        elevated_color: Color::rgb(0, 0, 0),
        dark_elevated_color: Color::rgb(255, 255, 255),
        high_contrast_elevated_color: Color::rgb(0, 0, 0),
        dark_high_contrast_elevated_color: Color::rgb(255, 255, 255),
    };

    /// Text labels of secondary content (UIKit's `secondaryLabel`).
    pub const SECONDARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(60, 60, 67, 153),
        dark_color: Color::rgba(235, 235, 245, 153),
        high_contrast_color: Color::rgba(60, 60, 67, 173),
        dark_high_contrast_color: Color::rgba(235, 235, 245, 173),
        elevated_color: Color::rgba(60, 60, 67, 153),
        dark_elevated_color: Color::rgba(235, 235, 245, 153),
        high_contrast_elevated_color: Color::rgba(60, 60, 67, 173),
        dark_high_contrast_elevated_color: Color::rgba(235, 235, 245, 173),
    };

    /// Text labels of tertiary content (UIKit's `tertiaryLabel`).
    pub const TERTIARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(60, 60, 67, 76),
        dark_color: Color::rgba(235, 235, 245, 76),
        high_contrast_color: Color::rgba(60, 60, 67, 96),
        dark_high_contrast_color: Color::rgba(235, 235, 245, 96),
        elevated_color: Color::rgba(60, 60, 67, 76),
        dark_elevated_color: Color::rgba(235, 235, 245, 76),
        high_contrast_elevated_color: Color::rgba(60, 60, 67, 96),
        dark_high_contrast_elevated_color: Color::rgba(235, 235, 245, 96),
    };

    /// Text labels of quaternary content (UIKit's `quaternaryLabel`).
    pub const QUATERNARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(60, 60, 67, 45),
        dark_color: Color::rgba(235, 235, 245, 40),
        high_contrast_color: Color::rgba(60, 60, 67, 66),
        dark_high_contrast_color: Color::rgba(235, 235, 245, 61),
        elevated_color: Color::rgba(60, 60, 67, 45),
        dark_elevated_color: Color::rgba(235, 235, 245, 40),
        high_contrast_elevated_color: Color::rgba(60, 60, 67, 66),
        dark_high_contrast_elevated_color: Color::rgba(235, 235, 245, 61),
    };

    /// An overlay fill for thin and small shapes (UIKit's `systemFill`).
    pub const SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(120, 120, 128, 51),
        dark_color: Color::rgba(120, 120, 128, 91),
        high_contrast_color: Color::rgba(120, 120, 128, 71),
        dark_high_contrast_color: Color::rgba(120, 120, 128, 112),
        elevated_color: Color::rgba(120, 120, 128, 51),
        dark_elevated_color: Color::rgba(120, 120, 128, 91),
        high_contrast_elevated_color: Color::rgba(120, 120, 128, 71),
        dark_high_contrast_elevated_color: Color::rgba(120, 120, 128, 112),
    };

    /// An overlay fill for medium-size shapes (UIKit's `secondarySystemFill`).
    pub const SECONDARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(120, 120, 128, 40),
        dark_color: Color::rgba(120, 120, 128, 81),
        high_contrast_color: Color::rgba(120, 120, 128, 61),
        dark_high_contrast_color: Color::rgba(120, 120, 128, 102),
        elevated_color: Color::rgba(120, 120, 128, 40),
        dark_elevated_color: Color::rgba(120, 120, 128, 81),
        high_contrast_elevated_color: Color::rgba(120, 120, 128, 61),
        dark_high_contrast_elevated_color: Color::rgba(120, 120, 128, 102),
    };

    /// An overlay fill for large shapes (UIKit's `tertiarySystemFill`).
    pub const TERTIARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(118, 118, 128, 30),
        dark_color: Color::rgba(118, 118, 128, 61),
        high_contrast_color: Color::rgba(118, 118, 128, 51),
        dark_high_contrast_color: Color::rgba(118, 118, 128, 81),
        elevated_color: Color::rgba(118, 118, 128, 30),
        dark_elevated_color: Color::rgba(118, 118, 128, 61),
        high_contrast_elevated_color: Color::rgba(118, 118, 128, 51),
        dark_high_contrast_elevated_color: Color::rgba(118, 118, 128, 81),
    };

    /// An overlay fill for large areas with complex content (UIKit's `quaternarySystemFill`),
    /// and `CupertinoButton`'s default disabled colour for its plain (no-background) style.
    pub const QUATERNARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(116, 116, 128, 20),
        dark_color: Color::rgba(118, 118, 128, 45),
        high_contrast_color: Color::rgba(116, 116, 128, 40),
        dark_high_contrast_color: Color::rgba(118, 118, 128, 66),
        elevated_color: Color::rgba(116, 116, 128, 20),
        dark_elevated_color: Color::rgba(118, 118, 128, 45),
        high_contrast_elevated_color: Color::rgba(116, 116, 128, 40),
        dark_high_contrast_elevated_color: Color::rgba(118, 118, 128, 66),
    };

    /// Placeholder text in controls and text views (UIKit's `placeholderText`).
    pub const PLACEHOLDER_TEXT: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(60, 60, 67, 76),
        dark_color: Color::rgba(235, 235, 245, 76),
        high_contrast_color: Color::rgba(60, 60, 67, 96),
        dark_high_contrast_color: Color::rgba(235, 235, 245, 96),
        elevated_color: Color::rgba(60, 60, 67, 76),
        dark_elevated_color: Color::rgba(235, 235, 245, 76),
        high_contrast_elevated_color: Color::rgba(60, 60, 67, 96),
        dark_high_contrast_elevated_color: Color::rgba(235, 235, 245, 96),
    };

    /// The main background of an interface (UIKit's `systemBackground`).
    pub const SYSTEM_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(255, 255, 255),
        dark_color: Color::rgb(0, 0, 0),
        high_contrast_color: Color::rgb(255, 255, 255),
        dark_high_contrast_color: Color::rgb(0, 0, 0),
        elevated_color: Color::rgb(255, 255, 255),
        dark_elevated_color: Color::rgb(28, 28, 30),
        high_contrast_elevated_color: Color::rgb(255, 255, 255),
        dark_high_contrast_elevated_color: Color::rgb(36, 36, 38),
    };

    /// Content layered on the main background (UIKit's `secondarySystemBackground`).
    pub const SECONDARY_SYSTEM_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(242, 242, 247),
        dark_color: Color::rgb(28, 28, 30),
        high_contrast_color: Color::rgb(235, 235, 240),
        dark_high_contrast_color: Color::rgb(36, 36, 38),
        elevated_color: Color::rgb(242, 242, 247),
        dark_elevated_color: Color::rgb(44, 44, 46),
        high_contrast_elevated_color: Color::rgb(235, 235, 240),
        dark_high_contrast_elevated_color: Color::rgb(54, 54, 56),
    };

    /// Content layered on secondary backgrounds (UIKit's `tertiarySystemBackground`).
    pub const TERTIARY_SYSTEM_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(255, 255, 255),
        dark_color: Color::rgb(44, 44, 46),
        high_contrast_color: Color::rgb(255, 255, 255),
        dark_high_contrast_color: Color::rgb(54, 54, 56),
        elevated_color: Color::rgb(255, 255, 255),
        dark_elevated_color: Color::rgb(58, 58, 60),
        high_contrast_elevated_color: Color::rgb(255, 255, 255),
        dark_high_contrast_elevated_color: Color::rgb(68, 68, 70),
    };

    /// The main background of a grouped interface, such as a table view (UIKit's
    /// `systemGroupedBackground`).
    pub const SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(242, 242, 247),
        dark_color: Color::rgb(0, 0, 0),
        high_contrast_color: Color::rgb(235, 235, 240),
        dark_high_contrast_color: Color::rgb(0, 0, 0),
        elevated_color: Color::rgb(242, 242, 247),
        dark_elevated_color: Color::rgb(28, 28, 30),
        high_contrast_elevated_color: Color::rgb(235, 235, 240),
        dark_high_contrast_elevated_color: Color::rgb(36, 36, 38),
    };

    /// Content layered on the main background of a grouped interface (UIKit's
    /// `secondarySystemGroupedBackground`).
    pub const SECONDARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(255, 255, 255),
        dark_color: Color::rgb(28, 28, 30),
        high_contrast_color: Color::rgb(255, 255, 255),
        dark_high_contrast_color: Color::rgb(36, 36, 38),
        elevated_color: Color::rgb(255, 255, 255),
        dark_elevated_color: Color::rgb(44, 44, 46),
        high_contrast_elevated_color: Color::rgb(255, 255, 255),
        dark_high_contrast_elevated_color: Color::rgb(54, 54, 56),
    };

    /// Content layered on secondary backgrounds of a grouped interface (UIKit's
    /// `tertiarySystemGroupedBackground`).
    pub const TERTIARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(242, 242, 247),
        dark_color: Color::rgb(44, 44, 46),
        high_contrast_color: Color::rgb(235, 235, 240),
        dark_high_contrast_color: Color::rgb(54, 54, 56),
        elevated_color: Color::rgb(242, 242, 247),
        dark_elevated_color: Color::rgb(58, 58, 60),
        high_contrast_elevated_color: Color::rgb(235, 235, 240),
        dark_high_contrast_elevated_color: Color::rgb(68, 68, 70),
    };

    /// Thin separator lines that let the content under them show through (UIKit's `separator`).
    pub const SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgba(60, 60, 67, 73),
        dark_color: Color::rgba(84, 84, 88, 153),
        high_contrast_color: Color::rgba(60, 60, 67, 94),
        dark_high_contrast_color: Color::rgba(84, 84, 88, 173),
        elevated_color: Color::rgba(60, 60, 67, 73),
        dark_elevated_color: Color::rgba(210, 210, 210, 153),
        high_contrast_elevated_color: Color::rgba(60, 60, 67, 94),
        dark_high_contrast_elevated_color: Color::rgba(84, 84, 88, 173),
    };

    /// Separator lines that hide the content under them (UIKit's `opaqueSeparator`).
    pub const OPAQUE_SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(198, 198, 200),
        dark_color: Color::rgb(56, 56, 58),
        high_contrast_color: Color::rgb(198, 198, 200),
        dark_high_contrast_color: Color::rgb(56, 56, 58),
        elevated_color: Color::rgb(198, 198, 200),
        dark_elevated_color: Color::rgb(56, 56, 58),
        high_contrast_elevated_color: Color::rgb(198, 198, 200),
        dark_high_contrast_elevated_color: Color::rgb(56, 56, 58),
    };

    /// Links (UIKit's `link`).
    pub const LINK: CupertinoDynamicColor = CupertinoDynamicColor {
        color: Color::rgb(0, 122, 255),
        dark_color: Color::rgb(9, 132, 255),
        high_contrast_color: Color::rgb(0, 122, 255),
        dark_high_contrast_color: Color::rgb(9, 132, 255),
        elevated_color: Color::rgb(0, 122, 255),
        dark_elevated_color: Color::rgb(9, 132, 255),
        high_contrast_elevated_color: Color::rgb(0, 122, 255),
        dark_high_contrast_elevated_color: Color::rgb(9, 132, 255),
    };
}
