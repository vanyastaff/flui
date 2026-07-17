//! [`NavigationBar`]/[`NavigationDestination`] — the M3 bottom navigation
//! bar: a persistent row of equal-width destinations with a pill-shaped
//! selection indicator.
//!
//! # Flutter parity
//!
//! `material/navigation_bar.dart`'s `NavigationBar`/`NavigationDestination`/
//! `NavigationIndicator` and `material/navigation_bar_theme.dart`'s
//! `NavigationBarThemeData` (oracle tag `3.44.0`).
//!
//! # V1 scope: a controlled, static-geometry component
//!
//! `NavigationBar` is a plain [`StatelessView`] here, exactly as in the
//! oracle (`NavigationBar extends StatelessWidget`) — `selected_index` is a
//! fully caller-controlled prop (Flutter parity: the widget itself holds no
//! selection state, `onDestinationSelected` is expected to drive a rebuild
//! with a new `selectedIndex`), so no `WidgetStatesController` needs to be
//! threaded down to carry `Selected` the way [`crate::Switch`]/[`crate::Checkbox`]/
//! [`crate::Radio`] share one with their own [`crate::InkWell`] — this
//! component computes `selected`/`enabled` as plain booleans at build time
//! and resolves every color/style from them directly. Each destination's own
//! `Hovered`/`Focused`/`Pressed`/`Disabled` still lives inside its private
//! `InkWell` element (unshared, persists across `NavigationBar` rebuilds the
//! same way any other stateful child element does), but never needs to
//! react outward: `_NavigationBarDefaultsM3`'s `iconTheme`/`labelTextStyle`
//! only branch on `disabled`/`selected` — never `hovered`/`focused`/`pressed`
//! — so icon/label color has nothing to recompute when those transient
//! states change.
//!
//! # Label behavior: `alwaysShow` only, the enum itself deferred
//!
//! The oracle's `NavigationDestinationLabelBehavior` has three variants
//! (`alwaysShow`/`alwaysHide`/`onlyShowSelected`); this V1 always behaves as
//! `alwaysShow` and does not expose the enum at all (neither on the widget
//! nor `NavigationBarThemeData`) — a half-wired enum that silently no-ops on
//! two of its three variants is worse than not shipping the choice yet.
//! `alwaysHide` needs no new geometry (an unconditionally-collapsed label
//! column), but `onlyShowSelected` needs the padding-driven position/fade
//! animation `_NavigationDestinationLayoutDelegate` drives from the
//! selection animation (`:1096-1131`) — this V1 has no animation substrate
//! for it (see the next section). Both are deferred together, named, rather
//! than partially wired.
//!
//! # Layout: `Column(MainAxisAlignment::Center)` replaces the animated delegate
//!
//! The oracle positions each destination's icon/label pair with
//! `_NavigationDestinationLayoutDelegate`, a `MultiChildLayoutDelegate` that
//! interpolates the icon's vertical offset by the selection animation
//! (`:1103-1109`). Because `alwaysShow` always evaluates that delegate at
//! `kAlwaysCompleteAnimation` (animation == 1.0, `_DestinationLayoutAnimationBuilder`'s
//! `alwaysShow` branch, `:967-968`) regardless of whether the destination is
//! actually selected, the delegate's own math reduces to: center the
//! icon+label block (no gap between them) within the full destination
//! height. A plain `Column` with `MainAxisAlignment::Center` computes
//! exactly that (no custom delegate needed) — verified against the oracle
//! formula: `yPositionOffset = halfHeight(iconSize) + halfHeight(labelSize)`,
//! i.e. the pair is centered as one block with zero inter-child gap, which
//! is `Column`'s own `MainAxisAlignment::Center` behavior for a
//! two-child, no-`Spacer` list.
//!
//! # Indicator: always laid out, snap-visible
//!
//! `NavigationIndicator`'s oracle scales in via a `Transform` (`:851-856`)
//! driven by the selection animation — a `Transform` never changes layout
//! size, so the 64×32 `Ink` box is *always* reserved in the `Stack` that
//! backs every destination's icon, whether or not the destination is
//! selected (this is what keeps a selected destination's icon vertically
//! aligned with its unselected siblings). This V1 reserves the identical
//! 64×32 `NAVIGATION_INDICATOR_WIDTH`×`NAVIGATION_INDICATOR_HEIGHT` box
//! for every destination unconditionally, painting it filled
//! ([`crate::material::Material`], `MaterialShape::Stadium`) only when
//! selected and fully transparent otherwise — a snap substitute for the
//! oracle's scale-in/fade animation (named deferral, not a layout
//! divergence: the reserved space is identical either way).
//!
//! # Overlay: one shared `overlay_color`, no default table
//!
//! The oracle sets no default `overlayColor` in `_NavigationBarDefaultsM3` —
//! `_IndicatorInkWell`'s `overlayColor: info.overlayColor ?? navigationBarTheme.overlayColor`
//! falls through to `InkResponse`'s own `Theme.of(context).hoverColor`-family
//! defaults when both are `null`, which [`crate::InkWell`] has no equivalent
//! fallback for yet (see that module's own "no hardcoded opacities" /
//! "`None` resolution = no overlay layer" policy). So absent a widget- or
//! theme-level [`NavigationBar::overlay_color`]/[`crate::NavigationBarThemeData::overlay_color`]
//! override, a destination's `InkWell` paints no overlay at all — matching
//! the oracle's own effective default, not merely this substrate's usual
//! gap.
//!
//! **Named simplification, not a shape divergence**: the oracle clips the
//! overlay to the *icon's* rect specifically (`_IndicatorInkWell.getRectCallback`,
//! `:637-644`, a `GlobalKey`-located `RenderBox` lookup) rather than the
//! whole destination cell. [`crate::InkWell`] has no per-position rect
//! callback (it always shape-clips its own bounds) — this V1 wraps the
//! *entire* destination's icon+label column in one `InkWell` instead,
//! painting over the full cell rather than a rect confined to the icon. This
//! is the same "whole tap target, not a sub-region" approximation
//! [`crate::Switch`]'s own module docs already establish for its thumb-splash
//! substitution.
//!
//! # Semantics: a direct `SemanticsRole` port
//!
//! `Semantics(role: SemanticsRole.tabBar, ...)`/`Semantics(role: SemanticsRole.tab,
//! selected: ...)` (`:293-296`, `:304-306`) port directly: FLUI's
//! `flui_semantics::SemanticsRole` already carries `Tab`/`TabBar` variants —
//! no substitution needed. This V1 flattens the oracle's two-widget-deep
//! per-destination wrapper (an outer `Semantics(role: tab, selected: ...)`
//! from `NavigationBar.build`, an inner `Semantics(enabled: ..., button:
//! true)` from `_NavigationBarDestinationSemantics`) into one
//! [`flui_widgets::Semantics`] node carrying every flag at once — the two
//! nodes only exist in the oracle because two different widgets each own
//! one; [`flui_widgets::Semantics`] can carry `role`/`selected`/`enabled`/
//! `button` on a single builder, so nothing is lost by not re-nesting.
//! [`flui_widgets::MergeSemantics`] still wraps it (Flutter parity:
//! `MergeSemantics`, `:303`), which is load-bearing if a destination's own
//! icon/label subtree ever contributes its own semantics nodes (Flutter
//! parity: folding the label `Text`'s node into the tab node rather than
//! leaving it a sibling).
//!
//! **Named deferral**: the oracle's extra "Tab N of M" accessibility label
//! (`_NavigationBarDestinationSemantics`'s non-web `Stack` branch, `:1013-1026`)
//! needs a `MaterialLocalizations.tabLabel`-equivalent localized string
//! table this crate does not have yet — not wired.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Selection/indicator/label animation** (`animationDuration`,
//!   `_SelectableAnimatedBuilder`, `NavigationIndicator`'s scale-in) — every
//!   transition snaps; see the sections above.
//! - **`NavigationDestinationLabelBehavior`** (`alwaysHide`/`onlyShowSelected`)
//!   — see the "Label behavior" section above.
//! - **`shadow_color`/`surface_tint_color`** (widget, theme, and default
//!   tiers) — [`crate::material::Material`] has no such parameters yet, the
//!   same gap every other `Material`-backed M3 component in this crate
//!   already has.
//! - **`indicator_shape`/`label_padding` overrides** — fixed at the M3
//!   defaults (`StadiumBorder`, `EdgeInsets.only(top: 4)`).
//! - **Destination `tooltip`** (long-press `Tooltip`) — no long-press
//!   gesture wired here.
//! - **`maintainBottomViewPadding`** — [`flui_widgets::SafeArea`] is used
//!   with its own defaults; this substrate exposes no override for it yet.

