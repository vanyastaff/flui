//! [`ThemeData`] — the value [`crate::Theme`] publishes to a subtree.
//!
//! Flutter parity: `material/theme_data.dart` `ThemeData` (oracle tag
//! `3.44.0`).

use flui_types::EdgeInsets;
use flui_types::Pixels;
use flui_types::platform::Brightness;
use flui_types::styling::{BorderRadius, BorderSide, Color};
use flui_types::typography::TextStyle;
use flui_widgets::WidgetStateProperty;

use crate::button_style::ButtonStyle;
use crate::color_scheme::ColorScheme;
use crate::shape::MaterialShape;
use crate::text_theme::TextTheme;
use crate::typography;

/// A [`Color`]-valued [`WidgetStateProperty`] that may itself resolve to
/// "no override" for a given state set — the shape every narrowed
/// `*ThemeData` color/side slot in this module uses (`fill_color`,
/// `check_color`, `overlay_color`, and friends).
type StateColor = WidgetStateProperty<Option<Color>>;

/// Compute the M3 default [`TextTheme`]: `englishLike2021` geometry overlaid
/// with a color-only theme uniformly recolored to `on_surface`.
///
/// Flutter parity, in two parts:
///
/// - The **merge direction** — geometry as the base, a color-only theme as
///   the patch — mirrors `Theme`'s build-time localization step:
///   `ThemeData.localize(baseTheme, localTextGeometry)` sets
///   `textTheme: localTextGeometry.merge(baseTheme.textTheme)`
///   (`theme_data.dart`, oracle tag `3.44.0`), i.e. `geometry.merge(color)`.
/// - The **uniform recolor** mirrors `Typography.material2021`'s
///   `base.black.apply(displayColor: dark, bodyColor: dark, ...)` /
///   `base.white.apply(displayColor: light, bodyColor: light, ...)`
///   (`typography.dart`, oracle tag `3.44.0`), where the oracle's `dark` and
///   `light` locals both reduce to `colorScheme.onSurface` regardless of
///   brightness — so every role ends up the same color, `on_surface`, not
///   the `black54`/`black87`/`black` (or `white70`/`white`) tiers
///   [`TextTheme::black_mountain_view`]/[`TextTheme::white_mountain_view`]
///   themselves carry.
///
/// **Documented divergence**: the oracle recomputes this lazily, per
/// `Theme.of` read, keyed on the ambient locale's `ScriptCategory` (English
/// vs. dense/tall scripts — ADR: see [`crate::typography`] module docs on why
/// dense/tall are deferred). FLUI V1 has no script-category-resolving
/// localization consumer yet, so this bakes the `englishLike`-only default
/// once, here, at [`ThemeData::light`]/[`ThemeData::dark`] construction time
/// instead of on every `Theme::of` read.
fn default_text_theme(brightness: Brightness, on_surface: Color) -> TextTheme {
    let geometry = typography::english_like_2021();
    let color_theme = match brightness {
        Brightness::Dark => TextTheme::white_mountain_view(),
        Brightness::Light => TextTheme::black_mountain_view(),
    };
    geometry.merge(&color_theme.apply_color(on_surface))
}

/// Overrides [`ElevatedButton`](crate::ElevatedButton)'s default
/// [`ButtonStyle`], resolved between the widget's own `style` and
/// `ElevatedButton::default_style` — see
/// `crate::button_style_button`'s resolve-then-coalesce docs for how this
/// slot's `style` participates in the three-tier cascade.
///
/// Flutter parity: `ElevatedButtonThemeData` (`material/elevated_button_theme.dart`,
/// oracle tag `3.44.0`), which carries the identical single `style` field.
/// **Named reduction**: the oracle also has a standalone `ElevatedButtonTheme`
/// `InheritedTheme` widget (so a subtree can override the style without
/// touching the whole [`ThemeData`]); FLUI V1 has no per-widget
/// `InheritedTheme` wrappers yet, so `ElevatedButton` reads only this
/// [`ThemeData`] slot via [`Theme::of`](crate::Theme::of).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElevatedButtonThemeData {
    /// Overrides for [`ElevatedButton`](crate::ElevatedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`FilledButton`](crate::FilledButton)'s default [`ButtonStyle`]
/// (both the plain and `tonal` variants share this one slot, matching the
/// oracle). Flutter parity: `FilledButtonThemeData`
/// (`material/filled_button_theme.dart`, oracle tag `3.44.0`) — same named
/// reduction as [`ElevatedButtonThemeData`] (no standalone `InheritedTheme`
/// wrapper yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilledButtonThemeData {
    /// Overrides for [`FilledButton`](crate::FilledButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`OutlinedButton`](crate::OutlinedButton)'s default
/// [`ButtonStyle`]. Flutter parity: `OutlinedButtonThemeData`
/// (`material/outlined_button_theme.dart`, oracle tag `3.44.0`) — same named
/// reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutlinedButtonThemeData {
    /// Overrides for [`OutlinedButton`](crate::OutlinedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`TextButton`](crate::TextButton)'s default [`ButtonStyle`].
/// Flutter parity: `TextButtonThemeData` (`material/text_button_theme.dart`,
/// oracle tag `3.44.0`) — same named reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextButtonThemeData {
    /// Overrides for [`TextButton`](crate::TextButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`IconButton`](crate::IconButton)'s default [`ButtonStyle`].
/// Flutter parity: `IconButtonThemeData` (`material/icon_button_theme.dart`,
/// oracle tag `3.44.0`) — same named reduction as [`ElevatedButtonThemeData`].
/// Not to be confused with [`flui_widgets::IconThemeData`], which colors an
/// `Icon` child, not an `IconButton`'s own resolved `ButtonStyle`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconButtonThemeData {
    /// Overrides for [`IconButton`](crate::IconButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`AppBar`](crate::AppBar)'s M3 token defaults, one field at a
/// time — an unset field here still falls through to `AppBar`'s own default
/// (see that type's `resolve_style`), it does not blank the whole slot.
///
/// Flutter parity: `AppBarThemeData` (`material/app_bar_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `AppBar` actually consumes:
/// [`background_color`](Self::background_color),
/// [`foreground_color`](Self::foreground_color),
/// [`elevation`](Self::elevation), [`title_text_style`](Self::title_text_style).
/// Named deferrals (no consumer in FLUI's `AppBar` yet, so a field here would
/// have nothing to reach): `scrolled_under_elevation`, `shadow_color`,
/// `surface_tint_color`, `shape`, `icon_theme`/`actions_icon_theme`,
/// `center_title`, `title_spacing`, `leading_width`, `toolbar_height`,
/// `toolbar_text_style`, `system_overlay_style`, `actions_padding`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppBarThemeData {
    /// Overrides [`AppBar`](crate::AppBar)'s default background color
    /// (`ColorScheme.surface`).
    pub background_color: Option<Color>,
    /// Overrides [`AppBar`](crate::AppBar)'s default icon/title color
    /// (`ColorScheme.on_surface`).
    pub foreground_color: Option<Color>,
    /// Overrides [`AppBar`](crate::AppBar)'s default elevation (`0.0`).
    pub elevation: Option<f32>,
    /// Overrides the title's text style verbatim — Flutter parity:
    /// `widget.titleTextStyle ?? appBarTheme.titleTextStyle ??
    /// defaults.titleTextStyle?.copyWith(color: foregroundColor)`
    /// (`app_bar.dart`, oracle tag `3.44.0`): unlike the default tier, a
    /// theme-supplied style is used as-is, not recolored to the resolved
    /// [`foreground_color`](Self::foreground_color).
    pub title_text_style: Option<TextStyle>,
}

