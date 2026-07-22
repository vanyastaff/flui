//! [`ListTile`] — a single fixed-height row of leading/title/subtitle/
//! trailing content, the data-display building block for lists, drawers, and
//! menus.
//!
//! # Flutter parity
//!
//! `material/list_tile.dart`'s `ListTile`, composed with `_LisTileDefaultsM3`
//! (oracle tag `3.44.0`, M3-only — this crate has no M2 mode):
//!
//! | Token | Value | Oracle |
//! |---|---|---|
//! | `contentPadding` | `EdgeInsetsDirectional.only(start: 16.0, end: 24.0)` | `_LisTileDefaultsM3` constructor |
//! | `minLeadingWidth` | `24.0` | `_LisTileDefaultsM3` constructor |
//! | `minVerticalPadding` | `8.0` | `_LisTileDefaultsM3` constructor |
//! | `shape` | `RoundedRectangleBorder()` (square corners) | `_LisTileDefaultsM3` constructor |
//! | `tileColor` | `Colors.transparent` | `_LisTileDefaultsM3.tileColor` |
//! | `titleTextStyle` | `TextTheme.bodyLarge` colored `onSurface` | `_LisTileDefaultsM3.titleTextStyle` |
//! | `subtitleTextStyle` | `TextTheme.bodyMedium` colored `onSurfaceVariant` | `_LisTileDefaultsM3.subtitleTextStyle` |
//! | `leadingAndTrailingTextStyle` | `TextTheme.labelSmall` colored `onSurfaceVariant` | `_LisTileDefaultsM3.leadingAndTrailingTextStyle` |
//! | `selectedColor` | `ColorScheme.primary` | `_LisTileDefaultsM3.selectedColor` |
//! | `iconColor` | `ColorScheme.onSurfaceVariant` | `_LisTileDefaultsM3.iconColor` |
//!
//! `minLeadingWidth` is `24.0`, not `40.0` — that's `_LisTileDefaultsM2`'s
//! value; the M3 table halves it.
//!
//! The one/two/three-line minimum tile heights (`_RenderListTile._defaultTileHeight`,
//! `list_tile.dart`, tag `3.44.0`) are a **flat literal table**, not part of
//! `_LisTileDefaultsM3`: `56.0`/`72.0`/`88.0` (one/two/three lines), reduced
//! to `48.0`/`64.0`/`76.0` when [`ListTile::dense`] is set. Note the
//! three-line dense height is `76.0`, not `88.0 - 12.0`— it's its own table
//! entry, not derived arithmetically.
//!
//! `horizontalTitleGap` defaults to `16.0` — also a bare `list_tile.dart`
//! literal (`ListTile.build`'s `horizontalTitleGap ?? tileTheme.horizontalTitleGap
//! ?? 16`), not a `_LisTileDefaultsM3` field.
//!
//! # Scope: M3 composition, not a `_RenderListTile` port
//!
//! The oracle lays out leading/title/subtitle/trailing with a custom
//! baseline-aware `_RenderListTile` (`SlottedMultiChildRenderObjectWidget`),
//! computing exact baseline offsets per `ListTileTitleAlignment` variant and
//! a `titleStart`/`adjustedTrailingWidth` intrinsic-width negotiation. This
//! substrate composes the SAME visible shape from `Row`/`Column`/`Padding`/
//! `ConstrainedBox`/`Expanded` instead — the honest claim is **M3 list-tile
//! composition (colors, typography, min-heights, tap), not a
//! `_RenderListTile` port**. Concretely:
//!
//! - **`leading`** is wrapped in `ConstrainedBox(min_width:
//!   min_leading_width)` — Flutter parity for `titleStart = max(minLeadingWidth,
//!   leadingSize.width) + horizontalTitleGap` (`list_tile.dart` `:1607-1609`):
//!   the leading slot never shrinks below `min_leading_width`, but (unlike the
//!   oracle) this substrate does not separately track and error on a leading
//!   widget that consumes the entire tile width.
//! - **`title`/`subtitle`** sit in an `Expanded(Column(...))`, wrapped in a
//!   single `Padding(vertical: min_vertical_padding)` around the whole
//!   two-line block — Flutter parity for "the minimum padding on the top and
//!   bottom of the title and subtitle widgets" ([`ListTile::min_vertical_padding`]'s
//!   doc), but NOT the oracle's per-`ListTileTitleAlignment` baseline math
//!   (`top`/`center`/`bottom`/`threeLine`/`titleHeight` each compute a
//!   different y-offset — `list_tile.dart` `:130-165`). `ListTileTitleAlignment`
//!   itself is not exposed; every tile lays out as the oracle's `threeLine`
//!   variant's **`≤2`-line (centered) arm, always** — title above subtitle,
//!   both left-aligned, `leading`/`trailing` vertically centered via
//!   `Row`'s `CrossAxisAlignment::Center`. The oracle's `threeLine` variant
//!   only centers `leading`/`trailing` when `isThreeLine` is `false`; when
//!   `true`, it TOP-aligns them instead (`listTile.minVerticalPadding` from
//!   the tile's top edge, `list_tile.dart` `:138-146`, `:161`) so a
//!   three-line tile's icon sits flush with the title, not centered against
//!   the full three-line block. This substrate always centers regardless of
//!   `is_three_line` — a named divergence, not (as an earlier revision of
//!   these docs claimed) a faithful port of the `threeLine` variant as a
//!   whole.
//! - **`trailing`**'s reserved-width floor (`max(trailingSize.width +
//!   horizontalTitleGap, 32.0)`, `list_tile.dart` `:1611-1613`) is not
//!   replicated — `Row`'s own intrinsic sizing handles it instead.
//! - **`isThreeLine`** is ported as the SAME explicit `bool` flag the oracle
//!   itself uses (`ListTile.isThreeLine`, never a subtitle-line-count
//!   heuristic) — see [`ListTile::is_three_line`].
//! - **`dense`** switches between the two literal height tables above AND
//!   clamps `title`'s resolved font size to `13.0`/subtitle's to `12.0`
//!   (`titleStyle.copyWith(fontSize: _isDenseLayout ? 13.0 : null)` and the
//!   subtitle equivalent, `list_tile.dart` `:923-926`/`:939-942`) — both
//!   ported. `VisualDensity`'s finer `baseSizeAdjustment` nudge
//!   (`list_tile.dart` `:1548`) is a named deferral (no `VisualDensity`
//!   consumer wired to this substrate's `ListTile` yet).
//! - **Baseline alignment** — every text run in this composition sits by
//!   `Column`/`Row` box-model layout, not by shared text baseline. The
//!   oracle's `_ListTile.performLayout` positions `title`/`subtitle` by
//!   their own top-of-box offsets too (not a true cross-widget baseline
//!   grid), so this is a smaller divergence than it may sound — but it is
//!   still not a byte-for-byte port and is named here for the record.
//!
//! # State-color cascade: a flat resolve, not `WidgetStateColor`
//!
//! The oracle resolves `iconColor`/`textColor` through
//! `_IndividualOverrides`, a `WidgetStateProperty<Color?>` with a
//! `disabled > selected > enabled` precedence (`list_tile.dart`
//! `_IndividualOverrides.resolve`, `:1217-1229`) and a `WidgetStateColor`
//! escape hatch this crate's plain `Color` fields have no counterpart for
//! (matching every other widget in this crate — see `crate::icon_button`'s
//! module docs). `resolve_content_color` carries the exact same
//! precedence, collapsed to a direct `if`/`else if`/`else` since there is no
//! live [`flui_widgets::WidgetStatesController`] backing a stateless
//! `ListTile` the way [`crate::ink_well::InkWell`] backs an interactive
//! surface — `selected`/`enabled` are plain `bool` widget properties, so a
//! static resolve at `build` time is exact, not an approximation.
//!
//! `theme.disabled_color` (Flutter's `ThemeData.disabledColor`) has no
//! `ThemeData` field in this crate yet — the disabled branch instead uses
//! `on_surface@38%`, the same M3 "disabled content" convention
//! `crate::icon_button`'s own `default_style` already applies for the
//! identical reason (see that module's `default_style`).
//!
//! # Deferred, and named
//!
//! - **`onLongPress`**, **`onFocusChange`**, **`mouseCursor`**,
//!   **`focusColor`/`hoverColor`/`splashColor`**, **`focusNode`/`autofocus`**,
//!   **`enableFeedback`**, **`statesController`** — no override surface yet,
//!   matching every other V1 interactive widget in this crate.
//! - **`style`** (`ListTileStyle.list`/`drawer`, an M2-only fork) —
//!   irrelevant to this M3-only crate.
//! - **`visualDensity`**, **`titleAlignment`** — see the "Scope" section.
//! - **`internalAddSemanticForOnTap`** — always behaves as the oracle's own
//!   eventual-default (`true`): the emitted `Semantics.button` flag is just
//!   `on_tap.is_some()`.
//! - **`ListTile.divideTiles`** — the `Iterable<Widget>` inter-tile divider
//!   helper; a natural, additive follow-up once a caller list-builds
//!   `ListTile`s (see [`crate::divider`]'s module docs).
//! - **The oracle's `SafeArea`/`IconTheme.merge` wrapper layers ARE
//!   ported**: `SafeArea::new().top(false).bottom(false).minimum(...)`, and
//!   `IconTheme::new(IconThemeData { color: .., ..IconTheme::of(ctx) }, ..)`
//!   — a genuine merge over the ambient `IconTheme::of(ctx)` snapshot (only
//!   `color` is overridden; `size`/`opacity`/etc. pass through from whatever
//!   `IconTheme` already wraps this tile), matching `IconTheme.merge`'s own
//!   contract, not a blanket replacement. Only `IconButtonTheme` has no
//!   port: FLUI has no `IconButtonTheme` ambient/`InheritedWidget` at all yet
//!   (`IconButton` reads `ThemeData.icon_button_theme` directly — see
//!   `crate::icon_button`'s module docs), so a nested `IconButton` in
//!   `leading`/`trailing` will not automatically pick up this tile's
//!   resolved icon color the way the
//!   oracle's does.

