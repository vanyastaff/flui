//! [`ThemeData`] — the value [`crate::Theme`] publishes to a subtree.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::rc::Rc;

use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::styling::{BorderRadius, BorderSide, BoxDecoration, Color};
use flui_sdk::painting::typography::TextStyle;
use flui_sdk::pipeline::FocusRingStyle;
use flui_sdk::platform::{Brightness, TargetPlatform};
use flui_sdk::widgets::{SelectionStyle, WidgetStateProperty};

use crate::button_style::ButtonStyle;
use crate::color_scheme::ColorScheme;
use crate::page_transitions::PageTransitionsTheme;
use crate::shape::MaterialShape;
use crate::text_theme::TextTheme;
use crate::typography;

/// A [`Color`]-valued [`WidgetStateProperty`] that may itself resolve to
/// "no override" for a given state set — the shape every narrowed
/// `*ThemeData` color/side slot in this module uses (`fill_color`,
/// `check_color`, `overlay_color`, and friends).
type StateColor = WidgetStateProperty<Option<Color>>;

/// A typed component configuration stored in [`ThemeData`].
///
/// Component themes interpolate by switching to `other` halfway through an
/// animation. A component with continuous values can override [`lerp_to`](Self::lerp_to)
/// while retaining the same typed storage and dependency tracking.
pub trait ComponentTheme: Clone + Debug + PartialEq + 'static {
    /// Interpolate this component theme toward `other`.
    #[must_use]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        if t < 0.5 { self.clone() } else { other.clone() }
    }
}

trait ErasedComponentTheme: Debug {
    fn as_any(&self) -> &dyn Any;
    fn content_eq(&self, other: &dyn ErasedComponentTheme) -> bool;
    fn lerp_to(&self, other: &dyn ErasedComponentTheme, t: f64) -> Rc<dyn ErasedComponentTheme>;
}

#[derive(Debug)]
struct TypedComponentTheme<T: ComponentTheme>(T);

impl<T: ComponentTheme> ErasedComponentTheme for TypedComponentTheme<T> {
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn content_eq(&self, other: &dyn ErasedComponentTheme) -> bool {
        let other = other
            .as_any()
            .downcast_ref::<T>()
            .expect("BUG: component values are compared only under matching TypeId keys");
        self.0 == *other
    }

    fn lerp_to(&self, other: &dyn ErasedComponentTheme, t: f64) -> Rc<dyn ErasedComponentTheme> {
        let other = other
            .as_any()
            .downcast_ref::<T>()
            .expect("BUG: component values are interpolated only under matching TypeId keys");
        Rc::new(TypedComponentTheme(self.0.lerp_to(other, t)))
    }
}

impl PartialEq for dyn ErasedComponentTheme {
    fn eq(&self, other: &Self) -> bool {
        self.content_eq(other)
    }
}

type ComponentMap = BTreeMap<TypeId, Rc<dyn ErasedComponentTheme>>;

#[derive(Clone, Debug, PartialEq)]
enum ComponentPatch {
    Insert(Rc<dyn ErasedComponentTheme>),
    Remove,
}

/// The baseline focus ring in `scheme`'s colours: the shape of
/// [`FocusRingStyle::BUILT_IN`], a 2 px `on_surface` band outside a 1 px
/// `surface` band. The two roles contrast with each other in every scheme,
/// so one of them contrasts with whatever the ring is drawn over.
fn focus_ring(scheme: &ColorScheme) -> FocusRingStyle {
    FocusRingStyle {
        outer: scheme.on_surface,
        inner: scheme.surface,
        ..FocusRingStyle::BUILT_IN
    }
}

/// The selection highlight in `scheme`'s colours: `primary` at 40 % while
/// the selecting widget holds the focus, `on_surface` at 16 % after a blur.
fn selection(scheme: &ColorScheme) -> SelectionStyle {
    SelectionStyle::new(
        scheme.primary.with_opacity(0.40),
        scheme.on_surface.with_opacity(0.16),
    )
}