use std::rc::Rc;

use flui_types::Alignment;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_view::prelude::*;
use flui_widgets::{
    Column, Expanded, IconTheme, IconThemeData, MainAxisAlignment, MergeSemantics, Padding, Row,
    SafeArea, Semantics, SemanticsRole, SizedBox, Stack, Text, WidgetState, WidgetStateProperty,
    WidgetStates,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::state_color::resolve_state_color;
use crate::theme::Theme;
use crate::theme_data::NavigationBarThemeData;

/// The bar's default height. Flutter parity: `_NavigationBarDefaultsM3`'s
/// `super(height: 80.0, ...)` (`navigation_bar.dart`, oracle tag `3.44.0`).
pub const NAVIGATION_BAR_HEIGHT: f32 = 80.0;

/// The bar's default elevation. Flutter parity:
/// `_NavigationBarDefaultsM3`'s `super(elevation: 3.0, ...)`.
pub const NAVIGATION_BAR_ELEVATION: f32 = 3.0;

/// The selection indicator's width. Flutter parity: `_kIndicatorWidth`
/// (`navigation_bar.dart`, `64.0`).
const NAVIGATION_INDICATOR_WIDTH: f32 = 64.0;

/// The selection indicator's height. Flutter parity: `_kIndicatorHeight`
/// (`32.0`).
const NAVIGATION_INDICATOR_HEIGHT: f32 = 32.0;

/// Each destination's icon side length. Flutter parity:
/// `_NavigationBarDefaultsM3.iconTheme`'s `size: 24.0`.
const NAVIGATION_DESTINATION_ICON_SIZE: f32 = 24.0;

/// The label's top padding. Flutter parity:
/// `_NavigationBarDefaultsM3.labelPadding`, `EdgeInsets.only(top: 4)`.
const NAVIGATION_LABEL_PADDING_TOP: f32 = 4.0;

// Compile-time geometry invariant (not a runtime test — every side is
// `const`): the indicator must fit within the destination's icon area
// alongside the 24dp icon it backs.
const _: () = assert!(NAVIGATION_DESTINATION_ICON_SIZE <= NAVIGATION_INDICATOR_HEIGHT);

/// A destination-selected callback: the tapped destination's index.
/// `Rc`-based (owner-local, per ADR-0027) — matches [`InkWell::on_tap`]'s own
/// callback shape.
type DestinationSelectedCallback = Rc<dyn Fn(usize)>;

/// One destination (icon + label) in a [`NavigationBar`].
///
/// Flutter parity: `NavigationDestination` (`navigation_bar.dart`, oracle tag
/// `3.44.0`).
///
/// # Examples
///
/// ```rust
/// use flui_material::NavigationDestination;
/// use flui_widgets::Icon;
/// use flui_widgets::icon::IconData;
///
/// let _home = NavigationDestination::new(Icon::new(IconData::new(0xE88A)), "Home");
/// ```
#[derive(Clone)]
pub struct NavigationDestination {
    icon: BoxedView,
    selected_icon: Option<BoxedView>,
    label: String,
    enabled: bool,
}

impl NavigationDestination {
    /// A destination showing `icon` (used both selected and unselected) and
    /// `label` below it.
    pub fn new(icon: impl IntoView, label: impl Into<String>) -> Self {
        Self {
            icon: icon.into_view().boxed(),
            selected_icon: None,
            label: label.into(),
            enabled: true,
        }
    }

    /// Sets a distinct icon shown while this destination is selected.
    /// Defaults to the same icon used unselected.
    #[must_use]
    pub fn selected_icon(mut self, selected_icon: impl IntoView) -> Self {
        self.selected_icon = Some(selected_icon.into_view().boxed());
        self
    }

    /// Sets whether this destination responds to taps. Defaults to `true`.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl std::fmt::Debug for NavigationDestination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationDestination")
            .field("label", &self.label)
            .field("has_selected_icon", &self.selected_icon.is_some())
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// A Material 3 bottom navigation bar: a row of equal-width
/// [`NavigationDestination`]s with a pill-shaped selection indicator.
///
/// See the module docs for the full V1 scope and named deferrals.
///
/// # Examples
///
/// ```rust
/// use flui_material::{NavigationBar, NavigationDestination};
/// use flui_widgets::Icon;
/// use flui_widgets::icon::IconData;
///
/// let _bar = NavigationBar::new(vec![
///     NavigationDestination::new(Icon::new(IconData::new(0xE88A)), "Home"),
///     NavigationDestination::new(Icon::new(IconData::new(0xE7FD)), "Profile"),
/// ])
/// .selected_index(0)
/// .on_destination_selected(|index| {
///     let _ = index;
/// });
/// ```
#[derive(Clone, StatelessView)]
pub struct NavigationBar {
    destinations: Vec<NavigationDestination>,
    selected_index: usize,
    on_destination_selected: Option<DestinationSelectedCallback>,
    height: Option<f32>,
    background_color: Option<Color>,
    elevation: Option<f32>,
    indicator_color: Option<Color>,
    overlay_color: Option<WidgetStateProperty<Option<Color>>>,
}

impl NavigationBar {
    /// A bar over `destinations`, `selected_index: 0`, no overrides.
    ///
    /// Flutter parity: `NavigationBar`'s constructor asserts `destinations.length
    /// >= 2` and `0 <= selectedIndex < destinations.length` (`navigation_bar.dart`
    /// `:122-123`) — Dart `assert`s are stripped in release builds, so this
    /// is a debug-only contract check there, not a production-enforced
    /// invariant; `debug_assert!` mirrors that exactly.
    #[must_use]
    pub fn new(destinations: Vec<NavigationDestination>) -> Self {
        debug_assert!(
            destinations.len() >= 2,
            "NavigationBar requires at least two destinations"
        );
        Self {
            destinations,
            selected_index: 0,
            on_destination_selected: None,
            height: None,
            background_color: None,
            elevation: None,
            indicator_color: None,
            overlay_color: None,
        }
    }

    /// Sets which destination is currently selected.
    #[must_use]
    pub fn selected_index(mut self, selected_index: usize) -> Self {
        debug_assert!(
            selected_index < self.destinations.len(),
            "selected_index must be < destinations.len()"
        );
        self.selected_index = selected_index;
        self
    }

    /// Sets the callback fired with a destination's index when it is tapped
    /// (and [`NavigationDestination::enabled`]).
    #[must_use]
    pub fn on_destination_selected(mut self, callback: impl Fn(usize) + 'static) -> Self {
        self.on_destination_selected = Some(Rc::new(callback));
        self
    }

    /// Overrides the bar's height. Defaults to [`NAVIGATION_BAR_HEIGHT`].
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Overrides the bar's background color. Defaults to
    /// `ColorScheme.surfaceContainer`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the bar's elevation. Defaults to [`NAVIGATION_BAR_ELEVATION`].
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Overrides the selection indicator's fill color. Defaults to
    /// `ColorScheme.secondaryContainer`.
    #[must_use]
    pub fn indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    /// Overrides every destination's shared state-overlay color. `None` (the
    /// default, at every tier) means no overlay layer — see the module
    /// docs' "Overlay" section.
    #[must_use]
    pub fn overlay_color(mut self, overlay_color: WidgetStateProperty<Option<Color>>) -> Self {
        self.overlay_color = Some(overlay_color);
        self
    }
}

impl std::fmt::Debug for NavigationBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationBar")
            .field("destinations", &self.destinations.len())
            .field("selected_index", &self.selected_index)
            .field("is_interactive", &self.on_destination_selected.is_some())
            .finish_non_exhaustive()
    }
}

