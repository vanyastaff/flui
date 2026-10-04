//! [`Chip`] and [`FilterChip`] — the M3 chip family V1: a compact,
//! outlined/filled information element with an optional leading avatar and
//! trailing delete affordance.
//!
//! # V1 scope: a reduced chip family
//!
//! The M3 chip family has several kinds (assist, input, choice, filter,
//! action) that differ only in how one shared state machine is configured.
//! This V1 ships two shapes, [`Chip`] and [`FilterChip`], each composing the
//! same reduced primitives ([`Material`], [`InkWell`], shared default-token
//! functions in this module) directly. Input, choice and action chips are
//! named deferrals: each is a thin reconfiguration the same shared functions
//! already support, they just have no constructor yet.
//!
//! [`Chip`] uses the generic M3 chip default table and widens it with an
//! optional [`Chip::on_pressed`], so a tappable "assist-shaped" chip is
//! available. This is a deliberate, documented widening of `Chip`'s surface,
//! not a change to the token values.
//!
//! Similarly, [`Chip::enabled`] is exposed directly so the M3 default
//! table's disabled branch is reachable and testable on this V1 type.
//!
//! # `ChipThemeData`: plain overrides, not `WidgetStateProperty`
//!
//! Unlike [`crate::CheckboxThemeData`]/[`crate::SwitchThemeData`]/
//! [`crate::RadioThemeData`]/[`crate::NavigationBarThemeData`], whose color
//! slots are all `Option<WidgetStateProperty<Option<Color>>>`,
//! [`crate::ChipThemeData`]'s fields are **plain** (`Option<Color>`,
//! `Option<BorderSide<f64>>`, …): every field except `color` (the container
//! fill, not exposed at the theme tier here — see below) is a plain,
//! non-resolved value. Per-state
//! variation for label/icon/delete-icon color is entirely a property of the
//! M3 *default* tables (each reconstructed fresh per build with the current
//! enabled/selected state already closed over — so their getters return
//! already-resolved plain values, not deferred per-state properties); the
//! theme/widget tiers above that default only ever override with one fixed
//! value, never a function of state.
//!
//! This has a structural benefit: with no
//! `WidgetStateProperty::Map` anywhere in [`crate::ChipThemeData`], the
//! first-match-wins map-ordering hazard [`crate::NavigationBar`]'s own
//! module docs warn about (a `Map` ordered `[Is(Selected), Is(Disabled),
//! Any]`, queried with a combined `{Selected, Disabled}` set, resolving the
//! wrong entry) cannot occur here at all — there is no map to order
//! wrong. The only place this module builds a [`WidgetStates`] query set is
//! `chip_states` (this module, private), which — like
//! `navigation_bar::navigation_destination_states` (also private) — returns
//! a **pure** single-state set (`{Disabled}` xor `{Selected}` xor `{}`,
//! never both), used solely to walk the M3 default tables' own `disabled >
//! selected > else` branch order for label/icon/delete-icon color. The
//! container fill color and the border `side` both have a genuinely
//! *combined*-state-dependent M3 default (see this module's own
//! `filter_chip_default_background_color` and `chip_default_side`
//! functions) that a pure query cannot express — both are implemented as
//! plain `(bool, bool)`-parameterized functions instead, with no
//! [`WidgetStates`]/`WidgetStateProperty` involved, sidestepping the hazard
//! class entirely rather than papering over it.
//!
//! `color` (the container fill) is not exposed as a [`crate::ChipThemeData`]
//! slot in V1: its M3 default has a real three-way branch (disabled-only,
//! selected-only, and a *third*, distinct disabled-AND-selected value — see
//! this module's own `filter_chip_default_background_color`) that only a
//! genuinely combined-state query can reproduce, which is exactly the shape
//! a caller-supplied `WidgetStateProperty` override could get wrong. Named
//! deferral, not a silent gap.
//!
//! # Container: `CustomPaint` foreground border, `Material` fill
//!
//! [`Material`] fills, clips, and elevates but paints no border side (see
//! that module's shape docs) — the same gap [`crate::OutlinedButton`] left
//! unpainted. [`Chip`]'s outline is load-bearing (the base chip has no fill
//! at all — its default color is transparent, so the
//! stroke is the only visible container boundary), so this V1 draws it
//! directly: a [`flui_sdk::widgets::CustomPaint`] wraps the [`Material`] subtree
//! with a `foreground_painter` that strokes the resolved [`MaterialShape`]
//! as an inset ring (`Canvas::draw_drrect` between the outer shape and an
//! inward-inset copy) — the same real-geometry approach this crate's
//! `checkbox::CheckboxPainter` (private) already established for a stroked
//! rounded shape, extended from a fixed-size leaf painter to a
//! child-sized container border. `CustomPaint` sizes to its child when one
//! is present, so the border painter always strokes at the exact size the
//! `Material`/`InkWell`/content subtree settles on.
//!
//! # Selection: checkmark replaces the avatar, snapped
//!
//! A fuller implementation animates a selected [`FilterChip`]'s leading slot
//! between the avatar and an overlaid checkmark (animated avatar-drawer width
//! plus a blended darkening scrim under the check). This V1 **snaps**: no
//! animation, and the checkmark *replaces* the avatar in the leading slot
//! rather than painting an overlay on top of it (see this module's own
//! `filter_chip_leading_content`) — chosen because painting a darkening scrim
//! over an arbitrary caller-supplied avatar widget has no home in this
//! substrate's paint primitives yet. The checkmark geometry itself
//! (`ChipCheckmarkPainter`, this module, private) is a real stroked
//! relative-coordinate path at its fully-settled shape — the same "real
//! stroked geometry, settled" precedent this crate's
//! `checkbox::CheckboxPainter`'s own module docs describe.
//!
//! # Disabled content: steady-state 38% opacity, no fade
//!
//! A chip's avatar and its label/delete icon are drawn at a reduced alpha
//! (`0x61`) whenever the chip is not fully enabled — and that is true not
//! only *during* an enable/disable transition but for the entire
//! steady-state lifetime of a chip that is (and stays) disabled. This V1 has
//! no enable animation to run at all (see the "Deferred" list below), but the
//! *steady-state* alpha is still real, observable behavior a disabled chip
//! must show — not merely a transition artifact safe to snap away. So
//! [`Chip`]/[`FilterChip`] wrap their composed avatar/label/delete content
//! (never the container fill or border, which are not drawn through this
//! opacity layer) in one [`flui_sdk::widgets::Opacity`] at the private
//! `DISABLED_CONTENT_ALPHA` when disabled, `1.0` when enabled — a single
//! group wrap, which is equivalent to wrapping each slot separately since
//! every slot uses the identical alpha value.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Avatar/delete/selection/enable animation** — every transition snaps
//!   directly to its settled end state (including the disabled-content
//!   opacity — see the "Disabled content" section above: the *value* is
//!   applied, the *fade into/out of* it is not); see the sections above.
//! - **Elevated variants** (and any chip's non-zero elevation or press
//!   elevation) — V1 is flat-only, elevation fixed at `0.0`.
//! - **Input, choice and action chips** — see the "V1 scope"
//!   section above.
//! - **Custom `delete_icon` widget override** — the delete affordance
//!   always renders the M3 default glyph (`cancel` for [`Chip`], `clear` for
//!   [`FilterChip`]).
//! - **Delete-button tooltip** — no localization
//!   substrate consumes it yet.
//! - **RTL** — the content `Row` always lays out left-to-right; no
//!   `Directionality` ambient in this substrate yet (the same gap
//!   [`flui_sdk::widgets::Icon`]'s own docs already name).
//! - **`focus_node`/`autofocus`** — [`InkWell`] itself has no `autofocus`
//!   hook yet, matching every other selection-control's own deferred list.
//! - **Avatar/delete-icon box constraints** — the avatar
//!   sizes intrinsically (no forced square constraint); the delete icon is
//!   fixed at [`CHIP_ICON_SIZE`].
//! - **Material elevation interplay, press elevation** — elevation is fixed
//!   at `0.0` for both types in V1 (matches "flat only").