use std::rc::Rc;

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_types::{EdgeInsets, Pixels};
use flui_view::prelude::*;
use flui_widgets::{
    Column, ConstrainedBox, CrossAxisAlignment, DefaultTextStyle, Expanded, IconTheme,
    IconThemeData, MainAxisSize, Padding, Row, SafeArea, Semantics, SizedBox,
};

use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// `_LisTileDefaultsM3`'s content padding start inset (`list_tile.dart`,
/// oracle tag `3.44.0`).
const CONTENT_PADDING_START: f32 = 16.0;
/// `_LisTileDefaultsM3`'s content padding end inset (`list_tile.dart`,
/// oracle tag `3.44.0`).
const CONTENT_PADDING_END: f32 = 24.0;
/// `_LisTileDefaultsM3`'s minimum leading width (`list_tile.dart`, oracle tag
/// `3.44.0`) — `24.0`, not M2's `40.0`.
const MIN_LEADING_WIDTH: f32 = 24.0;
/// `_LisTileDefaultsM3`'s minimum vertical padding (`list_tile.dart`, oracle
/// tag `3.44.0`).
const MIN_VERTICAL_PADDING: f32 = 8.0;
/// `ListTile.build`'s bare horizontal-title-gap literal (`list_tile.dart`
/// `:1029`, oracle tag `3.44.0`) — not part of `_LisTileDefaultsM3`.
const HORIZONTAL_TITLE_GAP: f32 = 16.0;