/// Overrides [`Card`](crate::Card)'s `_CardDefaultsM3` token defaults, one
/// field at a time.
///
/// Flutter parity: `CardThemeData` (`material/card_theme.dart`, oracle tag
/// `3.44.0`), narrowed to the fields FLUI's `Card` actually consumes:
/// [`color`](Self::color), [`elevation`](Self::elevation),
/// [`shape`](Self::shape), [`margin`](Self::margin). Named deferrals (no
/// consumer in FLUI's `Card` yet): `shadow_color`, `surface_tint_color`,
/// `clip_behavior`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardThemeData {
    /// Overrides [`Card`](crate::Card)'s default fill color
    /// (`ColorScheme.surfaceContainerLow`).
    pub color: Option<Color>,
    /// Overrides [`Card`](crate::Card)'s default elevation (`1.0`).
    pub elevation: Option<f32>,
    /// Overrides [`Card`](crate::Card)'s default shape (a 12dp rounded
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides [`Card`](crate::Card)'s default outer margin
    /// (`EdgeInsets.all(4.0)`).
    pub margin: Option<EdgeInsets>,
}

/// Overrides [`Dialog`](crate::Dialog)/[`AlertDialog`](crate::AlertDialog)'s
/// `_DialogDefaultsM3` token defaults, one field at a time.
///
/// Flutter parity: `DialogThemeData` (`material/dialog_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `Dialog`/`AlertDialog`
/// actually consume: [`background_color`](Self::background_color) (`Dialog`),
/// [`elevation`](Self::elevation) (`Dialog`), [`shape`](Self::shape)
/// (`Dialog`), [`title_text_style`](Self::title_text_style) (`AlertDialog`'s
/// title), [`content_text_style`](Self::content_text_style) (`AlertDialog`'s
/// content). Named deferrals (no consumer yet): `shadow_color`,
/// `surface_tint_color`, `alignment`, `icon_color`, `actions_padding`,
/// `barrier_color`, `inset_padding`, `clip_behavior`, `constraints` (the
/// oracle's own `Dialog.build` reads `constraints ?? dialogTheme.constraints
/// ?? const BoxConstraints(minWidth: 280.0)` — FLUI's `Dialog` has a widget
/// tier for this via [`Dialog::constraints`](crate::Dialog::constraints) but
/// no theme tier yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogThemeData {
    /// Overrides [`Dialog`](crate::Dialog)'s default background color
    /// (`ColorScheme.surfaceContainerHigh`).
    pub background_color: Option<Color>,
    /// Overrides [`Dialog`](crate::Dialog)'s default elevation (`6.0`).
    pub elevation: Option<f32>,
    /// Overrides [`Dialog`](crate::Dialog)'s default shape (a 28dp rounded
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides [`AlertDialog`](crate::AlertDialog)'s title text style
    /// (default: `TextTheme.headlineSmall`).
    pub title_text_style: Option<TextStyle>,
    /// Overrides [`AlertDialog`](crate::AlertDialog)'s content text style
    /// (default: `TextTheme.bodyMedium`).
    pub content_text_style: Option<TextStyle>,
}

/// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
/// `_FABDefaultsM3` token defaults, one field at a time.
///
/// Flutter parity: `FloatingActionButtonThemeData`
/// (`material/floating_action_button_theme.dart`, oracle tag `3.44.0`),
/// narrowed to the fields FLUI's `FloatingActionButton` actually consumes:
/// [`background_color`](Self::background_color),
/// [`foreground_color`](Self::foreground_color),
/// [`elevation`](Self::elevation) (the enabled/disabled tier — see
/// `floating_action_button.rs`'s `resolve_elevation`). Named deferrals (no
/// override surface in FLUI's `FloatingActionButton` yet, matching that
/// module's own deferred list): `focus_color`/`hover_color`/`splash_color`,
/// `focus_elevation`/`hover_elevation`/`disabled_elevation`/
/// `highlight_elevation`, `shape`, `enable_feedback`, `icon_size`, every
/// `*_size_constraints` field, the `extended*` fields, `mouse_cursor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FabThemeData {
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// default background color (`ColorScheme.primaryContainer`).
    pub background_color: Option<Color>,
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// default foreground/icon color (`ColorScheme.onPrimaryContainer`).
    pub foreground_color: Option<Color>,
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// enabled-tier elevation (`6.0`) — the same value the `disabled` tier
    /// falls back to (see `resolve_elevation`'s doc comment); the
    /// `pressed`/`hovered`/`focused` tiers are unaffected, matching the
    /// oracle's own independent `highlightElevation`/`hoverElevation`/
    /// `focusElevation` fields (this crate exposes no override for those).
    pub elevation: Option<f32>,
}

/// Overrides [`InputDecorator`](crate::input_decorator::InputDecorator)'s
/// `_InputDecoratorDefaultsM3` token defaults, one field at a time — an unset
/// field here still falls through to the M3 default table for that field
/// (see `input_decorator.rs`'s `default_*` functions), it does not blank the
/// whole slot.
///
/// Flutter parity: `InputDecorationThemeData` (`material/input_decorator.dart`,
/// oracle tag `3.44.0`), narrowed to the fields FLUI's `InputDecorator`
/// actually consumes: [`fill_color`](Self::fill_color),
/// [`active_indicator`](Self::active_indicator), [`hint_style`](Self::hint_style),
/// [`label_style`](Self::label_style), [`helper_style`](Self::helper_style),
/// [`error_style`](Self::error_style), [`content_padding`](Self::content_padding).
/// Named deferrals (no consumer in FLUI's `InputDecorator` yet — see that
/// module's docs for the full named-divergence list): `outlineBorder` (no
/// `OutlineInputBorder` variant in V1), `iconColor`/`prefixIconColor`/
/// `suffixIconColor` (no icon slots), `floatingLabelStyle` (V1's snap float
/// reuses [`label_style`](Self::label_style) for both positions — the oracle
/// itself resolves both through an identical state table), `isDense`,
/// `isCollapsed`, `border`, `focusColor`/`hoverColor` (the container's hover
/// blend is a fixed `ThemeData.hoverColor`-shaped constant in V1, not yet a
/// themeable slot), and every constraint/alignment/behavior field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputDecorationThemeData {
    /// Overrides the filled container's fill color, per state. `None` falls
    /// through to the M3 default (disabled: `onSurface@4%`; otherwise
    /// `surfaceContainerHighest`) — see `input_decorator.rs`'s
    /// `default_fill_color`.
    pub fill_color: Option<WidgetStateProperty<Option<Color>>>,
    /// Overrides the bottom underline indicator's color/width, per state.
    /// `None` falls through to the M3 default state table — see
    /// `input_decorator.rs`'s `default_active_indicator`.
    pub active_indicator: Option<WidgetStateProperty<Option<BorderSide<Pixels>>>>,
    /// Overrides the hint text style, per state. `None` falls through to the
    /// M3 default (disabled/enabled `onSurfaceVariant`).
    pub hint_style: Option<WidgetStateProperty<Option<TextStyle>>>,
    /// Overrides the label text style (both the floating and inline
    /// positions — see the struct doc's `floatingLabelStyle` note), per
    /// state. `None` falls through to the M3 default state table.
    pub label_style: Option<WidgetStateProperty<Option<TextStyle>>>,
    /// Overrides the helper line's text style, per state. `None` falls
    /// through to the M3 default (disabled/enabled `onSurfaceVariant`).
    pub helper_style: Option<WidgetStateProperty<Option<TextStyle>>>,
    /// Overrides the error line's text style, per state. `None` falls
    /// through to the M3 default (`error`, unconditionally).
    pub error_style: Option<WidgetStateProperty<Option<TextStyle>>>,
    /// Overrides the container's content padding. `None` falls through to
    /// the M3 filled-non-outline default (`EdgeInsets.fromLTRB(12, 8, 12,
    /// 8)`, `input_decorator.dart`'s `contentPadding` doc, tag `3.44.0`).
    pub content_padding: Option<EdgeInsets>,
}