use std::rc::Rc;
use std::sync::Arc;

use flui_sdk::painting::Canvas;
use flui_sdk::painting::TextStyle;
use flui_sdk::painting::{BorderSide, BorderStyle};
use flui_sdk::painting::{Paint, Path};
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::icon::IconData;
use flui_sdk::widgets::{
    ConstrainedBox, CrossAxisAlignment, CustomPaint, CustomPainter, DefaultTextStyle, Icon,
    IconTheme, IconThemeData, MainAxisSize, Opacity, Padding, Row, Semantics, WidgetState,
    WidgetStates,
};
use flui_sdk::{
    geometry::{EdgeInsets, Point, Size},
    painting::Color,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// The container's target height when its content fits within it.
pub const CHIP_HEIGHT: f64 = 32.0;

/// The container's corner radius, `8.0`.
const CORNER_RADIUS: f64 = 8.0;

/// The avatar/delete-icon/checkmark side length, `18.0`.
pub const CHIP_ICON_SIZE: f64 = 18.0;

/// The default container padding, `EdgeInsets::all(8.0)`.
const PADDING: f64 = 8.0;

/// The default label padding (horizontal only), at text scale 1x — the
/// text-scaler-driven 8px-to-4px interpolation is a named V1
/// simplification (no `MediaQuery` text-scaling substrate consumed here,
/// the same gap [`crate::elevated_button`]'s own `scaled_padding_1x` docs
/// already name for button padding).
const LABEL_PADDING_HORIZONTAL: f64 = 8.0;

/// The `cancel` icon's codepoint (`MaterialIcons`), [`Chip`]'s default delete
/// glyph.
const DELETE_ICON_CANCEL_CODEPOINT: u32 = 0xE139;

/// The `clear` icon's codepoint (`MaterialIcons`), [`FilterChip`]'s default
/// delete glyph.
const DELETE_ICON_CLEAR_CODEPOINT: u32 = 0xE168;

/// The opacity a disabled chip's avatar/label/delete content settles at
/// (`0x61`) — see the module docs' "Disabled content" section for why this is
/// steady-state behavior, not merely a transition artifact this V1 is
/// entitled to snap away.
const DISABLED_CONTENT_ALPHA: f64 = 0x61 as f64 / 255.0;

/// The content opacity for a chip in `enabled`'s state — `1.0` enabled,
/// [`DISABLED_CONTENT_ALPHA`] disabled. Extracted as its own pure function
/// (not left inline in `build`) so the two-value table is unit-testable
/// without mounting a widget tree.
fn disabled_content_opacity(enabled: bool) -> f64 {
    if enabled { 1.0 } else { DISABLED_CONTENT_ALPHA }
}

// Compile-time geometry invariant (not a runtime test — every side is
// `const`): the default padding must leave room for a positive content
// height inside the target container height.
const _: () = assert!(PADDING * 2.0 < CHIP_HEIGHT);

fn cancel_icon_data() -> IconData {
    IconData::new(DELETE_ICON_CANCEL_CODEPOINT).with_font_family("Material Icons")
}

fn clear_icon_data() -> IconData {
    IconData::new(DELETE_ICON_CLEAR_CODEPOINT).with_font_family("Material Icons")
}

/// Builds the PURE (never-combined) [`WidgetStates`] query set the M3
/// default tables' `disabled > selected > else` branch order resolves
/// against.
///
/// This is the same shape the private `navigation_destination_states`
/// helper (`navigation_bar.rs`) establishes, applied here for the identical
/// reason — the label/icon/delete-icon-color defaults all read as a plain
/// `enabled ? (selected ? A : B) : C` ternary (disabled always wins, selected
/// only distinguishes within the enabled branch), never a state genuinely
/// carrying both `Selected` and `Disabled` at once for those fields. A
/// combined query would risk resolving the wrong branch
/// through a `WidgetStateProperty::Map`-shaped consumer (see the module
/// docs) even though [`crate::ChipThemeData`] itself has no such field
/// today.
///
/// **Not** used for the container fill color or the border `side`, both of
/// which have a genuinely combined-state-dependent M3 default that this
/// pure set cannot express — see [`filter_chip_default_background_color`]
/// and [`chip_default_side`].
fn chip_states(selected: bool, enabled: bool) -> WidgetStates {
    if !enabled {
        WidgetStates::from(WidgetState::Disabled)
    } else if selected {
        WidgetStates::from(WidgetState::Selected)
    } else {
        WidgetStates::NONE
    }
}

/// Resolves a pure `disabled > selected > else` M3 default against
/// [`chip_states`]'s query set. Shared by every chip default-table function
/// that has this exact branch shape (see [`chip_states`]'s doc comment).
fn resolve_pure_chip_default(
    states: WidgetStates,
    disabled: Color,
    selected: Color,
    unselected: Color,
) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        disabled
    } else if states.contains_state(WidgetState::Selected) {
        selected
    } else {
        unselected
    }
}