/// The M3 "disabled content" opacity this substrate uses in place of
/// `ThemeData.disabledColor` (no such field exists yet) — see the module
/// docs' "State-color cascade" section.
const DISABLED_CONTENT_OPACITY: f32 = 0.38;

/// `_RenderListTile._defaultTileHeight`'s one/two/three-line ×
/// dense/not-dense table (`list_tile.dart` `:1503-1510`, oracle tag
/// `3.44.0`) — a flat literal table, NOT `_LisTileDefaultsM3` and NOT
/// arithmetically derived from the non-dense values.
fn default_tile_height(is_three_line: bool, has_subtitle: bool, is_dense: bool) -> f32 {
    match (is_three_line, has_subtitle, is_dense) {
        (true, _, true) => 76.0,
        (true, _, false) => 88.0,
        (false, true, true) => 64.0,
        (false, true, false) => 72.0,
        (false, false, true) => 48.0,
        (false, false, false) => 56.0,
    }
}

/// A single-row Material Design list entry: leading/title/subtitle/trailing
/// slots, an optional whole-tile tap target, and `enabled`/`selected` states.
/// See the module docs for the M3 default token table and this substrate's
/// composition-vs-`_RenderListTile` scope.
///
/// ```rust
/// use flui_material::ListTile;
/// use flui_widgets::{Icon, IconData, Text};
///
/// let _tile = ListTile::new()
///     .leading(Icon::new(IconData::new(0xE87D)))
///     .title(Text::new("Inbox"))
///     .subtitle(Text::new("12 unread messages"))
///     .on_tap(|| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct ListTile {
    leading: Option<BoxedView>,
    title: Option<BoxedView>,
    subtitle: Option<BoxedView>,
    trailing: Option<BoxedView>,
    is_three_line: Option<bool>,
    dense: Option<bool>,
    shape: Option<MaterialShape>,
    selected_color: Option<Color>,
    icon_color: Option<Color>,
    text_color: Option<Color>,
    title_text_style: Option<TextStyle>,
    subtitle_text_style: Option<TextStyle>,
    leading_and_trailing_text_style: Option<TextStyle>,
    content_padding: Option<EdgeInsets>,
    enabled: bool,
    on_tap: Option<Rc<dyn Fn()>>,
    selected: bool,
    tile_color: Option<Color>,
    selected_tile_color: Option<Color>,
    horizontal_title_gap: Option<f32>,
    min_vertical_padding: Option<f32>,
    min_leading_width: Option<f32>,
    min_tile_height: Option<f32>,
}

impl Default for ListTile {
    fn default() -> Self {
        Self {
            leading: None,
            title: None,
            subtitle: None,
            trailing: None,
            is_three_line: None,
            dense: None,
            shape: None,
            selected_color: None,
            icon_color: None,
            text_color: None,
            title_text_style: None,
            subtitle_text_style: None,
            leading_and_trailing_text_style: None,
            content_padding: None,
            enabled: true,
            on_tap: None,
            selected: false,
            tile_color: None,
            selected_tile_color: None,
            horizontal_title_gap: None,
            min_vertical_padding: None,
            min_leading_width: None,
            min_tile_height: None,
        }
    }
}

impl std::fmt::Debug for ListTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListTile")
            .field("has_leading", &self.leading.is_some())
            .field("has_title", &self.title.is_some())
            .field("has_subtitle", &self.subtitle.is_some())
            .field("has_trailing", &self.trailing.is_some())
            .field("is_three_line", &self.is_three_line)
            .field("enabled", &self.enabled)
            .field("selected", &self.selected)
            .field("interactive", &self.on_tap.is_some())
            .finish_non_exhaustive()
    }
}