/// Overrides [`ListTile`](crate::ListTile)'s `_LisTileDefaultsM3` token
/// defaults, one field at a time — an unset field here still falls through to
/// `ListTile`'s own default (see `list_tile.rs`'s `resolve_*` functions), it
/// does not blank the whole slot.
///
/// Flutter parity: `ListTileThemeData` (`material/list_tile_theme.dart`,
/// oracle tag `3.44.0`), narrowed to the fields FLUI's `ListTile` actually
/// consumes. FLUI collapses the oracle's two theme tiers (the ambient `ListTileTheme`
/// inherited widget and this [`ThemeData`] slot) into this one slot, the same
/// "named reduction" every other component-theme type in this crate already
/// makes (see [`CardThemeData`]'s doc comment) — `list_tile.dart`'s own
/// `ListTile.build` reads both (`tileTheme.iconColor ??
/// theme.listTileTheme.iconColor`), so this slot stands in for both reads.
/// Named deferrals (no consumer in FLUI's `ListTile` yet):
/// `style` (`ListTileStyle` is an M2-only fork this M3-only crate has no use
/// for), `enable_feedback`, `mouse_cursor`, `visual_density`,
/// `title_alignment` (baseline-precise top/center/bottom placement — this
/// substrate's `ListTile` is a `Row`/`Column` composition, not a baseline-aware
/// `_RenderListTile` port, see that module's docs), `control_affinity` (no
/// checkbox/switch/radio list-tile variants yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListTileThemeData {
    /// Overrides [`ListTile`](crate::ListTile)'s dense-layout flag.
    pub dense: Option<bool>,
    /// Overrides [`ListTile`](crate::ListTile)'s default shape (a plain
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides the color used for icons/text when the tile is selected
    /// (`ColorScheme.primary`).
    pub selected_color: Option<Color>,
    /// Overrides the default `leading`/`trailing` icon color
    /// (`ColorScheme.onSurfaceVariant`).
    pub icon_color: Option<Color>,
    /// Overrides the default `title`/`subtitle`/`leading`/`trailing` text
    /// color. `None` (the default) leaves each slot's own baked-in default
    /// color untouched — see `list_tile.rs`'s `resolve_content_color`.
    pub text_color: Option<Color>,
    /// Overrides [`ListTile::title`](crate::ListTile::title)'s text style
    /// (`TextTheme.bodyLarge` colored `onSurface`).
    pub title_text_style: Option<TextStyle>,
    /// Overrides [`ListTile::subtitle`](crate::ListTile::subtitle)'s text
    /// style (`TextTheme.bodyMedium` colored `onSurfaceVariant`).
    pub subtitle_text_style: Option<TextStyle>,
    /// Overrides [`ListTile::leading`](crate::ListTile::leading)/
    /// [`trailing`](crate::ListTile::trailing)'s text style
    /// (`TextTheme.labelSmall` colored `onSurfaceVariant`).
    pub leading_and_trailing_text_style: Option<TextStyle>,
    /// Overrides the tile's internal content padding
    /// (`EdgeInsetsDirectional.only(start: 16.0, end: 24.0)`).
    pub content_padding: Option<EdgeInsets>,
    /// Overrides the tile's background color when
    /// [`ListTile::selected`](crate::ListTile::selected) is `false`
    /// (`Colors.transparent`).
    pub tile_color: Option<Color>,
    /// Overrides the tile's background color when
    /// [`ListTile::selected`](crate::ListTile::selected) is `true`
    /// (`Colors.transparent`).
    pub selected_tile_color: Option<Color>,
    /// Overrides the gap between the leading/trailing slots and the title
    /// column (`16.0` — a bare `list_tile.dart` literal, not part of
    /// `_LisTileDefaultsM3`).
    pub horizontal_title_gap: Option<f32>,
    /// Overrides the minimum padding above/below the title/subtitle column
    /// (`8.0`).
    pub min_vertical_padding: Option<f32>,
    /// Overrides the minimum width reserved for
    /// [`ListTile::leading`](crate::ListTile::leading) (`24.0`).
    pub min_leading_width: Option<f32>,
    /// Overrides the tile's minimum height. `None` (the default) falls
    /// through to the one/two/three-line table — see `list_tile.rs`'s
    /// `default_tile_height`.
    pub min_tile_height: Option<f32>,
    /// Overrides [`ListTile::is_three_line`](crate::ListTile::is_three_line)
    /// when the widget itself leaves it unset.
    pub is_three_line: Option<bool>,
}

/// Overrides [`Divider`](crate::Divider)/[`VerticalDivider`](crate::VerticalDivider)'s
/// `_DividerDefaultsM3` token defaults, one field at a time.
///
/// Flutter parity: `DividerThemeData` (`material/divider_theme.dart`, oracle
/// tag `3.44.0`) — every oracle field has a consumer here, so nothing is
/// narrowed. FLUI collapses the oracle's `DividerTheme` inherited widget into
/// this one [`ThemeData`] slot, the same named reduction
/// [`ListTileThemeData`] makes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DividerThemeData {
    /// Overrides the line color (`ColorScheme.outlineVariant`).
    pub color: Option<Color>,
    /// Overrides [`Divider`](crate::Divider)'s height /
    /// [`VerticalDivider`](crate::VerticalDivider)'s width (`16.0`).
    pub space: Option<f32>,
    /// Overrides the line's thickness (`1.0`).
    pub thickness: Option<f32>,
    /// Overrides the leading-edge indent (`0.0`).
    pub indent: Option<f32>,
    /// Overrides the trailing-edge indent (`0.0`).
    pub end_indent: Option<f32>,
    /// Overrides the line's corner radius. `None` (the default) paints a
    /// square-cornered line.
    pub radius: Option<BorderRadius>,
}

/// Overrides [`Checkbox`](crate::Checkbox)'s `_CheckboxDefaultsM3` token
/// defaults, one field at a time — an unset field here still falls through
/// to `Checkbox`'s own M3 default table (see `checkbox.rs`'s
/// `checkbox_default_*` functions), it does not blank the whole slot.
///
/// Flutter parity: `CheckboxThemeData` (`material/checkbox_theme.dart`,
/// oracle tag `3.44.0`), narrowed to the fields FLUI's `Checkbox` actually
/// consumes: [`fill_color`](Self::fill_color), [`check_color`](Self::check_color),
/// [`overlay_color`](Self::overlay_color), [`side`](Self::side). Named
/// deferrals (no consumer in FLUI's `Checkbox` yet — see that module's docs
/// for the full named-divergence list): `mouse_cursor`, `splash_radius`
/// (fixed at the M3 default), `material_tap_target_size`, `visual_density`,
/// `shape` (fixed at the M3 2dp-rounded default).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckboxThemeData {
    /// Overrides [`Checkbox`](crate::Checkbox)'s default fill color, per
    /// state. `None` for a given state falls through to the M3 default.
    pub fill_color: Option<StateColor>,
    /// Overrides [`Checkbox`](crate::Checkbox)'s default checkmark/dash
    /// color, per state.
    pub check_color: Option<StateColor>,
    /// Overrides [`Checkbox`](crate::Checkbox)'s default state-overlay
    /// color (the InkWell-shaped hover/focus/press ramp), per state.
    pub overlay_color: Option<StateColor>,
    /// Overrides [`Checkbox`](crate::Checkbox)'s default border side, per
    /// state.
    pub side: Option<WidgetStateProperty<Option<BorderSide<Pixels>>>>,
}