/// Compute the M3 default [`TextTheme`]: `englishLike2021` geometry overlaid
/// with a color-only theme uniformly recolored to `on_surface`.
///
/// Two parts:
///
/// - The **merge direction** — geometry as the base, a color-only theme as
///   the patch, i.e. `geometry.merge(color)`.
/// - The **uniform recolor**: the M3 default text colors both reduce to
///   `colorScheme.onSurface` regardless of brightness — so every role ends
///   up the same color, `on_surface`, not the `black54`/`black87`/`black`
///   (or `white70`/`white`) tiers
///   [`TextTheme::black_mountain_view`]/[`TextTheme::white_mountain_view`]
///   themselves carry.
///
/// This is baked once, at [`ThemeData::light`]/[`ThemeData::dark`]
/// construction time, rather than recomputed lazily on every `Theme::of`
/// read keyed on the ambient locale's script category (English vs.
/// dense/tall scripts — see [`crate::typography`] module docs on why
/// dense/tall are deferred): FLUI V1 has no script-category-resolving
/// localization consumer yet, so only the `englishLike` default exists.
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
/// **Named reduction**: a subtree could override the style with a
/// standalone `ElevatedButtonTheme` `InheritedTheme` widget; FLUI V1 has no
/// per-widget `InheritedTheme` wrappers yet, so `ElevatedButton` reads this
/// typed component entry through [`Theme::component_of`](crate::Theme::component_of).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElevatedButtonThemeData {
    /// Overrides for [`ElevatedButton`](crate::ElevatedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`FilledButton`](crate::FilledButton)'s default [`ButtonStyle`]
/// (both the plain and `tonal` variants share this one slot). Same named
/// reduction as [`ElevatedButtonThemeData`] (no standalone `InheritedTheme`
/// wrapper yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilledButtonThemeData {
    /// Overrides for [`FilledButton`](crate::FilledButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`OutlinedButton`](crate::OutlinedButton)'s default
/// [`ButtonStyle`]. Same named reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutlinedButtonThemeData {
    /// Overrides for [`OutlinedButton`](crate::OutlinedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`TextButton`](crate::TextButton)'s default [`ButtonStyle`].
/// Same named reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextButtonThemeData {
    /// Overrides for [`TextButton`](crate::TextButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`IconButton`](crate::IconButton)'s default [`ButtonStyle`].
/// Same named reduction as [`ElevatedButtonThemeData`].
/// Not to be confused with [`flui_sdk::widgets::IconThemeData`], which colors an
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
/// Narrowed to the fields FLUI's `AppBar` actually consumes:
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
    pub elevation: Option<f64>,
    /// Overrides the title's text style verbatim — the cascade is
    /// `widget.titleTextStyle ?? appBarTheme.titleTextStyle ??
    /// defaults.titleTextStyle?.copyWith(color: foregroundColor)`: unlike
    /// the default tier, a theme-supplied style is used as-is, not recolored to the resolved
    /// [`foreground_color`](Self::foreground_color).
    pub title_text_style: Option<TextStyle>,
}

/// Overrides [`Card`](crate::Card)'s `_CardDefaultsM3` token defaults, one
/// field at a time.
///
/// Narrowed to the fields FLUI's `Card` actually consumes:
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
    pub elevation: Option<f64>,
    /// Overrides [`Card`](crate::Card)'s default shape (a 12dp rounded
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides [`Card`](crate::Card)'s default outer margin
    /// (`EdgeInsets.all(4.0)`).
    pub margin: Option<EdgeInsets>,
}

/// Overrides [`CarouselView`](crate::CarouselView)'s M3 token defaults, one field at a time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarouselViewThemeData {
    /// Overrides the cards' fill color (`ColorScheme.surface`).
    pub background_color: Option<Color>,
    /// Overrides the cards' elevation (`0.0`).
    pub elevation: Option<f64>,
    /// Overrides the cards' shape (a 28 dp rounded rectangle, M3's extra-large corner).
    pub shape: Option<MaterialShape>,
    /// Overrides the cards' state layer (`ColorScheme.onSurface` at the M3 pressed, hovered and
    /// focused alphas).
    pub overlay_color: Option<WidgetStateProperty<Option<Color>>>,
    /// Overrides the inset of each card inside its item's mask (`EdgeInsets::all(4.0)`, an
    /// 8 dp gap between neighbours).
    pub padding: Option<EdgeInsets>,
}