/// Resolves `height`/`elevation`/`background_color`/`indicator_color`
/// through the widget → theme → default cascade — Flutter parity:
/// `NavigationBar.build`'s `height ?? navigationBarTheme.height ?? defaults.height!`
/// family (`navigation_bar.dart` `:281-287`), extracted as its own pure
/// function per field so the tier precedence is unit-testable without
/// mounting a widget tree.
fn resolve_bar_geometry(
    view: &NavigationBar,
    theme: Option<&NavigationBarThemeData>,
    colors: &ColorScheme,
) -> (f32, f32, Color, Color) {
    let height = view
        .height
        .or(theme.and_then(|t| t.height))
        .unwrap_or(NAVIGATION_BAR_HEIGHT);
    let elevation = view
        .elevation
        .or(theme.and_then(|t| t.elevation))
        .unwrap_or(NAVIGATION_BAR_ELEVATION);
    let background_color = view
        .background_color
        .or(theme.and_then(|t| t.background_color))
        .unwrap_or(colors.surface_container);
    let indicator_color = view
        .indicator_color
        .or(theme.and_then(|t| t.indicator_color))
        .unwrap_or(colors.secondary_container);
    (height, elevation, background_color, indicator_color)
}

/// `_NavigationBarDefaultsM3.iconTheme` (`navigation_bar.dart`, oracle tag
/// `3.44.0`) — branches on `disabled`/`selected` only, never
/// `hovered`/`focused`/`pressed` (see the module docs).
fn navigation_destination_default_icon_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        colors.on_surface_variant.with_opacity(0.38)
    } else if states.contains_state(WidgetState::Selected) {
        colors.on_secondary_container
    } else {
        colors.on_surface_variant
    }
}