/// Overrides [`Chip`](crate::Chip)/[`FilterChip`](crate::FilterChip)'s
/// `_ChipDefaultsM3`/`_FilterChipDefaultsM3` token defaults, one field at a
/// time — an unset field here still falls through to the owning widget's own
/// M3 default table (see `chip.rs`'s `chip_default_*`/`chip_*_color_default`
/// functions), it does not blank the whole slot.
///
/// Flutter parity: `ChipThemeData` (`material/chip_theme.dart`, oracle tag
/// `3.44.0`), narrowed to the fields FLUI's [`Chip`](crate::Chip)/
/// [`FilterChip`](crate::FilterChip) actually consume:
/// [`label_color`](Self::label_color), [`icon_color`](Self::icon_color),
/// [`delete_icon_color`](Self::delete_icon_color),
/// [`checkmark_color`](Self::checkmark_color), [`side`](Self::side),
/// [`shape`](Self::shape), [`padding`](Self::padding),
/// [`label_padding`](Self::label_padding). Every field is a **plain**
/// override rather than a `StateColor`/`WidgetStateProperty` — see
/// `chip.rs`'s module docs ("`ChipThemeData`: plain overrides") for why this
/// diverges from [`CheckboxThemeData`]/[`SwitchThemeData`]/
/// [`RadioThemeData`]/[`NavigationBarThemeData`]'s per-state color slots:
/// `chip_theme.dart` genuinely types these fields as plain values too (only
/// `color`, the container fill, is a `WidgetStateProperty` in the oracle,
/// and that field is a named V1 deferral here — see the same module docs).
/// Named deferrals (no consumer in FLUI's `Chip`/`FilterChip` yet): `color`/
/// `background_color`/`disabled_color`/`selected_color`/
/// `secondary_selected_color` (the container fill — see `chip.rs`'s module
/// docs), `shadow_color`/`surface_tint_color`/`selected_shadow_color`
/// (`Material` has no such parameters yet, the same gap every other
/// `Material`-backed M3 component in this crate already has),
/// `show_checkmark` (V1 always shows one when selected), `brightness`,
/// `elevation`/`press_elevation` (V1 is flat-only, elevation fixed `0.0`),
/// `avatar_box_constraints`/`delete_icon_box_constraints`. `labelStyle`
/// (`TextStyle?`) and `secondaryLabelStyle` (`FilterChip`'s selected-state
/// label style in the oracle) are BOTH narrowed down to
/// [`label_color`](Self::label_color) alone: every meaningful per-state
/// difference in `_ChipDefaultsM3.labelStyle`/`_FilterChipDefaultsM3.labelStyle`
/// is color-only (`_textTheme.labelLarge?.copyWith(color: ...)`, both files)
/// — the font/size/weight geometry always comes from the ambient
/// `TextTheme.labelLarge` and is not independently themeable here, and
/// `secondaryLabelStyle` specifically (a full second `TextStyle` slot for
/// the selected state) has no separate consumer since the private
/// `chip_states`-driven color resolution (`chip.rs`) already covers
/// `Selected` within the one
/// [`label_color`](Self::label_color) field. `iconTheme`
/// (`IconThemeData?`) is narrowed the same way [`NavigationBarThemeData::icon_color`]
/// narrows its own `iconTheme` field — down to
/// [`icon_color`](Self::icon_color) alone — because [`Chip`](crate::Chip)/
/// [`FilterChip`](crate::FilterChip)'s avatar/checkmark/delete-icon size is
/// pinned at the M3 default (`CHIP_ICON_SIZE`, `18.0`, `chip.rs`) with no
/// override surface yet, so the `size`/`fill`/`weight`/`grade`/`optical_size`
/// axes `IconThemeData` also carries have nothing to reach. The delete
/// affordance's `_EnsureMinSemanticsSize`/`MaterialTapTargetSize`-driven
/// minimum tap target (`chip.dart`'s `_buildDeleteIcon`, padding the visible
/// 18dp glyph out to a `kMinInteractiveDimension`-or-`-8.0` accessible hit
/// area) has no analogue here either — `chip.rs`'s delete `InkWell` hit-tests
/// only its own visible icon bounds, a named accessibility-surface gap
/// alongside the module docs' other deferred delete-affordance items
/// (tooltip, custom icon override).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChipThemeData {
    /// Overrides the label's resolved text color, per the widget's own
    /// current state. `None` falls through to the M3 default table (see
    /// `chip.rs`'s `chip_content_color_default`).
    pub label_color: Option<Color>,
    /// Overrides the leading avatar/checkmark icon's color. `None` falls
    /// through to the M3 default table (see `chip.rs`'s
    /// `chip_icon_color_default`).
    pub icon_color: Option<Color>,
    /// Overrides the trailing delete icon's color. `None` falls through to
    /// the M3 default table (see `chip.rs`'s `chip_content_color_default`).
    pub delete_icon_color: Option<Color>,
    /// Overrides [`FilterChip`](crate::FilterChip)'s selected checkmark
    /// color. `None` falls through to the M3 default table (see `chip.rs`'s
    /// `chip_icon_color_default`). No effect on [`Chip`](crate::Chip),
    /// which never shows a checkmark.
    pub checkmark_color: Option<Color>,
    /// Overrides the container's border side. `None` falls through to the
    /// M3 default (see `chip.rs`'s `chip_default_side`).
    pub side: Option<BorderSide<Pixels>>,
    /// Overrides the container's shape. `None` falls through to the M3
    /// default (an 8dp rounded rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides the container's outer padding. `None` falls through to the
    /// M3 default (`EdgeInsets.all(8.0)`).
    pub padding: Option<EdgeInsets>,
    /// Overrides the label's inner padding. `None` falls through to the M3
    /// default (`EdgeInsets.symmetric(horizontal: 8.0)`).
    pub label_padding: Option<EdgeInsets>,
}

/// Overrides [`Switch`](crate::Switch)'s `_SwitchDefaultsM3` token defaults,
/// one field at a time — an unset field here still falls through to
/// `Switch`'s own M3 default table (see `switch.rs`'s `switch_default_*`
/// functions), it does not blank the whole slot.
///
/// Flutter parity: `SwitchThemeData` (`material/switch_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `Switch` actually consumes:
/// [`thumb_color`](Self::thumb_color), [`track_color`](Self::track_color),
/// [`track_outline_color`](Self::track_outline_color),
/// [`overlay_color`](Self::overlay_color). Named deferrals (no consumer in
/// FLUI's `Switch` yet — see that module's docs for the full
/// named-divergence list): `mouse_cursor`, `splash_radius`,
/// `material_tap_target_size`, `thumb_icon`, `padding`,
/// `track_outline_width` (all fixed at their M3 defaults).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SwitchThemeData {
    /// Overrides [`Switch`](crate::Switch)'s default thumb color, per state.
    pub thumb_color: Option<StateColor>,
    /// Overrides [`Switch`](crate::Switch)'s default track fill color, per
    /// state.
    pub track_color: Option<StateColor>,
    /// Overrides [`Switch`](crate::Switch)'s default track border color, per
    /// state.
    pub track_outline_color: Option<StateColor>,
    /// Overrides [`Switch`](crate::Switch)'s default state-overlay color
    /// (the InkWell-shaped hover/focus/press ramp), per state.
    pub overlay_color: Option<StateColor>,
}

