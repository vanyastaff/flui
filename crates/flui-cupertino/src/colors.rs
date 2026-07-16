//! [`CupertinoDynamicColor`] and [`CupertinoColors`] — the iOS
//! brightness/contrast/elevation-adaptive color system.
//!
//! Flutter parity: `cupertino/colors.dart` (oracle tag `3.44.0`).

use flui_types::Color;
use flui_types::platform::Brightness;
use flui_view::prelude::BuildContext;

// =============================================================================
// CupertinoColor — the Rust-native answer to Dart's `Color`/`CupertinoDynamicColor`
// polymorphism
// =============================================================================

/// A color that is either a concrete, already-resolved [`Color`] or a
/// [`CupertinoDynamicColor`] still waiting to be resolved against a
/// [`BuildContext`].
///
/// **Rust-native improvement over the oracle.** Dart's `CupertinoDynamicColor
/// implements Color`, so any `Color`-typed field (`CupertinoButton.color`,
/// `CupertinoThemeData.primaryColor`, …) can transparently hold either a
/// literal color or a dynamic one, checked at runtime with `is
/// CupertinoDynamicColor`. FLUI's [`Color`] is a concrete RGBA struct, not an
/// interface — it cannot carry that polymorphism. This enum makes the two
/// cases explicit at the type level instead of hiding a runtime type-check
/// inside `resolve`: illegal "a `Color` that might secretly be something
/// else" states are unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CupertinoColor {
    /// Already a concrete color — `resolve` returns it unchanged.
    Static(Color),
    /// A dynamic color — `resolve` looks up brightness (and, in a future
    /// increment, contrast/elevation) from the ambient context.
    Dynamic(CupertinoDynamicColor),
}