/// Resolves a destination's icon color through the theme → default cascade
/// (there is no widget-level icon-color override — Flutter parity: neither
/// `NavigationBar` nor `NavigationDestination` exposes one either).
fn resolve_navigation_destination_icon_color(
    theme_icon_color: Option<&WidgetStateProperty<Option<Color>>>,
    colors: &ColorScheme,
    states: WidgetStates,
) -> Color {
    resolve_state_color(theme_icon_color, &states)
        .unwrap_or_else(|| navigation_destination_default_icon_color(colors, states))
}

/// `_NavigationBarDefaultsM3.labelTextStyle` (`navigation_bar.dart`, oracle
/// tag `3.44.0`): `TextTheme.labelMedium` recolored per state — same
/// `disabled`/`selected`-only branch as the icon color.
fn navigation_destination_default_label_style(
    base: &TextStyle,
    colors: &ColorScheme,
    states: WidgetStates,
) -> TextStyle {
    let color = if states.contains_state(WidgetState::Disabled) {
        colors.on_surface_variant.with_opacity(0.38)
    } else if states.contains_state(WidgetState::Selected) {
        colors.on_surface
    } else {
        colors.on_surface_variant
    };
    base.clone().with_color(color)
}

/// Resolves a destination's label text style through the theme → default
/// cascade (no widget-level override, matching [`resolve_navigation_destination_icon_color`]).
fn resolve_navigation_destination_label_style(
    theme_label_style: Option<&WidgetStateProperty<Option<TextStyle>>>,
    base: &TextStyle,
    colors: &ColorScheme,
    states: WidgetStates,
) -> TextStyle {
    theme_label_style
        .and_then(|property| property.resolve(&states))
        .unwrap_or_else(|| navigation_destination_default_label_style(base, colors, states))
}