/// Overrides [`Dialog`](crate::Dialog)/[`AlertDialog`](crate::AlertDialog)'s
/// `_DialogDefaultsM3` token defaults, one field at a time.
///
/// Narrowed to the fields FLUI's `Dialog`/`AlertDialog`
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
    pub elevation: Option<f64>,
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
/// Narrowed to the fields FLUI's `FloatingActionButton` actually consumes:
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
    pub elevation: Option<f64>,
}

/// Overrides [`InputDecorator`](crate::input_decorator::InputDecorator)'s
/// `_InputDecoratorDefaultsM3` token defaults, one field at a time — an unset
/// field here still falls through to the M3 default table for that field
/// (see `input_decorator.rs`'s `default_*` functions), it does not blank the
/// whole slot.
///
/// Narrowed to the fields FLUI's `InputDecorator` actually consumes: [`fill_color`](Self::fill_color),
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
    pub active_indicator: Option<WidgetStateProperty<Option<BorderSide<f64>>>>,
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
/// Narrowed to the fields FLUI's `ListTile` actually consumes. FLUI
/// collapses the two theme tiers (an ambient `ListTileTheme` inherited
/// widget and this [`ThemeData`] slot) into this one slot, the same
/// "named reduction" every other component-theme type in this crate already
/// makes (see [`CardThemeData`]'s doc comment) — so this slot stands in for
/// both reads.
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
    pub horizontal_title_gap: Option<f64>,
    /// Overrides the minimum padding above/below the title/subtitle column
    /// (`8.0`).
    pub min_vertical_padding: Option<f64>,
    /// Overrides the minimum width reserved for
    /// [`ListTile::leading`](crate::ListTile::leading) (`24.0`).
    pub min_leading_width: Option<f64>,
    /// Overrides the tile's minimum height. `None` (the default) falls
    /// through to the one/two/three-line table — see `list_tile.rs`'s
    /// `default_tile_height`.
    pub min_tile_height: Option<f64>,
    /// Overrides [`ListTile::is_three_line`](crate::ListTile::is_three_line)
    /// when the widget itself leaves it unset.
    pub is_three_line: Option<bool>,
}

/// Overrides [`Divider`](crate::Divider)/[`VerticalDivider`](crate::VerticalDivider)'s
/// `_DividerDefaultsM3` token defaults, one field at a time.
///
/// Every field has a consumer here, so nothing is narrowed. FLUI collapses
/// a `DividerTheme` inherited widget into this one [`ThemeData`] slot, the same named reduction
/// [`ListTileThemeData`] makes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DividerThemeData {
    /// Overrides the line color (`ColorScheme.outlineVariant`).
    pub color: Option<Color>,
    /// Overrides [`Divider`](crate::Divider)'s height /
    /// [`VerticalDivider`](crate::VerticalDivider)'s width (`16.0`).
    pub space: Option<f64>,
    /// Overrides the line's thickness (`1.0`).
    pub thickness: Option<f64>,
    /// Overrides the leading-edge indent (`0.0`).
    pub indent: Option<f64>,
    /// Overrides the trailing-edge indent (`0.0`).
    pub end_indent: Option<f64>,
    /// Overrides the line's corner radius. `None` (the default) paints a
    /// square-cornered line.
    pub radius: Option<BorderRadius>,
}

/// Overrides [`Checkbox`](crate::Checkbox)'s `_CheckboxDefaultsM3` token
/// defaults, one field at a time — an unset field here still falls through
/// to `Checkbox`'s own M3 default table (see `checkbox.rs`'s
/// `checkbox_default_*` functions), it does not blank the whole slot.
///
/// Narrowed to the fields FLUI's `Checkbox` actually consumes: [`fill_color`](Self::fill_color), [`check_color`](Self::check_color),
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
    pub side: Option<WidgetStateProperty<Option<BorderSide<f64>>>>,
}