/// Overrides [`NavigationBar`](crate::NavigationBar)'s `_NavigationBarDefaultsM3`
/// token defaults, one field at a time — an unset field here still falls
/// through to `NavigationBar`'s own M3 default table (see `navigation_bar.rs`'s
/// `navigation_bar_default_*`/`navigation_destination_default_*` functions),
/// it does not blank the whole slot.
///
/// Flutter parity: `NavigationBarThemeData` (`material/navigation_bar_theme.dart`,
/// oracle tag `3.44.0`), narrowed to the fields FLUI's `NavigationBar` actually
/// consumes: [`height`](Self::height), [`background_color`](Self::background_color),
/// [`elevation`](Self::elevation), [`indicator_color`](Self::indicator_color),
/// [`icon_color`](Self::icon_color), [`label_text_style`](Self::label_text_style),
/// [`overlay_color`](Self::overlay_color). Named deferrals (no consumer in
/// FLUI's `NavigationBar` yet — see that module's docs for the full
/// named-divergence list): `shadow_color`/`surface_tint_color` (`Material` has
/// no such parameters yet — the same gap every other `Material`-backed M3
/// component in this crate already has), `indicator_shape` (fixed at the M3
/// `StadiumBorder` default), `label_padding` (fixed at the M3 `EdgeInsets.only(top:
/// 4)` default), `label_behavior` (V1 always behaves as `alwaysShow` — see the
/// module docs' "Label behavior" section for why the other two variants are
/// deferred wholesale rather than as a half-wired enum). [`icon_color`](Self::icon_color)
/// is itself a further narrowing: the oracle's `iconTheme` is a full
/// `WidgetStateProperty<IconThemeData?>` (size, color, opacity, the `fill`/
/// `weight`/`grade`/`optical_size` font-variation axes, shadows); this slot
/// carries only the color, since `NavigationDestination`'s icon size is
/// pinned at the M3 default (`NAVIGATION_DESTINATION_ICON_SIZE`, `24.0`) with
/// no override surface yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationBarThemeData {
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default height
    /// (`80.0`).
    pub height: Option<f32>,
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default background
    /// color (`ColorScheme.surfaceContainer`).
    pub background_color: Option<Color>,
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default elevation
    /// (`3.0`).
    pub elevation: Option<f32>,
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default selection
    /// indicator color (`ColorScheme.secondaryContainer`).
    pub indicator_color: Option<Color>,
    /// Overrides each [`NavigationDestination`](crate::NavigationDestination)'s
    /// default icon color, per state (`Selected`/`Disabled` only — the M3
    /// default table does not vary by `Hovered`/`Focused`/`Pressed`).
    pub icon_color: Option<StateColor>,
    /// Overrides each [`NavigationDestination`](crate::NavigationDestination)'s
    /// default label text style, per state. `None` for a given state falls
    /// through to `TextTheme.labelMedium` recolored by the M3 default table.
    pub label_text_style: Option<WidgetStateProperty<Option<TextStyle>>>,
    /// Overrides each destination's state-overlay color (the `InkWell`-shaped
    /// hover/focus/press ramp). `None` (the default, at every tier) means no
    /// overlay layer at all — see [`crate::ink_well`]'s own "no hardcoded
    /// opacities" policy, which this component inherits verbatim (the oracle
    /// itself sets no default `overlayColor` in `_NavigationBarDefaultsM3`
    /// either).
    pub overlay_color: Option<StateColor>,
}

/// Overrides [`TabBar`](crate::TabBar)'s `_TabsSecondaryDefaultsM3` token
/// defaults, one field at a time — an unset field still falls through to
/// `TabBar`'s own M3 secondary default table (see `tabs.rs`'s
/// `resolve_style`), it does not blank the whole slot.
///
/// Flutter parity: `TabBarThemeData` (`material/tab_bar_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `TabBar` actually consumes:
/// [`indicator_color`](Self::indicator_color), [`label_color`](Self::label_color),
/// [`unselected_label_color`](Self::unselected_label_color),
/// [`label_style`](Self::label_style),
/// [`unselected_label_style`](Self::unselected_label_style),
/// [`divider_color`](Self::divider_color), [`divider_height`](Self::divider_height),
/// [`overlay_color`](Self::overlay_color). Named deferrals (no consumer in
/// FLUI's `TabBar` yet — see that module's docs for the full
/// named-divergence list): `indicator` (custom `Decoration`), `indicator_size`
/// (fixed at `TabBarIndicatorSize::Tab`, the only size the secondary-only V1
/// supports), `label_padding` (fixed at `kTabLabelPadding`), `splash_factory`,
/// `mouse_cursor`, `tab_alignment` (fixed at the fill/equal-share layout),
/// `text_scaler`, `indicator_animation` (no `AnimationController` on
/// `TabController` yet), `splash_border_radius`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabBarThemeData {
    /// Overrides [`TabBar`](crate::TabBar)'s default selected-tab indicator
    /// color (`ColorScheme.primary`).
    pub indicator_color: Option<Color>,
    /// Overrides [`TabBar`](crate::TabBar)'s default selected-label color
    /// (`ColorScheme.onSurface`).
    pub label_color: Option<Color>,
    /// Overrides [`TabBar`](crate::TabBar)'s default unselected-label color
    /// (`ColorScheme.onSurfaceVariant`).
    pub unselected_label_color: Option<Color>,
    /// Overrides [`TabBar`](crate::TabBar)'s default selected-label text
    /// style (`TextTheme.titleSmall`).
    pub label_style: Option<TextStyle>,
    /// Overrides [`TabBar`](crate::TabBar)'s default unselected-label text
    /// style (`TextTheme.titleSmall`).
    pub unselected_label_style: Option<TextStyle>,
    /// Overrides [`TabBar`](crate::TabBar)'s default divider color
    /// (`ColorScheme.outlineVariant`).
    pub divider_color: Option<Color>,
    /// Overrides [`TabBar`](crate::TabBar)'s default divider height (`1.0`).
    pub divider_height: Option<f32>,
    /// Overrides each tab's default hover/focus/press overlay color, per
    /// state.
    pub overlay_color: Option<StateColor>,
}

/// Overrides [`Radio`](crate::Radio)'s `_RadioDefaultsM3` token defaults,
/// one field at a time — an unset field here still falls through to
/// `Radio`'s own M3 default table (see `radio.rs`'s `radio_default_*`
/// functions), it does not blank the whole slot.
///
/// Flutter parity: `RadioThemeData` (`material/radio_theme.dart`, oracle tag
/// `3.44.0`), narrowed to the fields FLUI's `Radio` actually consumes:
/// [`fill_color`](Self::fill_color), [`overlay_color`](Self::overlay_color).
/// Named deferrals (no consumer in FLUI's `Radio` yet — see that module's
/// docs for the full named-divergence list): `mouse_cursor`,
/// `splash_radius`, `material_tap_target_size`, `visual_density`, `side`
/// (defaults to the ring's own resolved `fill_color`, width `2.0`),
/// `inner_radius`, `background_color` (fixed transparent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioThemeData {
    /// Overrides [`Radio`](crate::Radio)'s default ring/inner-dot fill
    /// color, per state.
    pub fill_color: Option<StateColor>,
    /// Overrides [`Radio`](crate::Radio)'s default state-overlay color (the
    /// InkWell-shaped hover/focus/press ramp), per state.
    pub overlay_color: Option<StateColor>,
}