/// The label and delete-icon color default table: a three-way
/// (`disabled`/`selected`/`else`) table. A bare [`Chip`] is never selected, so
/// its plain two-way `enabled ? onSurfaceVariant : onSurface` table is
/// identical to this function's `unselected` branch, and [`Chip`]'s own
/// [`chip_states`] call site never sets [`WidgetState::Selected`] (see
/// [`chip_icon_color_default`]'s doc comment for the same note) — so reusing
/// this one function for both [`Chip`] and [`FilterChip`] is safe.
fn chip_content_color_default(states: WidgetStates, colors: &ColorScheme) -> Color {
    resolve_pure_chip_default(
        states,
        colors.on_surface,
        colors.on_secondary_container,
        colors.on_surface_variant,
    )
}

/// The avatar and checkmark icon color default table: a three-way
/// (`disabled`/`selected`/`else`) table. A bare [`Chip`]'s plain two-way
/// `enabled ? primary : onSurface` table is identical to this function's
/// `unselected` branch, and [`Chip`]'s own states never carry `Selected` (see
/// [`chip_states`]'s call site in [`Chip`]'s build), so the `selected` branch
/// is simply unreachable there.
fn chip_icon_color_default(states: WidgetStates, colors: &ColorScheme) -> Color {
    resolve_pure_chip_default(
        states,
        colors.on_surface,
        colors.on_secondary_container,
        colors.primary,
    )
}