impl ListTile {
    /// An empty `ListTile` — `enabled: true`, every other property falling
    /// through to its M3 default or theme override.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the leading slot (typically an icon or avatar).
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(BoxedView(Box::new(leading.into_view())));
        self
    }

    /// Sets the title slot (typically `Text`).
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.title = Some(BoxedView(Box::new(title.into_view())));
        self
    }

    /// Sets the subtitle slot (typically `Text`).
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl IntoView) -> Self {
        self.subtitle = Some(BoxedView(Box::new(subtitle.into_view())));
        self
    }

    /// Sets the trailing slot (typically an icon or metadata).
    #[must_use]
    pub fn trailing(mut self, trailing: impl IntoView) -> Self {
        self.trailing = Some(BoxedView(Box::new(trailing.into_view())));
        self
    }

    /// Marks this tile as displaying three lines of text. Flutter parity: an
    /// explicit flag, never a subtitle-line-count heuristic — see the module
    /// docs. Only meaningful with [`ListTile::subtitle`] set.
    ///
    /// Stored as `Option<bool>` internally (this setter still takes a plain
    /// `bool`) so an explicit `false` here can still be overridden by a
    /// theme-level `Some(true)` — mirroring [`ListTile::dense`]'s identical
    /// widget → theme → default cascade rather than special-casing this one
    /// field to always win.
    #[must_use]
    pub fn is_three_line(mut self, is_three_line: bool) -> Self {
        self.is_three_line = Some(is_three_line);
        self
    }

    /// Switches between the dense and non-dense tile-height tables — see
    /// `default_tile_height`.
    #[must_use]
    pub fn dense(mut self, dense: bool) -> Self {
        self.dense = Some(dense);
        self
    }

    /// Overrides the tile's shape (its `InkWell` clip and background fill).
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Overrides the icon/text color used when [`ListTile::selected`] is
    /// `true`. Defaults to `ColorScheme.primary`.
    #[must_use]
    pub fn selected_color(mut self, color: Color) -> Self {
        self.selected_color = Some(color);
        self
    }

    /// Overrides the default `leading`/`trailing` icon color. Defaults to
    /// `ColorScheme.onSurfaceVariant`.
    #[must_use]
    pub fn icon_color(mut self, color: Color) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Overrides the default `title`/`subtitle`/`leading`/`trailing` text
    /// color. `None` (the default) leaves each slot's own baked-in default
    /// color untouched.
    #[must_use]
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Overrides the title's text style verbatim.
    #[must_use]
    pub fn title_text_style(mut self, style: TextStyle) -> Self {
        self.title_text_style = Some(style);
        self
    }

    /// Overrides the subtitle's text style verbatim.
    #[must_use]
    pub fn subtitle_text_style(mut self, style: TextStyle) -> Self {
        self.subtitle_text_style = Some(style);
        self
    }

    /// Overrides the leading/trailing slots' text style verbatim.
    #[must_use]
    pub fn leading_and_trailing_text_style(mut self, style: TextStyle) -> Self {
        self.leading_and_trailing_text_style = Some(style);
        self
    }

    /// Overrides the tile's internal content padding. Defaults to
    /// `left: 16.0, right: 24.0`.
    #[must_use]
    pub fn content_padding(mut self, padding: EdgeInsets) -> Self {
        self.content_padding = Some(padding);
        self
    }

    /// Whether this tile responds to interaction. When `false`, `on_tap` is
    /// inoperative and every color resolves through the disabled branch.
    /// Defaults to `true`.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the whole-tile tap handler, wired through an [`InkWell`].
    /// Inoperative while [`ListTile::enabled`] is `false`.
    #[must_use]
    pub fn on_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(callback));
        self
    }

    /// Marks this tile as selected — recolors icons/text to
    /// [`ListTile::selected_color`]. Defaults to `false`.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Overrides the background color used when [`ListTile::selected`] is
    /// `false`. Defaults to `Colors.transparent`.
    #[must_use]
    pub fn tile_color(mut self, color: Color) -> Self {
        self.tile_color = Some(color);
        self
    }

    /// Overrides the background color used when [`ListTile::selected`] is
    /// `true`. Defaults to `Colors.transparent`.
    #[must_use]
    pub fn selected_tile_color(mut self, color: Color) -> Self {
        self.selected_tile_color = Some(color);
        self
    }

    /// Overrides the gap between the leading/trailing slots and the title
    /// column. Defaults to `16.0`.
    #[must_use]
    pub fn horizontal_title_gap(mut self, gap: f32) -> Self {
        self.horizontal_title_gap = Some(gap);
        self
    }

    /// Overrides the minimum padding above/below the title/subtitle column.
    /// Defaults to `8.0`.
    #[must_use]
    pub fn min_vertical_padding(mut self, padding: f32) -> Self {
        self.min_vertical_padding = Some(padding);
        self
    }

    /// Overrides the minimum width reserved for [`ListTile::leading`].
    /// Defaults to `24.0`.
    #[must_use]
    pub fn min_leading_width(mut self, width: f32) -> Self {
        self.min_leading_width = Some(width);
        self
    }

    /// Overrides the tile's minimum height. `None` (the default) falls
    /// through to the one/two/three-line table — see `default_tile_height`.
    #[must_use]
    pub fn min_tile_height(mut self, height: f32) -> Self {
        self.min_tile_height = Some(height);
        self
    }

    /// Whether this tile responds to a whole-tile tap — Flutter parity:
    /// `enabled ? onTap : null` being non-null.
    fn is_interactive(&self) -> bool {
        self.enabled && self.on_tap.is_some()
    }
}

/// [`ListTile`]'s theme-resolved geometry/color/style — see [`resolve_style`]'s
/// doc comment for the widget → theme → default cascade.
struct ResolvedListTileStyle {
    tile_color: Color,
    icon_color: Color,
    shape: MaterialShape,
    title_style: TextStyle,
    subtitle_style: TextStyle,
    leading_and_trailing_style: TextStyle,
    content_padding: EdgeInsets,
    horizontal_title_gap: f32,
    min_vertical_padding: f32,
    min_leading_width: f32,
    tile_height: f32,
}

/// Resolves the `disabled > selected > enabled` color precedence Flutter's
/// `_IndividualOverrides.resolve` applies (`list_tile.dart` `:1217-1229`,
/// oracle tag `3.44.0`) — see the module docs' "State-color cascade" section
/// for why this is a direct `if`/`else if`/`else`, not a
/// `WidgetStateProperty`.
fn resolve_content_color(
    enabled: bool,
    selected: bool,
    selected_color: Option<Color>,
    enabled_color: Option<Color>,
    disabled_fallback: Color,
) -> Option<Color> {
    if !enabled {
        Some(disabled_fallback)
    } else if selected {
        selected_color
    } else {
        enabled_color
    }
}