/// Builds the PURE (never-combined) `WidgetStates` query set a destination's
/// icon/label color resolves against.
///
/// Flutter parity: `NavigationDestination.build` resolves its icon/label
/// themes with three INDEPENDENT constant sets — `selectedState =
/// {selected}`, `unselectedState = {}`, `disabledState = {disabled}`
/// (`navigation_bar.dart:427-429`) — never a set containing both `selected`
/// AND `disabled` together, then picks ONE of the three resolved values via
/// a plain `enabled ? (selected-branch) : disabledIconTheme` bool check
/// (`:450-457`, `:498-502`). A combined `{selected, disabled}` query would
/// let a theme [`WidgetStateProperty::Map`]
/// ordered `[Is(Selected), Is(Disabled), Any]` resolve the SELECTED entry
/// for a disabled destination (first-match-wins still matches `Is(Selected)`
/// against the combined set), instead of the disabled entry the oracle's
/// pure-set query guarantees — see
/// `theme_disabled_and_selected_resolves_the_disabled_entry_not_the_selected_one`.
fn navigation_destination_states(selected: bool, enabled: bool) -> WidgetStates {
    if !enabled {
        WidgetStates::from(WidgetState::Disabled)
    } else if selected {
        WidgetStates::from(WidgetState::Selected)
    } else {
        WidgetStates::NONE
    }
}