/// The container border default table (flat variant only — see the module
/// docs), a `selected`-gated table. A bare `Chip` is never selected, and its
/// plain two-way `enabled ? outlineVariant : onSurface@12%` table agrees with
/// this function's `enabled`/`disabled` (unselected) branches, so [`Chip`]
/// safely calls this same function with `selected` pinned to `false`.
///
/// **Combined-state, not pure**: `selected` is checked FIRST and wins
/// unconditionally (a selected chip's side is transparent whether or not it
/// is also disabled) — unlike [`chip_content_color_default`]/
/// [`chip_icon_color_default`], `disabled` does NOT take priority here. A
/// disabled-and-selected chip's side is `transparent` (the selected
/// branch), not the disabled-only `onSurface@12%` a pure `disabled`-first
/// query would give.
/// Because of this real branch-order difference, `side` is resolved from
/// plain `(bool, bool)` parameters rather than a [`WidgetStates`] query —
/// see the module docs' "`ChipThemeData`: plain overrides" section.
fn chip_default_side(selected: bool, enabled: bool, colors: &ColorScheme) -> BorderSide<f64> {
    if selected {
        BorderSide::new(Color::TRANSPARENT, 1.0, BorderStyle::Solid)
    } else if enabled {
        BorderSide::new(colors.outline_variant, 1.0, BorderStyle::Solid)
    } else {
        BorderSide::new(
            colors.on_surface.with_opacity(0.12),
            1.0,
            BorderStyle::Solid,
        )
    }
}

/// The default container shape: an 8dp rounded rectangle.
fn chip_default_shape() -> MaterialShape {
    use flui_sdk::painting::BorderRadius;
    MaterialShape::RoundedRect(BorderRadius::all(flui_sdk::geometry::Radius::circular(
        CORNER_RADIUS,
    )))
}

/// The default container padding: `EdgeInsets::all(8.0)`.
fn chip_default_padding() -> EdgeInsets {
    EdgeInsets::all(PADDING)
}

/// The default label padding: `EdgeInsets.symmetric(horizontal: 8.0)` — the
/// text-scale-1x tier (see the module doc on [`LABEL_PADDING_HORIZONTAL`]).
fn chip_default_label_padding() -> EdgeInsets {
    EdgeInsets::symmetric(0.0, LABEL_PADDING_HORIZONTAL)
}

/// The container's minimum content height (excludes `padding`, includes
/// `label_padding`'s own vertical inset): `CHIP_HEIGHT - padding.vertical +
/// label_padding.vertical`, floored at `0.0`. Only the floor term is
/// computed — the label's own intrinsic height is already accommodated by
/// this substrate's plain `ConstrainedBox` + `Row` composition, which grows
/// past the floor when the label needs more room.
fn chip_content_min_height(padding: EdgeInsets, label_padding: EdgeInsets) -> f64 {
    let floor = CHIP_HEIGHT - padding.vertical_total() + label_padding.vertical_total();
    floor.max(0.0)
}

/// A tap/press callback taking no arguments. `Rc`-based (owner-local, per
/// ADR-0027) — matches [`InkWell::on_tap`]'s own callback shape.
type ChipTapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>;

/// A selection-change callback: the next selected value. `Rc`-based, same
/// shape as [`ChipTapCallback`].
type FilterChipSelectCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>, bool)>;