/// Overrides [`Chip`](crate::Chip)/[`FilterChip`](crate::FilterChip)'s
/// `_ChipDefaultsM3`/`_FilterChipDefaultsM3` token defaults, one field at a
/// time — an unset field here still falls through to the owning widget's own
/// M3 default table (see `chip.rs`'s `chip_default_*`/`chip_*_color_default`
/// functions), it does not blank the whole slot.
///
/// Narrowed to the fields FLUI's [`Chip`](crate::Chip)/
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
    pub side: Option<BorderSide<f64>>,
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
/// Narrowed to the fields FLUI's `Switch` actually consumes:
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
/// Narrowed to the fields FLUI's `NavigationBar` actually consumes: [`height`](Self::height), [`background_color`](Self::background_color),
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
    pub height: Option<f64>,
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default background
    /// color (`ColorScheme.surfaceContainer`).
    pub background_color: Option<Color>,
    /// Overrides [`NavigationBar`](crate::NavigationBar)'s default elevation
    /// (`3.0`).
    pub elevation: Option<f64>,
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
    /// hover/focus/press ramp). `None` (the default, at every tier) uses the
    /// ambient [`crate::RippleConfiguration`] colour at the M3 state-layer
    /// alphas (the oracle sets no default `overlayColor` in
    /// `_NavigationBarDefaultsM3` either).
    pub overlay_color: Option<StateColor>,
}

/// Overrides [`TabBar`](crate::TabBar)'s `_TabsSecondaryDefaultsM3` token
/// defaults, one field at a time — an unset field still falls through to
/// `TabBar`'s own M3 secondary default table (see `tabs.rs`'s
/// `resolve_style`), it does not blank the whole slot.
///
/// Narrowed to the fields FLUI's `TabBar` actually consumes:
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
    pub divider_height: Option<f64>,
    /// Overrides each tab's default hover/focus/press overlay color, per
    /// state.
    pub overlay_color: Option<StateColor>,
}

/// Overrides [`Radio`](crate::Radio)'s `_RadioDefaultsM3` token defaults,
/// one field at a time — an unset field here still falls through to
/// `Radio`'s own M3 default table (see `radio.rs`'s `radio_default_*`
/// functions), it does not blank the whole slot.
///
/// Narrowed to the fields FLUI's `Radio` actually consumes:
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

/// Overrides [`DataTable`](crate::DataTable)'s M3 token defaults, one field
/// at a time — an unset field here still falls through to `DataTable`'s own
/// M3 default table (see `data_table.rs`'s `DEFAULT_*` constants), it does
/// not blank the whole slot.
///
/// Narrowed to the fields FLUI's `DataTable` actually consumes. Named deferrals (no consumer in FLUI's `DataTable` yet — see
/// that module's docs for the full named-divergence list): `headingCellCursor`,
/// `dataRowCursor` (no `sortColumnIndex`/mouse-cursor surface yet),
/// `headingRowAlignment` (no sort-arrow layout to align around yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataTableThemeData {
    /// Overrides the table's background/border decoration. Defaults to no
    /// decoration.
    pub decoration: Option<BoxDecoration<f64>>,
    /// Overrides the data rows' background color, per state (`Selected`
    /// resolves the M3 default: `colorScheme.primary` at 8% opacity).
    pub data_row_color: Option<StateColor>,
    /// Overrides each data row's minimum height. Defaults to
    /// `kMinInteractiveDimension` (`48.0`).
    pub data_row_min_height: Option<f64>,
    /// Overrides each data row's maximum height. Defaults to
    /// `kMinInteractiveDimension` (`48.0`).
    pub data_row_max_height: Option<f64>,
    /// Overrides the data cells' text style. Defaults to `TextTheme.bodyMedium`.
    pub data_text_style: Option<TextStyle>,
    /// Overrides the heading row's background color, per state.
    pub heading_row_color: Option<StateColor>,
    /// Overrides the heading row's height. Defaults to `56.0`.
    pub heading_row_height: Option<f64>,
    /// Overrides the heading cells' text style. Defaults to `TextTheme.titleSmall`.
    pub heading_text_style: Option<TextStyle>,
    /// Overrides the margin between the table's edges and the first/last
    /// column's content. Defaults to `24.0`.
    pub horizontal_margin: Option<f64>,
    /// Overrides the margin between adjacent data columns. Defaults to `56.0`.
    pub column_spacing: Option<f64>,
    /// Overrides the divider painted between rows. Defaults to `1.0`.
    pub divider_thickness: Option<f64>,
    /// Overrides the margin around the leading selection checkbox, when
    /// shown. Defaults to [`Self::horizontal_margin`] (start) and half of it
    /// (end).
    pub checkbox_horizontal_margin: Option<f64>,
}

macro_rules! component_themes {
    ($($theme:ty),+ $(,)?) => {
        $(impl ComponentTheme for $theme {})+
    };
}