/// Visual-style configuration provided to descendants by a
/// [`Theme`](crate::Theme) ancestor.
///
/// Two ready-made M3 baselines are available: [`ThemeData::light`] and
/// [`ThemeData::dark`]. `#[non_exhaustive]`: further component themes (input
/// decoration and any future widget's own slot) land as additional fields
/// alongside their owning widgets, not in this theming-foundation unit — see
/// the crate root docs' scope section.
///
/// Flutter parity: `ThemeData` (`material/theme_data.dart`, oracle tag
/// `3.44.0`) — implemented subset: [`color_scheme`](Self::color_scheme),
/// [`text_theme`](Self::text_theme), and the button-family/`AppBar`/`Card`/
/// `Dialog`/`FloatingActionButton` component-theme slots below (each
/// narrowed to its owning widget's actually-consumed fields — see that
/// slot's own doc comment). Deferred: `iconTheme`, `extensions`, `platform`,
/// `useMaterial3` (this crate is M3-only — there is no M2 mode to switch
/// away from), and every other widget's component theme (`inputDecorationTheme`
/// and friends) until that widget lands.
///
/// # Example
///
/// ```rust
/// use flui_material::ThemeData;
///
/// let dark = ThemeData::dark();
/// assert_eq!(dark.brightness(), dark.color_scheme.brightness);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeData {
    /// The Material 3 color roles this theme provides.
    ///
    /// Flutter parity: `ThemeData.colorScheme`.
    pub color_scheme: ColorScheme,

    /// The type-scale roles this theme provides.
    ///
    /// Flutter parity: `ThemeData.textTheme`.
    pub text_theme: TextTheme,

    /// Overrides [`ElevatedButton`](crate::ElevatedButton)'s default style.
    /// `None` (the default): no override, `ElevatedButton` uses its own
    /// M3 token table verbatim. Flutter parity:
    /// `ThemeData.elevatedButtonTheme`.
    pub elevated_button_theme: Option<ElevatedButtonThemeData>,

    /// Overrides [`FilledButton`](crate::FilledButton)'s default style.
    /// Flutter parity: `ThemeData.filledButtonTheme`.
    pub filled_button_theme: Option<FilledButtonThemeData>,

    /// Overrides [`OutlinedButton`](crate::OutlinedButton)'s default style.
    /// Flutter parity: `ThemeData.outlinedButtonTheme`.
    pub outlined_button_theme: Option<OutlinedButtonThemeData>,

    /// Overrides [`TextButton`](crate::TextButton)'s default style. Flutter
    /// parity: `ThemeData.textButtonTheme`.
    pub text_button_theme: Option<TextButtonThemeData>,

    /// Overrides [`IconButton`](crate::IconButton)'s default style. Flutter
    /// parity: `ThemeData.iconButtonTheme`.
    pub icon_button_theme: Option<IconButtonThemeData>,

    /// Overrides [`AppBar`](crate::AppBar)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.appBarTheme`.
    pub app_bar_theme: Option<AppBarThemeData>,

    /// Overrides [`Card`](crate::Card)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.cardTheme`.
    pub card_theme: Option<CardThemeData>,

    /// Overrides [`Dialog`](crate::Dialog)/[`AlertDialog`](crate::AlertDialog)'s
    /// M3 token defaults, per field. Flutter parity: `ThemeData.dialogTheme`.
    pub dialog_theme: Option<DialogThemeData>,

    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s M3
    /// token defaults, per field. Flutter parity:
    /// `ThemeData.floatingActionButtonTheme`.
    pub floating_action_button_theme: Option<FabThemeData>,

    /// Overrides [`InputDecorator`](crate::input_decorator::InputDecorator)'s
    /// M3 token defaults, per field. Flutter parity:
    /// `ThemeData.inputDecorationTheme`.
    pub input_decoration_theme: Option<InputDecorationThemeData>,

    /// Overrides [`ListTile`](crate::ListTile)'s M3 token defaults, per
    /// field. Flutter parity: `ThemeData.listTileTheme`.
    pub list_tile_theme: Option<ListTileThemeData>,

    /// Overrides [`Divider`](crate::Divider)/
    /// [`VerticalDivider`](crate::VerticalDivider)'s M3 token defaults, per
    /// field. Flutter parity: `ThemeData.dividerTheme`.
    pub divider_theme: Option<DividerThemeData>,

    /// Overrides [`Checkbox`](crate::Checkbox)'s M3 token defaults, per
    /// field. Flutter parity: `ThemeData.checkboxTheme`.
    pub checkbox_theme: Option<CheckboxThemeData>,

    /// Overrides [`Chip`](crate::Chip)/[`FilterChip`](crate::FilterChip)'s
    /// M3 token defaults, per field. Flutter parity: `ThemeData.chipTheme`.
    pub chip_theme: Option<ChipThemeData>,

    /// Overrides [`Switch`](crate::Switch)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.switchTheme`.
    pub switch_theme: Option<SwitchThemeData>,

    /// Overrides [`Radio`](crate::Radio)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.radioTheme`.
    pub radio_theme: Option<RadioThemeData>,

    /// Overrides [`NavigationBar`](crate::NavigationBar)'s M3 token defaults,
    /// per field. Flutter parity: `ThemeData.navigationBarTheme`.
    pub navigation_bar_theme: Option<NavigationBarThemeData>,

    /// Overrides [`TabBar`](crate::TabBar)'s M3 secondary token defaults,
    /// per field. Flutter parity: `ThemeData.tabBarTheme`.
    pub tab_bar_theme: Option<TabBarThemeData>,
}

impl ThemeData {
    /// The M3 light baseline: [`ColorScheme::light`] plus its derived
    /// default [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn light() -> Self {
        let color_scheme = ColorScheme::light();
        let text_theme = default_text_theme(color_scheme.brightness, color_scheme.on_surface);
        Self {
            color_scheme,
            text_theme,
            elevated_button_theme: None,
            filled_button_theme: None,
            outlined_button_theme: None,
            text_button_theme: None,
            icon_button_theme: None,
            app_bar_theme: None,
            card_theme: None,
            dialog_theme: None,
            floating_action_button_theme: None,
            input_decoration_theme: None,
            list_tile_theme: None,
            divider_theme: None,
            checkbox_theme: None,
            chip_theme: None,
            switch_theme: None,
            radio_theme: None,
            navigation_bar_theme: None,
            tab_bar_theme: None,
        }
    }

    /// The M3 dark baseline: [`ColorScheme::dark`] plus its derived default
    /// [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn dark() -> Self {
        let color_scheme = ColorScheme::dark();
        let text_theme = default_text_theme(color_scheme.brightness, color_scheme.on_surface);
        Self {
            color_scheme,
            text_theme,
            elevated_button_theme: None,
            filled_button_theme: None,
            outlined_button_theme: None,
            text_button_theme: None,
            icon_button_theme: None,
            app_bar_theme: None,
            card_theme: None,
            dialog_theme: None,
            floating_action_button_theme: None,
            input_decoration_theme: None,
            list_tile_theme: None,
            divider_theme: None,
            checkbox_theme: None,
            chip_theme: None,
            switch_theme: None,
            radio_theme: None,
            navigation_bar_theme: None,
            tab_bar_theme: None,
        }
    }

    /// This theme's brightness, read from [`ColorScheme::brightness`].
    ///
    /// Flutter parity: `ThemeData.brightness => colorScheme.brightness`
    /// (`theme_data.dart`, oracle tag `3.44.0`).
    #[must_use]
    pub fn brightness(&self) -> Brightness {
        self.color_scheme.brightness
    }

    /// Return a copy of this theme with the given overrides applied.
    ///
    /// Build the patch with [`ThemeDataOverrides::default`] and struct-update
    /// syntax — mirrors [`ColorScheme::copy_with`]'s patch-struct shape, for
    /// the same reason: `ThemeData` is `#[non_exhaustive]`, and a positional
    /// `Option<T>`-per-field signature would have to change (breaking every
    /// caller) each time a component-theme slot is added. A patch struct
    /// absorbs new fields as `..Default::default()`-compatible additions
    /// instead.
    ///
    /// Flutter parity: `ThemeData.copyWith(colorScheme: ..., textTheme: ...)`
    /// (`theme_data.dart`, oracle tag `3.44.0`), narrowed to this crate's
    /// implemented subset and reshaped as a struct (Rust has no optional
    /// named parameters).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_material::{ThemeData, ThemeDataOverrides};
    ///
    /// let base = ThemeData::light();
    /// let patched = base.copy_with(ThemeDataOverrides {
    ///     color_scheme: Some(flui_material::ColorScheme::dark()),
    ///     ..Default::default()
    /// });
    /// assert_eq!(patched.color_scheme, flui_material::ColorScheme::dark());
    /// assert_eq!(patched.text_theme, base.text_theme);
    /// ```
    #[must_use]
    pub fn copy_with(&self, overrides: ThemeDataOverrides) -> Self {
        Self {
            color_scheme: overrides.color_scheme.unwrap_or(self.color_scheme),
            text_theme: overrides
                .text_theme
                .unwrap_or_else(|| self.text_theme.clone()),
            // Each component-theme slot is itself already `Option<_>` on
            // `ThemeData`, so — unlike `color_scheme`/`text_theme` above —
            // the override field mirrors that `Option<_>` shape directly
            // rather than doubling it: `Some(_)` replaces the whole slot,
            // `None` leaves whatever this theme already had (`Some` or
            // `None`) unchanged. See `ThemeDataOverrides`'s doc comment.
            elevated_button_theme: overrides
                .elevated_button_theme
                .or_else(|| self.elevated_button_theme.clone()),
            filled_button_theme: overrides
                .filled_button_theme
                .or_else(|| self.filled_button_theme.clone()),
            outlined_button_theme: overrides
                .outlined_button_theme
                .or_else(|| self.outlined_button_theme.clone()),
            text_button_theme: overrides
                .text_button_theme
                .or_else(|| self.text_button_theme.clone()),
            icon_button_theme: overrides
                .icon_button_theme
                .or_else(|| self.icon_button_theme.clone()),
            app_bar_theme: overrides
                .app_bar_theme
                .or_else(|| self.app_bar_theme.clone()),
            card_theme: overrides.card_theme.or_else(|| self.card_theme.clone()),
            dialog_theme: overrides.dialog_theme.or_else(|| self.dialog_theme.clone()),
            floating_action_button_theme: overrides
                .floating_action_button_theme
                .or_else(|| self.floating_action_button_theme.clone()),
            input_decoration_theme: overrides
                .input_decoration_theme
                .or_else(|| self.input_decoration_theme.clone()),
            list_tile_theme: overrides
                .list_tile_theme
                .or_else(|| self.list_tile_theme.clone()),
            divider_theme: overrides
                .divider_theme
                .or_else(|| self.divider_theme.clone()),
            checkbox_theme: overrides
                .checkbox_theme
                .or_else(|| self.checkbox_theme.clone()),
            chip_theme: overrides.chip_theme.or_else(|| self.chip_theme.clone()),
            switch_theme: overrides.switch_theme.or_else(|| self.switch_theme.clone()),
            radio_theme: overrides.radio_theme.or_else(|| self.radio_theme.clone()),
            navigation_bar_theme: overrides
                .navigation_bar_theme
                .or_else(|| self.navigation_bar_theme.clone()),
            tab_bar_theme: overrides
                .tab_bar_theme
                .or_else(|| self.tab_bar_theme.clone()),
        }
    }
}