/// A Material Design chip: a compact label with an optional leading avatar
/// and trailing delete affordance, outlined and unfilled by default.
///
/// See the module docs for the V1 scope (a reduced chip family, with an
/// [`Chip::on_pressed`] widening) and named deferrals.
///
/// ```rust
/// use flui_material::Chip;
/// use flui_sdk::widgets::Text;
///
/// let _info = Chip::new(Text::new("Tag"));
/// let _pressable = Chip::new(Text::new("Tag")).on_pressed(|_cx| {});
/// let _deletable = Chip::new(Text::new("Tag")).on_deleted(|_cx| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct Chip {
    label: BoxedView,
    avatar: Option<BoxedView>,
    on_pressed: Option<ChipTapCallback>,
    on_deleted: Option<ChipTapCallback>,
    enabled: bool,
}

impl std::fmt::Debug for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chip")
            .field("has_avatar", &self.avatar.is_some())
            .field("is_pressable", &self.is_pressable())
            .field("has_delete_button", &self.has_delete_button())
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl Chip {
    /// A `Chip` showing `label`, enabled, with no avatar, press handler, or
    /// delete handler.
    pub fn new(label: impl IntoView) -> Self {
        Self {
            label: BoxedView(Box::new(label.into_view())),
            avatar: None,
            on_pressed: None,
            on_deleted: None,
            enabled: true,
        }
    }

    /// Sets the leading avatar (typically a small icon or image).
    #[must_use]
    pub fn avatar(mut self, avatar: impl IntoView) -> Self {
        self.avatar = Some(BoxedView(Box::new(avatar.into_view())));
        self
    }

    /// Sets the press handler. Presence of a handler is what makes this
    /// chip tappable — see the module docs' "V1 scope" section.
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(crate::event_callback::press_callback(callback));
        self
    }

    /// Sets the delete handler. Presence of a handler is what shows the
    /// trailing delete icon.
    #[must_use]
    pub fn on_deleted<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_deleted = Some(crate::event_callback::press_callback(callback));
        self
    }

    /// Sets whether this chip responds to interaction. Defaults to `true`
    /// (see the module docs).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn is_pressable(&self) -> bool {
        self.enabled && self.on_pressed.is_some()
    }

    fn has_delete_button(&self) -> bool {
        self.on_deleted.is_some()
    }
}

impl StatelessView for Chip {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        let chip_theme = theme.chip_theme.clone();

        let states = chip_states(false, self.enabled);

        let label_color = chip_theme
            .as_ref()
            .and_then(|t| t.label_color)
            .unwrap_or_else(|| chip_content_color_default(states, &colors));
        let label_style = theme
            .text_theme
            .label_large
            .unwrap_or_default()
            .with_color(label_color);

        let icon_color = chip_theme
            .as_ref()
            .and_then(|t| t.icon_color)
            .unwrap_or_else(|| chip_icon_color_default(states, &colors));

        let delete_icon_color = chip_theme
            .as_ref()
            .and_then(|t| t.delete_icon_color)
            .unwrap_or_else(|| chip_content_color_default(states, &colors));

        let side = chip_theme
            .as_ref()
            .and_then(|t| t.side)
            .unwrap_or_else(|| chip_default_side(false, self.enabled, &colors));

        let shape = chip_theme
            .as_ref()
            .and_then(|t| t.shape)
            .unwrap_or_else(chip_default_shape);

        let padding = chip_theme
            .as_ref()
            .and_then(|t| t.padding)
            .unwrap_or_else(chip_default_padding);

        let label_padding = chip_theme
            .as_ref()
            .and_then(|t| t.label_padding)
            .unwrap_or_else(chip_default_label_padding);

        // `_ChipDefaultsM3.color => null` — the base chip has no fill; see
        // the module docs' "Container" section.
        let background_color = Color::TRANSPARENT;

        let avatar_view = self.avatar.clone().map(|avatar| {
            IconTheme::new(
                IconThemeData {
                    size: Some(CHIP_ICON_SIZE),
                    color: Some(icon_color),
                    ..IconThemeData::default()
                },
                avatar,
            )
            .boxed()
        });

        let delete_view = self.on_deleted.clone().map(|on_deleted| {
            let mut delete_button = InkWell::new(IconTheme::new(
                IconThemeData {
                    size: Some(CHIP_ICON_SIZE),
                    color: Some(delete_icon_color),
                    ..IconThemeData::default()
                },
                Icon::new(cancel_icon_data()),
            ))
            .shape(MaterialShape::Stadium);
            if self.enabled {
                delete_button = delete_button.on_tap(move |cx| on_deleted(cx));
            }
            delete_button.boxed()
        });