component_themes!(
    ElevatedButtonThemeData,
    FilledButtonThemeData,
    OutlinedButtonThemeData,
    TextButtonThemeData,
    IconButtonThemeData,
    AppBarThemeData,
    CardThemeData,
    CarouselViewThemeData,
    DialogThemeData,
    FabThemeData,
    InputDecorationThemeData,
    ListTileThemeData,
    DividerThemeData,
    CheckboxThemeData,
    ChipThemeData,
    SwitchThemeData,
    RadioThemeData,
    NavigationBarThemeData,
    TabBarThemeData,
    DataTableThemeData,
);

/// Which Material design a [`ThemeData`] follows.
///
/// One choice that components read rather than a flag per component, so an app switches
/// to Material 3 Expressive in one place and each component that has an Expressive form
/// follows it. `#[non_exhaustive]`: a later Material design is a new variant.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MaterialStyle {
    /// The Material 3 baseline.
    #[default]
    Baseline,
    /// Material 3 Expressive.
    Expressive,
}

/// Visual-style configuration provided to descendants by a
/// [`Theme`](crate::Theme) ancestor.
///
/// The seven ordinary fields retain static dependency masks. Component
/// configurations live in typed entries instead: install one with
/// [`with_component`](Self::with_component), read it with
/// [`component`](Self::component), and remove it with
/// [`without_component`](Self::without_component). Entries are local to each
/// theme value and may use any [`ComponentTheme`] implementation.
///
/// # Example
///
/// ```rust
/// use flui_material::{CardThemeData, ThemeData};
///
/// let theme = ThemeData::dark().with_component(CardThemeData { elevation: Some(2.0), ..Default::default() });
/// assert_eq!(theme.component::<CardThemeData>().and_then(|card| card.elevation), Some(2.0));
/// assert_eq!(theme.brightness(), theme.color_scheme.brightness);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeData {
    /// The Material 3 color roles this theme provides.
    pub color_scheme: ColorScheme,

    /// The type-scale roles this theme provides.
    pub text_theme: TextTheme,

    /// The keyboard focus indicator every control under the [`Theme`](crate::Theme)
    /// draws: [`Theme`](crate::Theme) provides it as the inherited
    /// `DefaultFocusRingStyle`. The baselines take two bands from the color
    /// scheme, `on_surface` outside and `surface` inside, so one of them
    /// contrasts with any background the control sits on.
    pub focus_ring: FocusRingStyle,

    /// How selected text is highlighted: [`Theme`](crate::Theme) provides it
    /// as the inherited `DefaultSelectionStyle`, which selection areas and
    /// text fields read. The baselines highlight with `primary` while
    /// focused and dim to `on_surface` after a blur.
    pub selection: SelectionStyle,

    /// The platform this theme adapts to: today, which page transition
    /// [`page_transitions_theme`](Self::page_transitions_theme) picks. The
    /// baselines use the platform the app was built for.
    pub platform: TargetPlatform,

    /// Which Material design the components follow: the M3 baseline, or Material 3
    /// Expressive. Today it picks the indicator [`MaterialApp`](crate::MaterialApp) gives
    /// every `RefreshIndicator`: a circular progress indicator for the baseline, a
    /// [`LoadingIndicator`](crate::LoadingIndicator) for Expressive.
    pub style: MaterialStyle,

    /// Which transition page routes run, per platform (ADR-0221 D3).
    /// [`MaterialApp`](crate::MaterialApp) installs the one for
    /// [`platform`](Self::platform).
    pub page_transitions_theme: PageTransitionsTheme,

    components: ComponentMap,
}

impl ThemeData {
    fn baseline(color_scheme: &ColorScheme) -> Self {
        let text_theme = default_text_theme(color_scheme.brightness, color_scheme.on_surface);
        Self {
            text_theme,
            focus_ring: focus_ring(color_scheme),
            selection: selection(color_scheme),
            color_scheme: *color_scheme,
            platform: TargetPlatform::current(),
            style: MaterialStyle::default(),
            page_transitions_theme: PageTransitionsTheme::default(),
            components: ComponentMap::new(),
        }
    }