/// Resolve `ListTile`'s M3 defaults through the widget → theme → default
/// cascade — see the module docs' token table and "State-color cascade"
/// section. Flutter parity: `ListTile.build`, `list_tile.dart`, oracle tag
/// `3.44.0`.
fn resolve_style(theme: &ThemeData, view: &ListTile) -> ResolvedListTileStyle {
    let tile_theme = theme.list_tile_theme.as_ref();
    let colors = theme.color_scheme;
    let disabled_content = colors.on_surface.with_opacity(DISABLED_CONTENT_OPACITY);

    let is_dense = view
        .dense
        .or_else(|| tile_theme.and_then(|t| t.dense))
        .unwrap_or(false);

    // The selected-color cascade is shared by icon and text resolution —
    // Flutter parity: both `resolveColor` calls in `ListTile.build` pass the
    // SAME `selectedColor` chain (`list_tile.dart` `:855-861`, `:878-884`).
    let selected_color = view
        .selected_color
        .or_else(|| tile_theme.and_then(|t| t.selected_color))
        .unwrap_or(colors.primary);

    let icon_color = resolve_content_color(
        view.enabled,
        view.selected,
        Some(selected_color),
        view.icon_color
            .or_else(|| tile_theme.and_then(|t| t.icon_color)),
        disabled_content,
    )
    .unwrap_or(colors.on_surface_variant);

    // Unlike `icon_color`, the enabled/not-selected branch stays `None` when
    // no override is set: each text style below already carries its own
    // baked-in M3 color (`onSurface`/`onSurfaceVariant`), and `None` means
    // "don't touch it" — see the module docs.
    let text_color = resolve_content_color(
        view.enabled,
        view.selected,
        Some(selected_color),
        view.text_color
            .or_else(|| tile_theme.and_then(|t| t.text_color)),
        disabled_content,
    );

    let title_style = view
        .title_text_style
        .clone()
        .or_else(|| tile_theme.and_then(|t| t.title_text_style.clone()))
        .unwrap_or_else(|| {
            theme
                .text_theme
                .body_large
                .clone()
                .unwrap_or_default()
                .with_color(colors.on_surface)
        });
    let title_style = match text_color {
        Some(color) => title_style.with_color(color),
        None => title_style,
    };
    // Flutter parity: `titleStyle.copyWith(fontSize: _isDenseLayout ? 13.0 :
    // null)` (`list_tile.dart` `:923-926`, oracle tag `3.44.0`) — Dart's
    // `copyWith(fontSize: null)` means "leave the current value alone", not
    // "clear it", so the non-dense branch intentionally does nothing here
    // rather than resetting `font_size` to `None`.
    let title_style = if is_dense {
        title_style.with_font_size(13.0)
    } else {
        title_style
    };

    let subtitle_style = view
        .subtitle_text_style
        .clone()
        .or_else(|| tile_theme.and_then(|t| t.subtitle_text_style.clone()))
        .unwrap_or_else(|| {
            theme
                .text_theme
                .body_medium
                .clone()
                .unwrap_or_default()
                .with_color(colors.on_surface_variant)
        });
    let subtitle_style = match text_color {
        Some(color) => subtitle_style.with_color(color),
        None => subtitle_style,
    };
    // Flutter parity: `subtitleStyle.copyWith(fontSize: _isDenseLayout ?
    // 12.0 : null)` (`list_tile.dart` `:939-942`, oracle tag `3.44.0`) — see
    // `title_style`'s identical clamp above for the `copyWith(fontSize:
    // null)` = "leave alone" semantics.
    let subtitle_style = if is_dense {
        subtitle_style.with_font_size(12.0)
    } else {
        subtitle_style
    };

    let leading_and_trailing_style = view
        .leading_and_trailing_text_style
        .clone()
        .or_else(|| tile_theme.and_then(|t| t.leading_and_trailing_text_style.clone()))
        .unwrap_or_else(|| {
            theme
                .text_theme
                .label_small
                .clone()
                .unwrap_or_default()
                .with_color(colors.on_surface_variant)
        });
    let leading_and_trailing_style = match text_color {
        Some(color) => leading_and_trailing_style.with_color(color),
        None => leading_and_trailing_style,
    };

    // Flutter parity: `backgroundColor`/`selectedBackgroundColor` both fall
    // through to the SAME `defaults.tileColor` (`Colors.transparent`,
    // `list_tile.dart` `:823-830`) — there is no separate "default selected
    // tile color" constant.
    let background_color = view
        .tile_color
        .or_else(|| tile_theme.and_then(|t| t.tile_color))
        .unwrap_or(Color::TRANSPARENT);
    let selected_background_color = view
        .selected_tile_color
        .or_else(|| tile_theme.and_then(|t| t.selected_tile_color))
        .unwrap_or(Color::TRANSPARENT);
    let tile_color = if view.selected {
        selected_background_color
    } else {
        background_color
    };

    let shape = view
        .shape
        .or_else(|| tile_theme.and_then(|t| t.shape))
        .unwrap_or_default();

    let content_padding = view
        .content_padding
        .or_else(|| tile_theme.and_then(|t| t.content_padding))
        .unwrap_or_else(|| {
            EdgeInsets::new(
                px(0.0),
                px(CONTENT_PADDING_END),
                px(0.0),
                px(CONTENT_PADDING_START),
            )
        });

    let horizontal_title_gap = view
        .horizontal_title_gap
        .or_else(|| tile_theme.and_then(|t| t.horizontal_title_gap))
        .unwrap_or(HORIZONTAL_TITLE_GAP);
    let min_vertical_padding = view
        .min_vertical_padding
        .or_else(|| tile_theme.and_then(|t| t.min_vertical_padding))
        .unwrap_or(MIN_VERTICAL_PADDING);
    let min_leading_width = view
        .min_leading_width
        .or_else(|| tile_theme.and_then(|t| t.min_leading_width))
        .unwrap_or(MIN_LEADING_WIDTH);

    // Flutter parity: `isThreeLine ?? tileTheme.isThreeLine ?? false`
    // (`list_tile.dart` `:1019-1023`, oracle tag `3.44.0`) — the same
    // widget → theme → default cascade `dense` uses just above, now that
    // `ListTile::is_three_line` is `Option<bool>` too: an explicit
    // widget-level `false` short-circuits the cascade and wins over a
    // theme-level `true`, exactly like the oracle's `??` chain.
    let is_three_line = view
        .is_three_line
        .or_else(|| tile_theme.and_then(|t| t.is_three_line))
        .unwrap_or(false);
    let tile_height = view
        .min_tile_height
        .or_else(|| tile_theme.and_then(|t| t.min_tile_height))
        .unwrap_or_else(|| default_tile_height(is_three_line, view.subtitle.is_some(), is_dense));

    ResolvedListTileStyle {
        tile_color,
        icon_color,
        shape,
        title_style,
        subtitle_style,
        leading_and_trailing_style,
        content_padding,
        horizontal_title_gap,
        min_vertical_padding,
        min_leading_width,
        tile_height,
    }
}