/// Patch for [`ThemeData::copy_with`] — every field mirrors a [`ThemeData`]
/// field, `None` meaning "leave unchanged".
///
/// Deliberately **not** `#[non_exhaustive]` (unlike [`ThemeData`] itself):
/// `#[non_exhaustive]` blocks external-crate struct-literal construction
/// even via `..Default::default()` functional update, which is the only way
/// callers build this patch — see [`ColorSchemeOverrides`](crate::ColorSchemeOverrides)'s
/// doc comment for the same reasoning. Every future component-theme slot on
/// [`ThemeData`] gets a matching field here; adding one is still additive
/// for any caller already writing `..Default::default()`, without needing
/// the `#[non_exhaustive]` ceremony.
///
/// Flutter parity: the optional-parameter list of `ThemeData.copyWith`
/// (`theme_data.dart`, oracle tag `3.44.0`), reshaped as a struct.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeDataOverrides {
    /// Overrides [`ThemeData::color_scheme`].
    pub color_scheme: Option<ColorScheme>,
    /// Overrides [`ThemeData::text_theme`].
    pub text_theme: Option<TextTheme>,
    /// Replaces [`ThemeData::elevated_button_theme`] wholesale when `Some`;
    /// `None` leaves it unchanged (see [`ThemeData::copy_with`]'s doc
    /// comment on why this mirrors `Option<ElevatedButtonThemeData>`
    /// directly rather than doubling the `Option`).
    pub elevated_button_theme: Option<ElevatedButtonThemeData>,
    /// Replaces [`ThemeData::filled_button_theme`] wholesale when `Some`.
    pub filled_button_theme: Option<FilledButtonThemeData>,
    /// Replaces [`ThemeData::outlined_button_theme`] wholesale when `Some`.
    pub outlined_button_theme: Option<OutlinedButtonThemeData>,
    /// Replaces [`ThemeData::text_button_theme`] wholesale when `Some`.
    pub text_button_theme: Option<TextButtonThemeData>,
    /// Replaces [`ThemeData::icon_button_theme`] wholesale when `Some`.
    pub icon_button_theme: Option<IconButtonThemeData>,
    /// Replaces [`ThemeData::app_bar_theme`] wholesale when `Some`.
    pub app_bar_theme: Option<AppBarThemeData>,
    /// Replaces [`ThemeData::card_theme`] wholesale when `Some`.
    pub card_theme: Option<CardThemeData>,
    /// Replaces [`ThemeData::dialog_theme`] wholesale when `Some`.
    pub dialog_theme: Option<DialogThemeData>,
    /// Replaces [`ThemeData::floating_action_button_theme`] wholesale when
    /// `Some`.
    pub floating_action_button_theme: Option<FabThemeData>,
    /// Replaces [`ThemeData::input_decoration_theme`] wholesale when `Some`.
    pub input_decoration_theme: Option<InputDecorationThemeData>,
    /// Replaces [`ThemeData::list_tile_theme`] wholesale when `Some`.
    pub list_tile_theme: Option<ListTileThemeData>,
    /// Replaces [`ThemeData::divider_theme`] wholesale when `Some`.
    pub divider_theme: Option<DividerThemeData>,
    /// Replaces [`ThemeData::checkbox_theme`] wholesale when `Some`.
    pub checkbox_theme: Option<CheckboxThemeData>,
    /// Replaces [`ThemeData::chip_theme`] wholesale when `Some`.
    pub chip_theme: Option<ChipThemeData>,
    /// Replaces [`ThemeData::switch_theme`] wholesale when `Some`.
    pub switch_theme: Option<SwitchThemeData>,
    /// Replaces [`ThemeData::radio_theme`] wholesale when `Some`.
    pub radio_theme: Option<RadioThemeData>,
    /// Replaces [`ThemeData::navigation_bar_theme`] wholesale when `Some`.
    pub navigation_bar_theme: Option<NavigationBarThemeData>,
    /// Replaces [`ThemeData::tab_bar_theme`] wholesale when `Some`.
    pub tab_bar_theme: Option<TabBarThemeData>,
}