        let content = build_chip_row(
            avatar_view,
            self.label.clone(),
            label_style,
            label_padding,
            delete_view,
        );
        let content = Opacity::new(disabled_content_opacity(self.enabled)).child(content);

        let padded_content = Padding::new(padding).child(
            ConstrainedBox::new(chip_content_constraints(padding, label_padding)).child(content),
        );

        let mut ink_well = InkWell::new(padded_content).shape(shape);
        if self.is_pressable() {
            let on_pressed = self.on_pressed.clone();
            ink_well = ink_well.on_tap(move |cx| {
                if let Some(handler) = &on_pressed {
                    handler(cx);
                }
            });
        }

        let container = CustomPaint::new()
            .foreground_painter(
                Arc::new(ChipBorderPainter { side, shape }) as Arc<dyn CustomPainter>
            )
            .child(Material::new(background_color).shape(shape).child(ink_well));

        Semantics::new()
            .button(self.on_pressed.is_some())
            .enabled(self.enabled)
            .child(container)
    }
}

/// The M3 filter chip: a toggleable chip showing a leading checkmark (in
/// place of the avatar — see the module docs' "Selection" section) when
/// [`FilterChip::selected`].
///
/// See the module docs for the V1 scope (flat variant only) and named
/// deferrals.
///
/// ```rust
/// use flui_material::FilterChip;
/// use flui_sdk::widgets::Text;
///
/// let _chip = FilterChip::new(Text::new("Vegetarian"))
///     .selected(true)
///     .on_selected(|_cx, _next| { /* ... */ });
/// let _disabled = FilterChip::new(Text::new("Vegetarian"));
/// ```
#[derive(Clone, StatelessView)]
pub struct FilterChip {
    label: BoxedView,
    avatar: Option<BoxedView>,
    selected: bool,
    on_selected: Option<FilterChipSelectCallback>,
    on_deleted: Option<ChipTapCallback>,
}

impl std::fmt::Debug for FilterChip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterChip")
            .field("selected", &self.selected)
            .field("has_avatar", &self.avatar.is_some())
            .field("is_interactive", &self.on_selected.is_some())
            .field("has_delete_button", &self.on_deleted.is_some())
            .finish_non_exhaustive()
    }
}

impl FilterChip {
    /// A `FilterChip` showing `label`, unselected, disabled (no
    /// [`Self::on_selected`] set yet), with no avatar or delete handler.
    pub fn new(label: impl IntoView) -> Self {
        Self {
            label: BoxedView(Box::new(label.into_view())),
            avatar: None,
            selected: false,
            on_selected: None,
            on_deleted: None,
        }
    }

    /// Sets the leading avatar shown while unselected. Replaced by a
    /// checkmark while selected — see the module docs' "Selection" section.
    #[must_use]
    pub fn avatar(mut self, avatar: impl IntoView) -> Self {
        self.avatar = Some(BoxedView(Box::new(avatar.into_view())));
        self
    }

    /// Sets whether this chip is currently selected.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the selection-change handler, fired with the next selected
    /// value on tap. Presence of a handler is what makes this chip
    /// interactive.
    #[must_use]
    pub fn on_selected<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>, bool) -> R + 'static,
    ) -> Self {
        self.on_selected = Some(crate::event_callback::value_callback(callback));
        self
    }

    /// Sets the delete handler. Presence of a handler is what shows the
    /// trailing delete icon.
    #[must_use]
    pub fn on_deleted<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_deleted = Some(crate::event_callback::press_callback(callback));
        self
    }

    fn is_enabled(&self) -> bool {
        self.on_selected.is_some()
    }
}

/// Which widget occupies a filter chip's leading slot. See
/// [`filter_chip_leading_content`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterChipLeading {
    /// Selected: a checkmark, regardless of whether an avatar is set — see
    /// the module docs' "Selection" section.
    Checkmark,
    /// Unselected, with an avatar set.
    Avatar,
    /// Unselected, no avatar.
    None,
}

/// Decides the leading slot's content — a pure decision function so the
/// avatar/checkmark swap is unit-testable without mounting a widget tree.
/// "Checkmark wins outright when selected", per the module docs' "Selection"
/// section (a fuller implementation keeps both children present and blends
/// between them; V1 shows exactly one).
fn filter_chip_leading_content(selected: bool, has_avatar: bool) -> FilterChipLeading {
    if selected {
        FilterChipLeading::Checkmark
    } else if has_avatar {
        FilterChipLeading::Avatar
    } else {
        FilterChipLeading::None
    }
}