/// Composes the `leading`/title-column/`trailing` row — see the module docs'
/// "Scope" section for how this stands in for `_RenderListTile`.
fn build_content_row(view: &ListTile, resolved: &ResolvedListTileStyle) -> Row<Vec<BoxedView>> {
    let mut children: Vec<BoxedView> = Vec::new();

    if let Some(leading) = &view.leading {
        let leading_constraints = BoxConstraints::new(
            px(resolved.min_leading_width),
            Pixels::INFINITY,
            px(0.0),
            Pixels::INFINITY,
        );
        children.push(
            ConstrainedBox::new(leading_constraints)
                .child(DefaultTextStyle::new(
                    resolved.leading_and_trailing_style.clone(),
                    leading.clone(),
                ))
                .boxed(),
        );
        children.push(SizedBox::width(resolved.horizontal_title_gap).boxed());
    }

    let title_view: BoxedView = view
        .title
        .clone()
        .unwrap_or_else(|| flui_widgets::SizedBox::shrink().boxed());
    let mut column_children: Vec<BoxedView> =
        vec![DefaultTextStyle::new(resolved.title_style.clone(), title_view).boxed()];
    if let Some(subtitle) = &view.subtitle {
        column_children
            .push(DefaultTextStyle::new(resolved.subtitle_style.clone(), subtitle.clone()).boxed());
    }
    children.push(
        Expanded::new(
            Padding::new(EdgeInsets::symmetric(
                px(resolved.min_vertical_padding),
                px(0.0),
            ))
            .child(
                Column::new(column_children)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min),
            ),
        )
        .boxed(),
    );

    if let Some(trailing) = &view.trailing {
        children.push(SizedBox::width(resolved.horizontal_title_gap).boxed());
        children.push(
            DefaultTextStyle::new(
                resolved.leading_and_trailing_style.clone(),
                trailing.clone(),
            )
            .boxed(),
        );
    }

    Row::new(children).cross_axis_alignment(CrossAxisAlignment::Center)
}

impl StatelessView for ListTile {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        debug_assert!(
            self.is_three_line != Some(true) || self.subtitle.is_some(),
            "ListTile::is_three_line(true) requires ListTile::subtitle to be set — \
             Flutter parity: `assert(isThreeLine != true || subtitle != null)` \
             (list_tile.dart, oracle tag 3.44.0)"
        );

        let theme = Theme::of(ctx);
        let resolved = resolve_style(&theme, self);

        // Flutter parity: `IconTheme.merge(data: iconThemeData, child: ...)`
        // (`list_tile.dart` `:1008-1009`) — a MERGE over the ambient theme,
        // not a replacement: only `color` is overridden, every other field
        // (`size`, `opacity`, …) passes the enclosing `IconTheme::of`
        // through unchanged. `IconThemeData { color: ..,
        // ..IconThemeData::default() }` would instead blank every other
        // field back to `None`, discarding an ambient `size` override an
        // app-level `IconTheme` set above this tile.
        let ambient_icon_theme = IconTheme::of(ctx);
        let content = SafeArea::new()
            .top(false)
            .bottom(false)
            .minimum(resolved.content_padding)
            .child(IconTheme::new(
                IconThemeData {
                    color: Some(resolved.icon_color),
                    ..ambient_icon_theme
                },
                build_content_row(self, &resolved),
            ));

        let semantics = Semantics::new()
            .button(self.on_tap.is_some())
            .selected(self.selected)
            .enabled(self.enabled)
            .child(content);

        let mut ink_well = InkWell::new(semantics).shape(resolved.shape);
        if self.is_interactive() {
            let on_tap = self
                .on_tap
                .clone()
                .expect("BUG: is_interactive() checked on_tap.is_some()");
            ink_well = ink_well.on_tap(move || on_tap());
        }

        let tile_constraints = BoxConstraints::new(
            Pixels::ZERO,
            Pixels::INFINITY,
            px(resolved.tile_height),
            Pixels::INFINITY,
        );