/// Builds one destination's `Expanded(MergeSemantics(Semantics(InkWell(...))))`
/// subtree — see the module docs for the layout/overlay/semantics shape.
#[allow(clippy::too_many_arguments, reason = "internal helper, not public API")]
fn build_destination(
    colors: &ColorScheme,
    indicator_color: Color,
    icon_color_theme: Option<&WidgetStateProperty<Option<Color>>>,
    label_style_theme: Option<&WidgetStateProperty<Option<TextStyle>>>,
    label_base: &TextStyle,
    overlay_color: &WidgetStateProperty<Option<Color>>,
    index: usize,
    selected: bool,
    destination: &NavigationDestination,
    on_destination_selected: Option<&DestinationSelectedCallback>,
) -> Expanded {
    let enabled = destination.enabled;
    let states = navigation_destination_states(selected, enabled);

    let icon_color = resolve_navigation_destination_icon_color(icon_color_theme, colors, states);
    let label_style =
        resolve_navigation_destination_label_style(label_style_theme, label_base, colors, states);

    let icon_child = if selected {
        destination
            .selected_icon
            .clone()
            .unwrap_or_else(|| destination.icon.clone())
    } else {
        destination.icon.clone()
    };
    let icon_themed = IconTheme::new(
        IconThemeData {
            size: Some(NAVIGATION_DESTINATION_ICON_SIZE),
            color: Some(icon_color),
            ..IconThemeData::default()
        },
        icon_child,
    );

    // Always reserved at full size, painted transparent when unselected —
    // see the module docs' "Indicator" section.
    let indicator_fill = if selected {
        indicator_color
    } else {
        Color::TRANSPARENT
    };
    let indicator = SizedBox::new(NAVIGATION_INDICATOR_WIDTH, NAVIGATION_INDICATOR_HEIGHT)
        .child(Material::new(indicator_fill).shape(MaterialShape::Stadium));

    let icon_stack =
        Stack::new(vec![indicator.boxed(), icon_themed.boxed()]).alignment(Alignment::CENTER);

    let label = Padding::only(0.0, NAVIGATION_LABEL_PADDING_TOP, 0.0, 0.0)
        .child(Text::new(destination.label.clone()).style(label_style));

    let column = Column::new(vec![icon_stack.boxed(), label.boxed()])
        .main_axis_alignment(MainAxisAlignment::Center);

    // Flutter parity: `NavigationBar._handleTap` always returns a real
    // `VoidCallback` — the real callback when `onDestinationSelected` is
    // set, a no-op closure `() {}` otherwise (`navigation_bar.dart:272-274`)
    // — and `_IndicatorInkWell.onTap` is `enabled ? info.onTap : null`
    // (`:606`), never gated on whether a callback was actually supplied. So
    // `InkResponse.enabled` (`isWidgetEnabled`, any tap-family callback
    // non-null) is `true` for an enabled destination even with no
    // `on_destination_selected` at all — it still paints its hover/press
    // overlay. Wiring `on_tap` only when a callback is present would leave
    // a callback-less-but-enabled destination reading as disabled to
    // `InkWell`, silently dropping its overlay.
    let mut ink_well = InkWell::new(column).overlay_color(overlay_color.clone());
    if enabled {
        let callback = on_destination_selected.cloned();
        ink_well = ink_well.on_tap(move || {
            if let Some(callback) = &callback {
                callback(index);
            }
        });
    }

    let semantics = Semantics::new()
        .role(SemanticsRole::Tab)
        .selected(selected)
        .enabled(enabled)
        .button(true)
        .child(ink_well);

    Expanded::new(MergeSemantics::new().child(semantics))
}