/// The container fill color default table (flat variant only — see the
/// module docs).
///
/// **Genuinely combined-state, not pure**: disabled-and-selected resolves
/// to its own distinct value (`onSurface@12%`), different from BOTH plain
/// `disabled` (`transparent` — no default color, the same "no fill" the
/// base [`Chip`] has) and plain `selected` (`secondaryContainer`). A pure
/// `disabled`-first-then-`selected` query cannot express this third
/// outcome, which is why this function takes `(bool, bool)` directly
/// rather than a [`WidgetStates`] set — see the module docs'
/// `ChipThemeData` section.
fn filter_chip_default_background_color(
    selected: bool,
    enabled: bool,
    colors: &ColorScheme,
) -> Color {
    match (selected, enabled) {
        (true, false) => colors.on_surface.with_opacity(0.12),
        (true, true) => colors.secondary_container,
        // Unselected resolves to no fill either way — enabled and disabled
        // are genuinely the same value here (unlike the selected column
        // above): both unselected branches fall through to no fill.
        (false, false | true) => Color::TRANSPARENT,
    }
}

impl StatelessView for FilterChip {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        let chip_theme = theme.chip_theme.clone();
        let enabled = self.is_enabled();
        let selected = self.selected;

        let states = chip_states(selected, enabled);

        let label_color = chip_theme
            .as_ref()
            .and_then(|t| t.label_color)
            .unwrap_or_else(|| chip_content_color_default(states, &colors));
        let label_style = theme
            .text_theme
            .label_large
            .unwrap_or_default()
            .with_color(label_color);

        let icon_color = chip_theme
            .as_ref()
            .and_then(|t| t.icon_color)
            .unwrap_or_else(|| chip_icon_color_default(states, &colors));

        let checkmark_color = chip_theme
            .as_ref()
            .and_then(|t| t.checkmark_color)
            .unwrap_or_else(|| chip_icon_color_default(states, &colors));

        let delete_icon_color = chip_theme
            .as_ref()
            .and_then(|t| t.delete_icon_color)
            .unwrap_or_else(|| chip_content_color_default(states, &colors));

        let side = chip_theme
            .as_ref()
            .and_then(|t| t.side)
            .unwrap_or_else(|| chip_default_side(selected, enabled, &colors));

        let shape = chip_theme
            .as_ref()
            .and_then(|t| t.shape)
            .unwrap_or_else(chip_default_shape);

        let padding = chip_theme
            .as_ref()
            .and_then(|t| t.padding)
            .unwrap_or_else(chip_default_padding);

        let label_padding = chip_theme
            .as_ref()
            .and_then(|t| t.label_padding)
            .unwrap_or_else(chip_default_label_padding);

        let background_color = filter_chip_default_background_color(selected, enabled, &colors);

        let leading = match filter_chip_leading_content(selected, self.avatar.is_some()) {
            FilterChipLeading::Checkmark => Some(
                CustomPaint::new()
                    .size(Size::new(CHIP_ICON_SIZE, CHIP_ICON_SIZE))
                    .painter(Arc::new(ChipCheckmarkPainter {
                        color: checkmark_color,
                    }) as Arc<dyn CustomPainter>)
                    .boxed(),
            ),
            FilterChipLeading::Avatar => self.avatar.clone().map(|avatar| {
                IconTheme::new(
                    IconThemeData {
                        size: Some(CHIP_ICON_SIZE),
                        color: Some(icon_color),
                        ..IconThemeData::default()
                    },
                    avatar,
                )
                .boxed()
            }),
            FilterChipLeading::None => None,
        };

        let delete_view = self.on_deleted.clone().map(|on_deleted| {
            let mut delete_button = InkWell::new(IconTheme::new(
                IconThemeData {
                    size: Some(CHIP_ICON_SIZE),
                    color: Some(delete_icon_color),
                    ..IconThemeData::default()
                },
                Icon::new(clear_icon_data()),
            ))
            .shape(MaterialShape::Stadium);
            if enabled {
                delete_button = delete_button.on_tap(move |cx| on_deleted(cx));
            }
            delete_button.boxed()
        });

        let content = build_chip_row(
            leading,
            self.label.clone(),
            label_style,
            label_padding,
            delete_view,
        );
        let content = Opacity::new(disabled_content_opacity(enabled)).child(content);