        ConstrainedBox::new(tile_constraints).child(
            Material::new(resolved.tile_color)
                .shape(resolved.shape)
                .child(ink_well),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_data::ListTileThemeData;

    #[test]
    fn new_leaves_every_override_unset_and_defaults_enabled() {
        let tile = ListTile::new();
        assert!(tile.leading.is_none());
        assert!(tile.title.is_none());
        assert!(tile.subtitle.is_none());
        assert!(tile.trailing.is_none());
        assert!(tile.is_three_line.is_none());
        assert!(tile.dense.is_none());
        assert!(tile.enabled);
        assert!(!tile.selected);
        assert!(tile.on_tap.is_none());
    }

    #[test]
    fn is_interactive_requires_both_enabled_and_on_tap() {
        assert!(!ListTile::new().is_interactive());
        assert!(
            !ListTile::new()
                .enabled(false)
                .on_tap(|| {})
                .is_interactive()
        );
        assert!(ListTile::new().on_tap(|| {}).is_interactive());
    }

    /// `_RenderListTile._defaultTileHeight`'s literal table (`list_tile.dart`
    /// `:1503-1510`, oracle tag `3.44.0`) — the mutation-honest pin for every
    /// branch, including the non-arithmetic three-line-dense value (`76.0`,
    /// not `88.0 - 12.0`).
    #[test]
    fn default_tile_height_matches_the_oracle_table() {
        assert_eq!(default_tile_height(false, false, false), 56.0);
        assert_eq!(default_tile_height(false, false, true), 48.0);
        assert_eq!(default_tile_height(false, true, false), 72.0);
        assert_eq!(default_tile_height(false, true, true), 64.0);
        assert_eq!(default_tile_height(true, true, false), 88.0);
        assert_eq!(default_tile_height(true, true, true), 76.0);
    }

    /// `_LisTileDefaultsM3`'s literal token table (`list_tile.dart`, oracle
    /// tag `3.44.0`). `min_leading_width` is `24.0`, not M2's `40.0`.
    #[test]
    fn default_constants_match_the_oracle() {
        assert_eq!(CONTENT_PADDING_START, 16.0);
        assert_eq!(CONTENT_PADDING_END, 24.0);
        assert_eq!(MIN_LEADING_WIDTH, 24.0);
        assert_eq!(MIN_VERTICAL_PADDING, 8.0);
        assert_eq!(HORIZONTAL_TITLE_GAP, 16.0);
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_token_table() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let tile = ListTile::new();
        let resolved = resolve_style(&theme, &tile);

        assert_eq!(resolved.tile_color, Color::TRANSPARENT);
        assert_eq!(resolved.icon_color, colors.on_surface_variant);
        assert_eq!(resolved.shape, MaterialShape::default());
        assert_eq!(resolved.title_style.color, Some(colors.on_surface));
        assert_eq!(
            resolved.subtitle_style.color,
            Some(colors.on_surface_variant)
        );
        assert_eq!(
            resolved.content_padding,
            EdgeInsets::new(
                px(0.0),
                px(CONTENT_PADDING_END),
                px(0.0),
                px(CONTENT_PADDING_START)
            )
        );
        assert_eq!(resolved.horizontal_title_gap, HORIZONTAL_TITLE_GAP);
        assert_eq!(resolved.min_vertical_padding, MIN_VERTICAL_PADDING);
        assert_eq!(resolved.min_leading_width, MIN_LEADING_WIDTH);
        assert_eq!(resolved.tile_height, 56.0);
    }

    #[test]
    fn resolve_style_two_line_tile_uses_the_two_line_height() {
        let theme = ThemeData::light();
        let tile = ListTile::new().subtitle(flui_widgets::SizedBox::shrink());
        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.tile_height, 72.0);
    }