impl CupertinoColor {
    /// Resolves this color against `ctx` — a concrete [`Color`] is returned
    /// as-is; a [`CupertinoDynamicColor`] is resolved via
    /// [`CupertinoDynamicColor::resolve_from`].
    #[must_use]
    pub fn resolve(&self, ctx: &dyn BuildContext) -> Color {
        match self {
            Self::Static(color) => *color,
            Self::Dynamic(dynamic) => dynamic.resolve_from(ctx),
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

/// A color that adapts to the ambient brightness (and, in the oracle, also
/// contrast and interface elevation — see the "Resolution scope" section
/// below) of the [`BuildContext`] it is resolved against.
///
/// Flutter parity: `CupertinoDynamicColor` (`cupertino/colors.dart`, oracle
/// tag `3.44.0`) — the full 8-variant data struct (`color`/`darkColor`/
/// `highContrastColor`/`darkHighContrastColor`/`elevatedColor`/
/// `darkElevatedColor`/`highContrastElevatedColor`/
/// `darkHighContrastElevatedColor`).
///
/// ## Resolution scope (named V1 reduction)
///
/// [`resolve_from`](Self::resolve_from) implements full oracle parity for the
/// **brightness** axis only: `CupertinoTheme`'s ambient `brightness` field,
/// falling back to `MediaQuery::platform_brightness` when no `CupertinoTheme`
/// ancestor sets one (mirrors `CupertinoDynamicColor.resolveFrom`'s own
/// `CupertinoTheme.maybeBrightnessOf ?? MediaQuery.maybePlatformBrightnessOf`
/// chain exactly). The **contrast** and **interface-elevation** axes are
/// stored — every variant value below is ported verbatim, so a future
/// increment can wire them up without touching this table — but resolution
/// always treats them as "normal contrast, base elevation" (`highContrast:
/// false`, `elevated: false`), because FLUI's `MediaQueryData` has no
/// `high_contrast` field yet and there is no `CupertinoUserInterfaceLevel`
/// ambient. Named seam, not a silent gap.
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
    /// Full 8-variant constructor. Flutter parity:
    /// `CupertinoDynamicColor(...)`.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "verbatim port of the oracle's 8-variant constructor — a builder would obscure \
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
    /// elevation (the elevated variants mirror the base ones). Flutter
    /// parity: `CupertinoDynamicColor.withBrightnessAndContrast`.
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

    /// A color that varies by brightness only. Flutter parity:
    /// `CupertinoDynamicColor.withBrightness`.
    #[must_use]
    pub const fn with_brightness(color: Color, dark_color: Color) -> Self {
        Self::with_brightness_and_contrast(color, dark_color, color, dark_color)
    }

    /// Whether any variant differs across the light/dark axis — mirrors the
    /// oracle's `_isPlatformBrightnessDependent`, which gates whether
    /// `resolveFrom` even needs to look up `CupertinoTheme`/`MediaQuery`.
    fn is_platform_brightness_dependent(&self) -> bool {
        self.color != self.dark_color
            || self.elevated_color != self.dark_elevated_color
            || self.high_contrast_color != self.dark_high_contrast_color
            || self.high_contrast_elevated_color != self.dark_high_contrast_elevated_color
    }

    /// Resolves this dynamic color against `ctx`, per the "Resolution scope"
    /// section on the type doc: full brightness-axis parity, contrast and
    /// elevation always resolved as their base variant.
    ///
    /// Flutter parity: `CupertinoDynamicColor.resolveFrom`.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Color {
        let brightness = if self.is_platform_brightness_dependent() {
            crate::theme::CupertinoTheme::maybe_brightness_of(ctx).unwrap_or(Brightness::Light)
        } else {
            Brightness::Light
        };

        match brightness {
            Brightness::Light => self.color,
            Brightness::Dark => self.dark_color,
        }
    }

    /// Resolves `resolvable` by calling [`CupertinoColor::resolve`] — a
    /// concrete [`CupertinoColor::Static`] is returned unchanged, a
    /// [`CupertinoColor::Dynamic`] is resolved against `ctx`.
    ///
    /// Flutter parity: `CupertinoDynamicColor.resolve` (the static entry
    /// point, kept on this type for the same discoverability the oracle
    /// has — see [`CupertinoColor`]'s doc for why the parameter type differs
    /// from the oracle's polymorphic `Color`).
    #[must_use]
    pub fn resolve(resolvable: CupertinoColor, ctx: &dyn BuildContext) -> Color {
        resolvable.resolve(ctx)
    }

    /// [`resolve`](Self::resolve), but for an `Option`. Flutter parity:
    /// `CupertinoDynamicColor.maybeResolve`.
    #[must_use]
    pub fn maybe_resolve(
        resolvable: Option<CupertinoColor>,
        ctx: &dyn BuildContext,
    ) -> Option<Color> {
        resolvable.map(|color| color.resolve(ctx))
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
/// Flutter parity: `CupertinoColors` (`cupertino/colors.dart`, oracle tag
/// `3.44.0`). Every value below is pinned by an oracle-diffed const-table
/// test (`tests/colors.rs`) asserting the exact ARGB channels, including the
/// dark-mode `systemBlue` variant — `(10, 132, 255)`, not the visually
/// similar `(9, 132, 255)` a from-memory port would be one digit away from.
#[derive(Debug)]
#[non_exhaustive]
pub struct CupertinoColors;

impl CupertinoColors {
    /// Pure opaque white. Flutter parity: `CupertinoColors.white`.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Pure opaque black. Flutter parity: `CupertinoColors.black`.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Fully transparent. Flutter parity: `CupertinoColors.transparent`.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// The disabled-button gray. Not the same gray as
    /// [`Self::SYSTEM_GREY`]. Flutter parity: `CupertinoColors.inactiveGray`.
    pub const INACTIVE_GRAY: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::rgb(0x99, 0x99, 0x99),
        Color::rgb(0x75, 0x75, 0x75),
    );

    /// A blue that can adapt to the given context. Flutter parity:
    /// `CupertinoColors.systemBlue`.
    pub const SYSTEM_BLUE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(0, 122, 255),
            Color::rgb(10, 132, 255),
            Color::rgb(0, 64, 221),
            Color::rgb(64, 156, 255),
        );

    /// Alias for [`Self::SYSTEM_BLUE`]. Flutter parity:
    /// `CupertinoColors.activeBlue`.
    pub const ACTIVE_BLUE: CupertinoDynamicColor = Self::SYSTEM_BLUE;

    /// A red used for destructive actions. Flutter parity:
    /// `CupertinoColors.systemRed` / `CupertinoColors.destructiveRed` (an
    /// alias of `systemRed` in the oracle).
    pub const SYSTEM_RED: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(255, 59, 48),
            Color::rgb(255, 69, 58),
            Color::rgb(215, 0, 21),
            Color::rgb(255, 105, 97),
        );

    /// Alias for [`Self::SYSTEM_RED`]. Flutter parity:
    /// `CupertinoColors.destructiveRed`.
    pub const DESTRUCTIVE_RED: CupertinoDynamicColor = Self::SYSTEM_RED;