    /// The M3 light baseline: [`ColorScheme::light`] plus its derived
    /// default [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn light() -> Self {
        Self::baseline(&ColorScheme::light())
    }

    /// The M3 dark baseline: [`ColorScheme::dark`] plus its derived default
    /// [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn dark() -> Self {
        Self::baseline(&ColorScheme::dark())
    }

    /// Return a theme with `component` inserted or replacing its previous value.
    #[must_use]
    pub fn with_component<T: ComponentTheme>(mut self, component: T) -> Self {
        self.components
            .insert(TypeId::of::<T>(), Rc::new(TypedComponentTheme(component)));
        self
    }

    /// Return a theme without the `T` component entry.
    #[must_use]
    pub fn without_component<T: ComponentTheme>(mut self) -> Self {
        self.components.remove(&TypeId::of::<T>());
        self
    }

    /// Borrow the `T` component entry, or `None` when this theme does not contain it.
    #[must_use]
    pub fn component<T: ComponentTheme>(&self) -> Option<&T> {
        self.components.get(&TypeId::of::<T>()).map(|component| {
            component
                .as_any()
                .downcast_ref::<T>()
                .expect("BUG: a component map entry must match its TypeId key")
        })
    }

    pub(crate) fn changed_component_types(&self, old: &Self) -> Vec<TypeId> {
        self.components
            .keys()
            .chain(old.components.keys())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|id| self.components.get(id) != old.components.get(id))
            .collect()
    }

    pub(crate) fn lerp_with_base(
        &self,
        other: &Self,
        t: f64,
        color_scheme: &ColorScheme,
        text_theme: TextTheme,
        focus_ring: FocusRingStyle,
        selection: SelectionStyle,
    ) -> Self {
        let mut components = ComponentMap::new();
        let keys = self
            .components
            .keys()
            .chain(other.components.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        for id in keys {
            match (self.components.get(&id), other.components.get(&id)) {
                (Some(from), Some(to)) => {
                    components.insert(id, from.lerp_to(&**to, t));
                }
                (Some(from), None) if t < 0.5 => {
                    components.insert(id, Rc::clone(from));
                }
                (None, Some(to)) if t >= 0.5 => {
                    components.insert(id, Rc::clone(to));
                }
                _ => {}
            }
        }
        let mut result = if t < 0.5 { self.clone() } else { other.clone() };
        result.color_scheme = *color_scheme;
        result.text_theme = text_theme;
        result.focus_ring = focus_ring;
        result.selection = selection;
        result.components = components;
        result
    }

    /// This theme's brightness, read from [`ColorScheme::brightness`].
    #[must_use]
    pub fn brightness(&self) -> Brightness {
        self.color_scheme.brightness
    }

    /// Return a copy of this theme with the given base-field and component patches applied.
    ///
    /// An absent component patch leaves that entry unchanged. Use
    /// [`ThemeDataOverrides::without_component`] for explicit removal and
    /// [`ThemeDataOverrides::with_component`] for insertion or replacement.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_material::{CardThemeData, ThemeData, ThemeDataOverrides};
    ///
    /// let base = ThemeData::light();
    /// let patched = base.copy_with(ThemeDataOverrides::default()
    ///     .with_component(CardThemeData { elevation: Some(3.0), ..Default::default() }));
    /// assert_eq!(patched.component::<CardThemeData>().and_then(|card| card.elevation), Some(3.0));
    /// assert_eq!(patched.text_theme, base.text_theme);
    /// ```
    #[must_use]
    pub fn copy_with(&self, overrides: ThemeDataOverrides) -> Self {
        let mut components = self.components.clone();
        for (id, patch) in overrides.components {
            match patch {
                ComponentPatch::Insert(component) => {
                    components.insert(id, component);
                }
                ComponentPatch::Remove => {
                    components.remove(&id);
                }
            }
        }
        Self {
            color_scheme: overrides.color_scheme.unwrap_or(self.color_scheme),
            text_theme: overrides
                .text_theme
                .unwrap_or_else(|| self.text_theme.clone()),
            focus_ring: overrides.focus_ring.unwrap_or(self.focus_ring),
            selection: overrides.selection.unwrap_or(self.selection),
            style: overrides.style.unwrap_or(self.style),
            platform: overrides.platform.unwrap_or(self.platform),
            page_transitions_theme: overrides
                .page_transitions_theme
                .unwrap_or_else(|| self.page_transitions_theme.clone()),
            components,
        }
    }
}