    #[test]
    fn resolve_style_three_line_tile_uses_the_three_line_height() {
        let theme = ThemeData::light();
        let tile = ListTile::new()
            .subtitle(flui_widgets::SizedBox::shrink())
            .is_three_line(true);
        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.tile_height, 88.0);
    }

    /// A theme-level `is_three_line: Some(true)` reaches the resolved tile
    /// height when the widget itself leaves `is_three_line` unset — Flutter
    /// parity: `isThreeLine ?? tileTheme.isThreeLine ?? false`
    /// (`list_tile.dart` `:1019-1023`, oracle tag `3.44.0`). This is the
    /// cascade tier that a plain `bool` field (rather than `Option<bool>`)
    /// could never reach at all.
    #[test]
    fn resolve_style_theme_level_is_three_line_reaches_the_tile_height_when_widget_leaves_it_unset()
    {
        let mut theme = ThemeData::light();
        theme.list_tile_theme = Some(ListTileThemeData {
            is_three_line: Some(true),
            ..Default::default()
        });
        let tile = ListTile::new().subtitle(flui_widgets::SizedBox::shrink());

        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.tile_height, 88.0);
    }

    /// Mutation-honest combined-tier pin: an explicit widget-level
    /// `is_three_line(false)` must WIN over a theme-level `Some(true)` —
    /// the exact case a plain `bool` widget field couldn't represent (no way
    /// to distinguish "unset" from "explicitly false"), and the reason this
    /// field is `Option<bool>` like `dense`, not `bool`.
    #[test]
    fn resolve_style_widget_level_is_three_line_false_overrides_a_theme_level_true() {
        let mut theme = ThemeData::light();
        theme.list_tile_theme = Some(ListTileThemeData {
            is_three_line: Some(true),
            ..Default::default()
        });
        let tile = ListTile::new()
            .subtitle(flui_widgets::SizedBox::shrink())
            .is_three_line(false);

        let resolved = resolve_style(&theme, &tile);
        assert_eq!(
            resolved.tile_height, 72.0,
            "an explicit widget-level is_three_line(false) must override the theme's \
             is_three_line: Some(true), matching the oracle's `??` short-circuit"
        );
    }

    #[test]
    fn resolve_style_dense_two_line_tile_uses_the_dense_height() {
        let theme = ThemeData::light();
        let tile = ListTile::new()
            .subtitle(flui_widgets::SizedBox::shrink())
            .dense(true);
        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.tile_height, 64.0);
    }

    /// `dense` clamps title to `13.0` and subtitle to `12.0` — Flutter
    /// parity: `titleStyle.copyWith(fontSize: _isDenseLayout ? 13.0 : null)`
    /// and the subtitle equivalent (`list_tile.dart` `:923-926`/`:939-942`,
    /// oracle tag `3.44.0`). Mutation-honest: `13.0`/`12.0` are distinct
    /// literals, so a resolver that swapped them or clamped only one slot
    /// would fail this.
    #[test]
    fn resolve_style_dense_clamps_title_and_subtitle_font_size() {
        let theme = ThemeData::light();
        let tile = ListTile::new()
            .subtitle(flui_widgets::SizedBox::shrink())
            .dense(true);
        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.title_style.font_size, Some(13.0));
        assert_eq!(resolved.subtitle_style.font_size, Some(12.0));
    }

    /// Non-dense leaves each style's own baked-in M3 type-scale size alone
    /// (`bodyLarge`: `16.0`, `bodyMedium`: `14.0`) — Dart's
    /// `copyWith(fontSize: null)` means "unchanged", not "clear to null", so
    /// the non-dense branch must NOT reset `font_size` to `None`.
    #[test]
    fn resolve_style_non_dense_leaves_the_type_scale_font_size_untouched() {
        let theme = ThemeData::light();
        let tile = ListTile::new().subtitle(flui_widgets::SizedBox::shrink());
        let resolved = resolve_style(&theme, &tile);
        assert_eq!(resolved.title_style.font_size, Some(16.0));
        assert_eq!(resolved.subtitle_style.font_size, Some(14.0));
    }

    /// Selected recolors both icon and title text to `ColorScheme.primary` —
    /// `_LisTileDefaultsM3.selectedColor` (`list_tile.dart`, oracle tag
    /// `3.44.0`).
    #[test]
    fn resolve_style_selected_uses_the_primary_color() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let tile = ListTile::new().selected(true);
        let resolved = resolve_style(&theme, &tile);

        assert_eq!(resolved.icon_color, colors.primary);
        assert_eq!(resolved.title_style.color, Some(colors.primary));
    }

    /// Mutation-honest combined-state pin: `disabled` must win over
    /// `selected` — Flutter parity: `_IndividualOverrides.resolve` checks
    /// `WidgetState.disabled` BEFORE `WidgetState.selected` (`list_tile.dart`
    /// `:1222-1227`, oracle tag `3.44.0`). A resolver that checked `selected`
    /// first would report `primary`, not the disabled color, here.
    #[test]
    fn resolve_style_disabled_wins_over_selected() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let tile = ListTile::new().selected(true).enabled(false);
        let resolved = resolve_style(&theme, &tile);

        let expected = colors.on_surface.with_opacity(DISABLED_CONTENT_OPACITY);
        assert_eq!(resolved.icon_color, expected);
        assert_eq!(resolved.title_style.color, Some(expected));
    }

    #[test]
    fn resolve_style_falls_through_to_the_list_tile_theme_when_no_widget_override_is_set() {
        let mut theme = ThemeData::light();
        let themed_icon_color = Color::rgb(1, 2, 3);
        theme.list_tile_theme = Some(ListTileThemeData {
            icon_color: Some(themed_icon_color),
            min_leading_width: Some(30.0),
            ..Default::default()
        });

        let resolved = resolve_style(&theme, &ListTile::new());

        assert_eq!(resolved.icon_color, themed_icon_color);
        assert_eq!(resolved.min_leading_width, 30.0);
        // `horizontal_title_gap` was left unset on the theme slot — it falls
        // through to its own default independently.
        assert_eq!(resolved.horizontal_title_gap, HORIZONTAL_TITLE_GAP);
    }

    #[test]
    fn resolve_style_widget_override_wins_over_the_list_tile_theme() {
        let mut theme = ThemeData::light();
        theme.list_tile_theme = Some(ListTileThemeData {
            icon_color: Some(Color::rgb(1, 1, 1)),
            ..Default::default()
        });
        let widget_color = Color::rgb(9, 9, 9);

        let tile = ListTile::new().icon_color(widget_color);
        let resolved = resolve_style(&theme, &tile);

        assert_eq!(resolved.icon_color, widget_color);
    }

    #[test]
    fn resolve_style_enabled_unselected_text_color_stays_unset_by_default() {
        // Unlike `icon_color`, an enabled/unselected tile with no override
        // leaves `text_color` at `None` — the title/subtitle styles keep
        // their own baked-in M3 color instead of being forced to a shared
        // constant. See the module docs' "State-color cascade" section.
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let resolved = resolve_style(&theme, &ListTile::new());
        assert_eq!(resolved.title_style.color, Some(colors.on_surface));
        assert_eq!(
            resolved.subtitle_style.color,
            Some(colors.on_surface_variant)
        );
    }
}