impl Default for ThemeData {
    /// Same default as Flutter's `ThemeData()`: the M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_light() {
        assert_eq!(ThemeData::default(), ThemeData::light());
    }

    #[test]
    fn brightness_agrees_with_color_scheme() {
        assert_eq!(ThemeData::light().brightness(), Brightness::Light);
        assert_eq!(ThemeData::dark().brightness(), Brightness::Dark);
        assert_eq!(
            ThemeData::light().brightness(),
            ThemeData::light().color_scheme.brightness
        );
    }

    #[test]
    fn light_and_dark_are_distinct() {
        assert_ne!(ThemeData::light(), ThemeData::dark());
    }

    /// The default text theme's color is always `on_surface`, regardless of
    /// brightness — the M3 uniform-recolor behavior (see
    /// [`default_text_theme`]'s doc comment).
    #[test]
    fn default_text_theme_color_is_on_surface_for_both_brightnesses() {
        let light = ThemeData::light();
        let dark = ThemeData::dark();

        for role in light.text_theme.roles() {
            assert_eq!(
                role.and_then(|s| s.color),
                Some(light.color_scheme.on_surface)
            );
        }
        for role in dark.text_theme.roles() {
            assert_eq!(
                role.and_then(|s| s.color),
                Some(dark.color_scheme.on_surface)
            );
        }
    }

    /// The default text theme still carries `englishLike2021` geometry — the
    /// color overlay must not clobber font-size/weight/etc.
    #[test]
    fn default_text_theme_keeps_englishlike_geometry() {
        let theme = ThemeData::light();
        let body_medium = theme.text_theme.body_medium.expect("body_medium is set");
        assert_eq!(body_medium.font_size, Some(14.0));
        assert_eq!(
            body_medium.font_weight,
            Some(flui_types::typography::FontWeight::W400)
        );
    }

    #[test]
    fn copy_with_overrides_only_the_given_fields() {
        let base = ThemeData::light();
        let new_scheme = ColorScheme::dark();

        let patched = base.copy_with(ThemeDataOverrides {
            color_scheme: Some(new_scheme),
            ..Default::default()
        });

        assert_eq!(patched.color_scheme, new_scheme);
        assert_eq!(patched.text_theme, base.text_theme);
    }

    #[test]
    fn copy_with_no_overrides_is_identity() {
        let base = ThemeData::dark();
        assert_eq!(base.copy_with(ThemeDataOverrides::default()), base);
    }

    #[test]
    fn light_and_dark_leave_every_component_theme_slot_unset() {
        // A helper `fn`, not a `[ThemeData; 2]`/`vec![...]` loop: `ThemeData`
        // now carries enough `Option<ComponentThemeData>` slots that a
        // stack-allocated 2-element array trips clippy's
        // `large_stack_arrays` lint, and a `Vec` of the same trips
        // `useless_vec` right back (the loop consumes it immediately, so
        // clippy sees no reason for the heap allocation).
        fn assert_every_slot_unset(theme: &ThemeData) {
            assert!(theme.elevated_button_theme.is_none());
            assert!(theme.filled_button_theme.is_none());
            assert!(theme.outlined_button_theme.is_none());
            assert!(theme.text_button_theme.is_none());
            assert!(theme.icon_button_theme.is_none());
            assert!(theme.app_bar_theme.is_none());
            assert!(theme.card_theme.is_none());
            assert!(theme.dialog_theme.is_none());
            assert!(theme.floating_action_button_theme.is_none());
            assert!(theme.input_decoration_theme.is_none());
            assert!(theme.list_tile_theme.is_none());
            assert!(theme.divider_theme.is_none());
            assert!(theme.checkbox_theme.is_none());
            assert!(theme.chip_theme.is_none());
            assert!(theme.switch_theme.is_none());
            assert!(theme.radio_theme.is_none());
            assert!(theme.navigation_bar_theme.is_none());
        }

        assert_every_slot_unset(&ThemeData::light());
        assert_every_slot_unset(&ThemeData::dark());
    }

    #[test]
    fn copy_with_sets_the_new_component_theme_slots() {
        let base = ThemeData::light();
        let elevated_button_theme = ElevatedButtonThemeData {
            style: Some(ButtonStyle {
                elevation: Some(flui_widgets::WidgetStateProperty::all(Some(42.0))),
                ..Default::default()
            }),
        };
        let app_bar_theme = AppBarThemeData {
            elevation: Some(9.0),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            elevated_button_theme: Some(elevated_button_theme.clone()),
            app_bar_theme: Some(app_bar_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.elevated_button_theme, Some(elevated_button_theme));
        assert_eq!(patched.app_bar_theme, Some(app_bar_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.card_theme.is_none());
    }

    /// `None` in the override leaves an ALREADY-SET slot alone — it must not
    /// be read as "clear this slot back to `None`" (see `copy_with`'s doc
    /// comment on why the override field mirrors `Option<T>` directly).
    #[test]
    fn copy_with_none_preserves_an_already_set_component_theme_slot() {
        let card_theme = CardThemeData {
            elevation: Some(3.0),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            card_theme: Some(card_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.card_theme, Some(card_theme));
    }

    /// `ThemeDataOverrides::input_decoration_theme` round-trips through
    /// `copy_with` — the new slot's own dedicated proof, distinct from
    /// `copy_with_sets_the_new_component_theme_slots`'s pre-existing
    /// coverage of the other slots.
    #[test]
    fn copy_with_sets_input_decoration_theme_slot() {
        use flui_types::geometry::px;

        let base = ThemeData::light();
        let input_decoration_theme = InputDecorationThemeData {
            content_padding: Some(EdgeInsets::all(px(9.0))),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            input_decoration_theme: Some(input_decoration_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.input_decoration_theme, Some(input_decoration_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.card_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_component_theme_slot`, for
    /// the new `input_decoration_theme` slot specifically.
    #[test]
    fn copy_with_none_preserves_an_already_set_input_decoration_theme_slot() {
        use flui_types::geometry::px;

        let input_decoration_theme = InputDecorationThemeData {
            content_padding: Some(EdgeInsets::all(px(9.0))),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            input_decoration_theme: Some(input_decoration_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.input_decoration_theme, Some(input_decoration_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `list_tile_theme` slot.
    #[test]
    fn copy_with_sets_list_tile_theme_slot() {
        let base = ThemeData::light();
        let list_tile_theme = ListTileThemeData {
            min_leading_width: Some(30.0),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            list_tile_theme: Some(list_tile_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.list_tile_theme, Some(list_tile_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.divider_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `list_tile_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_list_tile_theme_slot() {
        let list_tile_theme = ListTileThemeData {
            min_leading_width: Some(30.0),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            list_tile_theme: Some(list_tile_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.list_tile_theme, Some(list_tile_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `divider_theme` slot.
    #[test]
    fn copy_with_sets_divider_theme_slot() {
        let base = ThemeData::light();
        let divider_theme = DividerThemeData {
            thickness: Some(3.0),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            divider_theme: Some(divider_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.divider_theme, Some(divider_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.list_tile_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `divider_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_divider_theme_slot() {
        let divider_theme = DividerThemeData {
            thickness: Some(3.0),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            divider_theme: Some(divider_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.divider_theme, Some(divider_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `checkbox_theme` slot.
    #[test]
    fn copy_with_sets_checkbox_theme_slot() {
        let base = ThemeData::light();
        let checkbox_theme = CheckboxThemeData {
            fill_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                1, 2, 3,
            )))),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            checkbox_theme: Some(checkbox_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.checkbox_theme, Some(checkbox_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.switch_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `checkbox_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_checkbox_theme_slot() {
        let checkbox_theme = CheckboxThemeData {
            fill_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                1, 2, 3,
            )))),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            checkbox_theme: Some(checkbox_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.checkbox_theme, Some(checkbox_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `chip_theme` slot.
    #[test]
    fn copy_with_sets_chip_theme_slot() {
        let base = ThemeData::light();
        let chip_theme = ChipThemeData {
            label_color: Some(Color::rgb(1, 2, 3)),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            chip_theme: Some(chip_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.chip_theme, Some(chip_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.switch_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `chip_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_chip_theme_slot() {
        let chip_theme = ChipThemeData {
            label_color: Some(Color::rgb(1, 2, 3)),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            chip_theme: Some(chip_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.chip_theme, Some(chip_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `switch_theme` slot.
    #[test]
    fn copy_with_sets_switch_theme_slot() {
        let base = ThemeData::light();
        let switch_theme = SwitchThemeData {
            thumb_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                4, 5, 6,
            )))),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            switch_theme: Some(switch_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.switch_theme, Some(switch_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.radio_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `switch_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_switch_theme_slot() {
        let switch_theme = SwitchThemeData {
            thumb_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                4, 5, 6,
            )))),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            switch_theme: Some(switch_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.switch_theme, Some(switch_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `radio_theme` slot.
    #[test]
    fn copy_with_sets_radio_theme_slot() {
        let base = ThemeData::light();
        let radio_theme = RadioThemeData {
            fill_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                7, 8, 9,
            )))),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            radio_theme: Some(radio_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.radio_theme, Some(radio_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.checkbox_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `radio_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_radio_theme_slot() {
        let radio_theme = RadioThemeData {
            fill_color: Some(flui_widgets::WidgetStateProperty::all(Some(Color::rgb(
                7, 8, 9,
            )))),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            radio_theme: Some(radio_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.radio_theme, Some(radio_theme));
    }

    /// Same shape as `copy_with_sets_input_decoration_theme_slot`, for the
    /// `navigation_bar_theme` slot.
    #[test]
    fn copy_with_sets_navigation_bar_theme_slot() {
        let base = ThemeData::light();
        let navigation_bar_theme = NavigationBarThemeData {
            height: Some(96.0),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            navigation_bar_theme: Some(navigation_bar_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.navigation_bar_theme, Some(navigation_bar_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.radio_theme.is_none());
    }

    /// Same already-set-slot-survives-a-`None`-override proof as
    /// `copy_with_none_preserves_an_already_set_input_decoration_theme_slot`,
    /// for the `navigation_bar_theme` slot.
    #[test]
    fn copy_with_none_preserves_an_already_set_navigation_bar_theme_slot() {
        let navigation_bar_theme = NavigationBarThemeData {
            height: Some(96.0),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            navigation_bar_theme: Some(navigation_bar_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.navigation_bar_theme, Some(navigation_bar_theme));
    }
}