impl StatelessView for NavigationBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        let nav_theme = theme.navigation_bar_theme.as_ref();

        let (height, elevation, background_color, indicator_color) =
            resolve_bar_geometry(self, nav_theme, &colors);

        let overlay_color = self
            .overlay_color
            .clone()
            .or_else(|| nav_theme.and_then(|t| t.overlay_color.clone()))
            .unwrap_or_else(|| WidgetStateProperty::all(None));
        let icon_color_theme = nav_theme.and_then(|t| t.icon_color.as_ref());
        let label_style_theme = nav_theme.and_then(|t| t.label_text_style.as_ref());
        let label_base = theme.text_theme.label_medium.clone().unwrap_or_default();

        let children: Vec<BoxedView> = self
            .destinations
            .iter()
            .enumerate()
            .map(|(index, destination)| {
                build_destination(
                    &colors,
                    indicator_color,
                    icon_color_theme,
                    label_style_theme,
                    &label_base,
                    &overlay_color,
                    index,
                    index == self.selected_index,
                    destination,
                    self.on_destination_selected.as_ref(),
                )
                .boxed()
            })
            .collect();

        let content = Semantics::new()
            .role(SemanticsRole::TabBar)
            .container(true)
            .explicit_child_nodes(true)
            .child(SizedBox::height(height).child(Row::new(children)));

        Material::new(background_color)
            .elevation(elevation)
            .child(SafeArea::new().child(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light() -> ColorScheme {
        ColorScheme::light()
    }

    fn icon() -> flui_widgets::Icon {
        flui_widgets::Icon::new(flui_widgets::icon::IconData::new(0xE88A))
    }

    // ------------------------------------------------------------------
    // Construction / builder surface
    // ------------------------------------------------------------------

    #[test]
    fn new_leaves_every_override_unset_and_selects_the_first_destination() {
        let bar = NavigationBar::new(vec![
            NavigationDestination::new(icon(), "Home"),
            NavigationDestination::new(icon(), "Profile"),
        ]);
        assert_eq!(bar.selected_index, 0);
        assert!(bar.height.is_none());
        assert!(bar.background_color.is_none());
        assert!(bar.elevation.is_none());
        assert!(bar.indicator_color.is_none());
        assert!(bar.overlay_color.is_none());
        assert!(bar.on_destination_selected.is_none());
    }

    #[test]
    fn selected_index_builder_overrides_the_default() {
        let bar = NavigationBar::new(vec![
            NavigationDestination::new(icon(), "Home"),
            NavigationDestination::new(icon(), "Profile"),
        ])
        .selected_index(1);
        assert_eq!(bar.selected_index, 1);
    }

    #[test]
    fn on_destination_selected_makes_the_bar_interactive() {
        let bar = NavigationBar::new(vec![
            NavigationDestination::new(icon(), "Home"),
            NavigationDestination::new(icon(), "Profile"),
        ])
        .on_destination_selected(|_| {});
        assert!(bar.on_destination_selected.is_some());
    }

    #[test]
    fn new_destination_defaults_to_enabled_with_no_selected_icon() {
        let destination = NavigationDestination::new(icon(), "Home");
        assert!(destination.enabled);
        assert!(destination.selected_icon.is_none());
        assert_eq!(destination.label, "Home");
    }

    #[test]
    fn destination_enabled_builder_overrides_the_default() {
        let destination = NavigationDestination::new(icon(), "Home").enabled(false);
        assert!(!destination.enabled);
    }

    // ------------------------------------------------------------------
    // resolve_bar_geometry — widget > theme > default cascade, mutation-run:
    // each probe below was verified to fail against a deliberately broken
    // cascade (a tier short-circuited, or the wrong `ColorScheme` field
    // read) before being confirmed against the real implementation.
    // ------------------------------------------------------------------

    fn bar() -> NavigationBar {
        NavigationBar::new(vec![
            NavigationDestination::new(icon(), "Home"),
            NavigationDestination::new(icon(), "Profile"),
        ])
    }

    #[test]
    fn geometry_defaults_match_the_m3_token_table() {
        let (height, elevation, background_color, indicator_color) =
            resolve_bar_geometry(&bar(), None, &light());
        assert_eq!(height, NAVIGATION_BAR_HEIGHT);
        assert_eq!(elevation, NAVIGATION_BAR_ELEVATION);
        assert_eq!(background_color, light().surface_container);
        assert_eq!(indicator_color, light().secondary_container);
    }

    #[test]
    fn theme_tier_beats_the_default_when_no_widget_override_is_set() {
        let theme = NavigationBarThemeData {
            height: Some(96.0),
            elevation: Some(1.0),
            background_color: Some(Color::rgb(9, 9, 9)),
            indicator_color: Some(Color::rgb(8, 8, 8)),
            ..Default::default()
        };
        let (height, elevation, background_color, indicator_color) =
            resolve_bar_geometry(&bar(), Some(&theme), &light());
        assert_eq!(height, 96.0);
        assert_eq!(elevation, 1.0);
        assert_eq!(background_color, Color::rgb(9, 9, 9));
        assert_eq!(indicator_color, Color::rgb(8, 8, 8));
    }

    #[test]
    fn widget_tier_wins_over_theme_and_default() {
        let theme = NavigationBarThemeData {
            height: Some(96.0),
            ..Default::default()
        };
        let widget = bar().height(120.0).indicator_color(Color::rgb(1, 2, 3));
        let (height, _, _, indicator_color) = resolve_bar_geometry(&widget, Some(&theme), &light());
        assert_eq!(height, 120.0);
        assert_eq!(indicator_color, Color::rgb(1, 2, 3));
    }

    // ------------------------------------------------------------------
    // Icon/label color state table — per-state probes, oracle branch order
    // ------------------------------------------------------------------

    #[test]
    fn default_icon_color_unselected_enabled_is_on_surface_variant() {
        assert_eq!(
            navigation_destination_default_icon_color(&light(), WidgetStates::NONE),
            light().on_surface_variant
        );
    }

    #[test]
    fn default_icon_color_selected_is_on_secondary_container() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            navigation_destination_default_icon_color(&light(), states),
            light().on_secondary_container
        );
    }

    #[test]
    fn default_icon_color_disabled_wins_over_selected() {
        // Branch-order pin: `disabled` is checked BEFORE `selected` in the
        // oracle (`_NavigationBarDefaultsM3.iconTheme`).
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        assert_eq!(
            navigation_destination_default_icon_color(&light(), states),
            light().on_surface_variant.with_opacity(0.38)
        );
    }

    #[test]
    fn default_icon_color_ignores_hover_focus_pressed() {
        // Named-parity pin: the M3 default table has no branch for these —
        // confirmed by asserting the plain-unselected default is unchanged
        // under each.
        let base = light().on_surface_variant;
        for extra in [
            WidgetState::Hovered,
            WidgetState::Focused,
            WidgetState::Pressed,
        ] {
            let states = WidgetStates::from(extra);
            assert_eq!(
                navigation_destination_default_icon_color(&light(), states),
                base
            );
        }
    }

    #[test]
    fn icon_color_theme_tier_beats_the_default() {
        let theme = WidgetStateProperty::all(Some(Color::rgb(7, 7, 7)));
        let resolved =
            resolve_navigation_destination_icon_color(Some(&theme), &light(), WidgetStates::NONE);
        assert_eq!(resolved, Color::rgb(7, 7, 7));
    }

    #[test]
    fn default_label_style_selected_is_on_surface() {
        let base = TextStyle::default();
        let states = WidgetStates::from(WidgetState::Selected);
        let style = navigation_destination_default_label_style(&base, &light(), states);
        assert_eq!(style.color, Some(light().on_surface));
    }

    #[test]
    fn default_label_style_unselected_enabled_is_on_surface_variant() {
        let base = TextStyle::default();
        let style = navigation_destination_default_label_style(&base, &light(), WidgetStates::NONE);
        assert_eq!(style.color, Some(light().on_surface_variant));
    }

    #[test]
    fn default_label_style_disabled_wins_over_selected() {
        let base = TextStyle::default();
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        let style = navigation_destination_default_label_style(&base, &light(), states);
        assert_eq!(
            style.color,
            Some(light().on_surface_variant.with_opacity(0.38))
        );
    }

    #[test]
    fn label_style_theme_tier_beats_the_default() {
        let theme_style = TextStyle::default().with_color(Color::rgb(3, 3, 3));
        let theme: WidgetStateProperty<Option<TextStyle>> =
            WidgetStateProperty::all(Some(theme_style.clone()));
        let resolved = resolve_navigation_destination_label_style(
            Some(&theme),
            &TextStyle::default(),
            &light(),
            WidgetStates::NONE,
        );
        assert_eq!(resolved.color, theme_style.color);
    }

    // ------------------------------------------------------------------
    // navigation_destination_states
    // ------------------------------------------------------------------

    #[test]
    fn states_selected_and_enabled_carries_only_selected() {
        let states = navigation_destination_states(true, true);
        assert!(states.contains_state(WidgetState::Selected));
        assert!(!states.contains_state(WidgetState::Disabled));
    }

    #[test]
    fn states_unselected_and_disabled_carries_only_disabled() {
        let states = navigation_destination_states(false, false);
        assert!(!states.contains_state(WidgetState::Selected));
        assert!(states.contains_state(WidgetState::Disabled));
    }

    /// Regression: a destination that is BOTH selected and disabled (e.g.
    /// the current tab of a now-locked section) must query with the PURE
    /// `{disabled}` set, never a combined `{selected, disabled}` one — see
    /// `navigation_destination_states`'s own doc comment for why the oracle
    /// never queries a combined set, and
    /// `theme_disabled_and_selected_resolves_the_disabled_entry_not_the_selected_one`
    /// for the end-to-end proof this enables.
    #[test]
    fn states_selected_and_disabled_carries_only_disabled_not_both() {
        let states = navigation_destination_states(true, false);
        assert!(states.contains_state(WidgetState::Disabled));
        assert!(
            !states.contains_state(WidgetState::Selected),
            "a disabled destination's query states must never also carry Selected, even when \
             the destination is selected — combining them would let a theme Map resolve the \
             wrong (selected) entry for a disabled destination",
        );
    }

    /// Mutation-run: reverting `navigation_destination_states` to its
    /// pre-fix shape (`Selected` and `Disabled` combined into one query set
    /// whenever both apply) was confirmed to make this test fail — the
    /// combined set satisfies `WidgetStateConstraint::Is(Selected)` (the
    /// first entry in the map below), so the OLD code resolved
    /// `selected_style` for a disabled+selected destination instead of
    /// `disabled_style`.
    #[test]
    fn theme_disabled_and_selected_resolves_the_disabled_entry_not_the_selected_one() {
        use flui_widgets::WidgetStateConstraint;

        let selected_style = Color::rgb(1, 1, 1);
        let disabled_style = Color::rgb(2, 2, 2);
        let theme: WidgetStateProperty<Option<Color>> = WidgetStateProperty::from_map([
            (
                WidgetStateConstraint::Is(WidgetState::Selected),
                Some(selected_style),
            ),
            (
                WidgetStateConstraint::Is(WidgetState::Disabled),
                Some(disabled_style),
            ),
            (WidgetStateConstraint::Any, None),
        ]);

        // Selected AND disabled.
        let states = navigation_destination_states(true, false);
        let resolved = resolve_navigation_destination_icon_color(Some(&theme), &light(), states);

        assert_eq!(
            resolved, disabled_style,
            "a disabled destination must resolve the theme's Disabled entry even when it is \
             ALSO selected — the oracle never queries a combined {{selected, disabled}} set, so \
             a theme Map ordered Selected-before-Disabled must still give the disabled result",
        );
    }
}