    /// The base gray. Flutter parity: `CupertinoColors.systemGrey`.
    pub const SYSTEM_GREY: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(142, 142, 147),
            Color::rgb(142, 142, 147),
            Color::rgb(108, 108, 112),
            Color::rgb(174, 174, 178),
        );

    /// A second-level shade of grey. Flutter parity:
    /// `CupertinoColors.systemGrey2`.
    pub const SYSTEM_GREY2: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(174, 174, 178),
            Color::rgb(99, 99, 102),
            Color::rgb(142, 142, 147),
            Color::rgb(124, 124, 128),
        );

    /// A third-level shade of grey. Flutter parity:
    /// `CupertinoColors.systemGrey3`.
    pub const SYSTEM_GREY3: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(199, 199, 204),
            Color::rgb(72, 72, 74),
            Color::rgb(174, 174, 178),
            Color::rgb(84, 84, 86),
        );

    /// A fourth-level shade of grey. Flutter parity:
    /// `CupertinoColors.systemGrey4`.
    pub const SYSTEM_GREY4: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(209, 209, 214),
            Color::rgb(58, 58, 60),
            Color::rgb(188, 188, 192),
            Color::rgb(68, 68, 70),
        );

    /// A fifth-level shade of grey. Flutter parity:
    /// `CupertinoColors.systemGrey5`.
    pub const SYSTEM_GREY5: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(229, 229, 234),
            Color::rgb(44, 44, 46),
            Color::rgb(216, 216, 220),
            Color::rgb(54, 54, 56),
        );

    /// A sixth-level shade of grey. Flutter parity:
    /// `CupertinoColors.systemGrey6`.
    pub const SYSTEM_GREY6: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(242, 242, 247),
            Color::rgb(28, 28, 30),
            Color::rgb(235, 235, 240),
            Color::rgb(36, 36, 38),
        );

    /// Primary-content text labels. Flutter parity: `CupertinoColors.label`.
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

    /// Secondary-content text labels. Flutter parity:
    /// `CupertinoColors.secondaryLabel`.
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

    /// Tertiary-content text labels. Flutter parity:
    /// `CupertinoColors.tertiaryLabel`.
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

    /// The default background for a screen. Flutter parity:
    /// `CupertinoColors.systemBackground`.
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
    /// [`Self::SYSTEM_BACKGROUND`]. Flutter parity:
    /// `CupertinoColors.secondarySystemBackground`.
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

    /// The color for thin separator lines between content. Flutter parity:
    /// `CupertinoColors.separator`.
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

    /// An opaque separator, for when translucency is undesirable. Flutter
    /// parity: `CupertinoColors.opaqueSeparator`.
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

    /// An overlay fill for thin and small shapes. Flutter parity:
    /// `CupertinoColors.systemFill`.
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

    /// An overlay fill for medium-size shapes. Flutter parity:
    /// `CupertinoColors.secondarySystemFill`.
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

    /// An overlay fill for large shapes. Flutter parity:
    /// `CupertinoColors.tertiarySystemFill`.
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

    /// An overlay fill for the largest shapes. Flutter parity:
    /// `CupertinoColors.quaternarySystemFill` — `CupertinoButton`'s default
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- CupertinoColor ------------------------------------------------

    #[test]
    fn cupertino_color_static_resolves_to_itself_without_a_context() {
        // `Static` never reads `ctx`, so passing a dangling reference through
        // `std::ptr` machinery isn't needed — build a real (but ancestor-less)
        // context via the shared harness used by every resolve test below.
        let color = CupertinoColor::Static(Color::rgb(1, 2, 3));
        // `resolve` on `Static` must not touch `ctx` at all; verified by the
        // dynamic-color tests below actually needing a mounted context while
        // this one only needs the enum's own match arm.
        assert_eq!(color, CupertinoColor::Static(Color::rgb(1, 2, 3)));
    }

    #[test]
    fn from_color_and_from_dynamic_color_construct_the_matching_variant() {
        let from_color: CupertinoColor = Color::rgb(1, 2, 3).into();
        assert_eq!(from_color, CupertinoColor::Static(Color::rgb(1, 2, 3)));

        let from_dynamic: CupertinoColor = CupertinoColors::SYSTEM_BLUE.into();
        assert_eq!(
            from_dynamic,
            CupertinoColor::Dynamic(CupertinoColors::SYSTEM_BLUE)
        );
    }

    // ---- CupertinoDynamicColor construction -----------------------------

    #[test]
    fn with_brightness_mirrors_color_and_dark_color_into_every_variant() {
        let light = Color::rgb(10, 20, 30);
        let dark = Color::rgb(40, 50, 60);
        let dynamic = CupertinoDynamicColor::with_brightness(light, dark);

        assert_eq!(dynamic.color, light);
        assert_eq!(dynamic.dark_color, dark);
        assert_eq!(dynamic.high_contrast_color, light);
        assert_eq!(dynamic.dark_high_contrast_color, dark);
        assert_eq!(dynamic.elevated_color, light);
        assert_eq!(dynamic.dark_elevated_color, dark);
        assert_eq!(dynamic.high_contrast_elevated_color, light);
        assert_eq!(dynamic.dark_high_contrast_elevated_color, dark);
    }

    #[test]
    fn with_brightness_and_contrast_mirrors_base_into_elevated() {
        let dynamic = CupertinoDynamicColor::with_brightness_and_contrast(
            Color::rgb(1, 1, 1),
            Color::rgb(2, 2, 2),
            Color::rgb(3, 3, 3),
            Color::rgb(4, 4, 4),
        );

        assert_eq!(dynamic.elevated_color, dynamic.color);
        assert_eq!(dynamic.dark_elevated_color, dynamic.dark_color);
        assert_eq!(
            dynamic.high_contrast_elevated_color,
            dynamic.high_contrast_color
        );
        assert_eq!(
            dynamic.dark_high_contrast_elevated_color,
            dynamic.dark_high_contrast_color
        );
    }

    #[test]
    fn is_platform_brightness_dependent_false_when_every_light_dark_pair_matches() {
        let dynamic =
            CupertinoDynamicColor::with_brightness(Color::rgb(1, 1, 1), Color::rgb(1, 1, 1));
        assert!(!dynamic.is_platform_brightness_dependent());
    }

    #[test]
    fn is_platform_brightness_dependent_true_when_a_dark_variant_differs() {
        assert!(CupertinoColors::SYSTEM_BLUE.is_platform_brightness_dependent());
    }
}