/// Patch for [`ThemeData::copy_with`].
///
/// A base field set to `Some` replaces that field; `None` leaves it unchanged.
/// Component patches are typed and distinguish an absent patch from removal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeDataOverrides {
    /// Overrides [`ThemeData::color_scheme`].
    pub color_scheme: Option<ColorScheme>,
    /// Overrides [`ThemeData::text_theme`].
    pub text_theme: Option<TextTheme>,
    /// Overrides [`ThemeData::focus_ring`].
    pub focus_ring: Option<FocusRingStyle>,
    /// Overrides [`ThemeData::selection`].
    pub selection: Option<SelectionStyle>,
    /// Overrides [`ThemeData::style`].
    pub style: Option<MaterialStyle>,
    /// Overrides [`ThemeData::platform`].
    pub platform: Option<TargetPlatform>,
    /// Overrides [`ThemeData::page_transitions_theme`].
    pub page_transitions_theme: Option<PageTransitionsTheme>,
    components: BTreeMap<TypeId, ComponentPatch>,
}

impl ThemeDataOverrides {
    /// Insert or replace the `T` component when the patch is applied.
    #[must_use]
    pub fn with_component<T: ComponentTheme>(mut self, component: T) -> Self {
        self.components.insert(
            TypeId::of::<T>(),
            ComponentPatch::Insert(Rc::new(TypedComponentTheme(component))),
        );
        self
    }

    /// Remove the `T` component when the patch is applied.
    #[must_use]
    pub fn without_component<T: ComponentTheme>(mut self) -> Self {
        self.components
            .insert(TypeId::of::<T>(), ComponentPatch::Remove);
        self
    }
}

impl ThemeData {
    /// Field mask of `color_scheme` (bit 0).
    pub const FIELD_COLOR_SCHEME: flui_sdk::view::FieldMask<Self> =
        flui_sdk::view::FieldMask::bit(0);
    /// Field mask of `text_theme` (bit 1).
    pub const FIELD_TEXT_THEME: flui_sdk::view::FieldMask<Self> = flui_sdk::view::FieldMask::bit(1);
    /// Field mask of `focus_ring` (bit 2).
    pub const FIELD_FOCUS_RING: flui_sdk::view::FieldMask<Self> = flui_sdk::view::FieldMask::bit(2);
    /// Field mask of `selection` (bit 3).
    pub const FIELD_SELECTION: flui_sdk::view::FieldMask<Self> = flui_sdk::view::FieldMask::bit(3);
    /// Field mask of `platform` (bit 4).
    pub const FIELD_PLATFORM: flui_sdk::view::FieldMask<Self> = flui_sdk::view::FieldMask::bit(4);
    /// Field mask of `style` (bit 5).
    pub const FIELD_STYLE: flui_sdk::view::FieldMask<Self> = flui_sdk::view::FieldMask::bit(5);
    /// Field mask of `page_transitions_theme` (bit 6).
    pub const FIELD_PAGE_TRANSITIONS_THEME: flui_sdk::view::FieldMask<Self> =
        flui_sdk::view::FieldMask::bit(6);
}

impl flui_sdk::view::InheritedData for ThemeData {
    fn field_mask_diff(&self, other: &Self) -> flui_sdk::view::FieldMask<Self> {
        let mut changed = flui_sdk::view::FieldMask::NONE;
        if self.color_scheme != other.color_scheme {
            changed = changed.union(Self::FIELD_COLOR_SCHEME);
        }
        if self.text_theme != other.text_theme {
            changed = changed.union(Self::FIELD_TEXT_THEME);
        }
        if self.focus_ring != other.focus_ring {
            changed = changed.union(Self::FIELD_FOCUS_RING);
        }
        if self.selection != other.selection {
            changed = changed.union(Self::FIELD_SELECTION);
        }
        if self.platform != other.platform {
            changed = changed.union(Self::FIELD_PLATFORM);
        }
        if self.style != other.style {
            changed = changed.union(Self::FIELD_STYLE);
        }
        if self.page_transitions_theme != other.page_transitions_theme {
            changed = changed.union(Self::FIELD_PAGE_TRANSITIONS_THEME);
        }
        changed
    }
}

impl Default for ThemeData {
    /// The M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}