        let padded_content = Padding::new(padding).child(
            ConstrainedBox::new(chip_content_constraints(padding, label_padding)).child(content),
        );

        let mut ink_well = InkWell::new(padded_content).shape(shape);
        if enabled {
            let on_selected = self.on_selected.clone();
            ink_well = ink_well.on_tap(move |cx| {
                if let Some(handler) = &on_selected {
                    handler(cx, !selected);
                }
            });
        }

        let container = CustomPaint::new()
            .foreground_painter(
                Arc::new(ChipBorderPainter { side, shape }) as Arc<dyn CustomPainter>
            )
            .child(Material::new(background_color).shape(shape).child(ink_well));

        Semantics::new()
            .selected(selected)
            .button(true)
            .enabled(enabled)
            .child(container)
    }
}

/// Assembles a chip's `leading? / label / delete?` content row. Shared by
/// [`Chip`] and [`FilterChip`] — the only difference between the two is
/// what `leading` resolves to.
fn build_chip_row(
    leading: Option<BoxedView>,
    label: BoxedView,
    label_style: TextStyle,
    label_padding: EdgeInsets,
    delete: Option<BoxedView>,
) -> Row {
    let mut children: Vec<BoxedView> = Vec::new();
    if let Some(leading) = leading {
        children.push(leading);
    }
    children.push(
        Padding::new(label_padding)
            .child(DefaultTextStyle::new(label_style, label))
            .boxed(),
    );
    if let Some(delete) = delete {
        children.push(delete);
    }
    Row::new(children)
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
}

/// The `ConstrainedBox` constraints imposing [`chip_content_min_height`]'s
/// floor on the content row.
fn chip_content_constraints(padding: EdgeInsets, label_padding: EdgeInsets) -> BoxConstraints {
    BoxConstraints::new(
        0.0,
        f64::INFINITY,
        chip_content_min_height(padding, label_padding),
        f64::INFINITY,
    )
}

/// Strokes a chip's resolved [`MaterialShape`] as an inset ring — the same
/// `Canvas::draw_drrect`-between-two-insets approach the private
/// `CheckboxPainter` (`checkbox.rs`) uses for its own stroked box, see the
/// module docs' "Container" section for why this substitutes for
/// [`Material`]'s own missing border-side paint path.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChipBorderPainter {
    side: BorderSide<f64>,
    shape: MaterialShape,
}

impl CustomPainter for ChipBorderPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !self.side.style.is_solid() || self.side.width <= 0.0 {
            return;
        }
        let outer = self.shape.to_rrect(size);
        let inner = outer.inflate(-self.side.width);
        canvas.draw_drrect(outer, inner, &Paint::fill(self.side.color));
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old != self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Paints the settled (non-animated) checkmark [`FilterChip`] shows in its
/// leading slot while selected — a relative-coordinate stroke path at its
/// fully-settled shape (the full `start -> mid -> end` polyline, no animated
/// partial stroke) — the same "real stroked geometry, settled" precedent the
/// private `CheckboxPainter`'s (`checkbox.rs`) own module docs describe,
/// using the identical relative coordinates (`0.15, 0.45` / `0.4, 0.7` /
/// `0.85, 0.25`) that painter's own `draw_checkmark` uses — but, unlike that
/// full-cell checkmark, scaled to `avatar height * 0.75` and offset by
/// `avatar height * 0.125` on both axes (a little smaller than the avatar):
/// this painter's `size` is the full leading-slot cell (avatar's own size),
/// so the checkmark itself must be drawn at 75% of that cell, inset by
/// 12.5% on each side — drawing at the full cell size would be ~33%
/// oversized and pinned to the wrong corner.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChipCheckmarkPainter {
    color: Color,
}

impl CustomPainter for ChipCheckmarkPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let cell = size.height;
        // The stroke scales with the FULL cell height, not `check_size`.
        let stroke_width = 2.0 * cell / 24.0;
        let paint = Paint::stroke(self.color, stroke_width);

        // The check is 75% of the cell, offset by 12.5% on both axes — see
        // this struct's own doc comment.
        let check_size = cell * 0.75;
        let origin_offset = cell * 0.125;
        let point = |dx: f64, dy: f64| Point::new(origin_offset + dx, origin_offset + dy);
        let mut path = Path::new();
        path.move_to(point(check_size * 0.15, check_size * 0.45));
        path.line_to(point(check_size * 0.4, check_size * 0.7));
        path.line_to(point(check_size * 0.85, check_size * 0.25));
        canvas.draw_path(&path, &paint);
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old != self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
