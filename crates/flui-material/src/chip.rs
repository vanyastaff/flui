//! [`Chip`] and [`FilterChip`] — the M3 chip family V1: a compact,
//! outlined/filled information element with an optional leading avatar and
//! trailing delete affordance.
//!
//! # Flutter parity
//!
//! `material/chip.dart`'s `Chip`/`RawChip`/`_ChipDefaultsM3`,
//! `material/chip_theme.dart`'s `ChipThemeData`, and
//! `material/filter_chip.dart`'s `FilterChip`/`_FilterChipDefaultsM3` (oracle
//! tag `3.44.0`).
//!
//! # V1 scope: `RawChip`, honestly reduced
//!
//! The oracle's chip family is one `RawChip` state machine that every
//! concrete chip type (`Chip`, `InputChip`, `ChoiceChip`, `FilterChip`,
//! `ActionChip`) configures differently. FLUI does not port `RawChip` itself
//! — it ports the two shapes this V1 ships, [`Chip`] and [`FilterChip`],
//! each composing the same reduced primitives ([`Material`], [`InkWell`],
//! shared default-token functions in this module) directly. `InputChip`,
//! `ChoiceChip`, and `ActionChip` are named deferrals: each is a thin
//! `RawChip` reconfiguration the same shared functions already support, they
//! just have no constructor yet.
//!
//! No `assist_chip.dart` file exists at this oracle tag — M3's "assist chip"
//! shape is realized by `ActionChip`'s own `_ActionChipDefaultsM3`, a
//! variant `_ChipDefaultsM3` this V1 does not port. [`Chip`] instead ports
//! `_ChipDefaultsM3` directly (`RawChip`'s own generic fallback default
//! table, the one `Chip.build` implicitly gets since it passes no
//! `defaultProperties`) and widens it with an optional
//! [`Chip::on_pressed`] — the oracle's bare `Chip` never exposes `onPressed`
//! (`ActionChip` owns that), but `RawChip` itself already supports it, and a
//! tappable "assist-shaped" chip is exactly what this task calls for. This
//! is a deliberate, documented widening of `Chip`'s surface beyond the
//! oracle's own `Chip`, not a divergence in `_ChipDefaultsM3`'s token
//! values.
//!
//! Similarly, [`Chip::enabled`] surfaces `RawChip.isEnabled` directly (the
//! oracle's `Chip.build` never varies it — it is always `true`) so the M3
//! default table's disabled branch is reachable and testable on this V1
//! type, matching the task's expected disabled-state coverage.
//!
//! # `ChipThemeData`: plain overrides, not `WidgetStateProperty`
//!
//! Unlike [`crate::CheckboxThemeData`]/[`crate::SwitchThemeData`]/
//! [`crate::RadioThemeData`]/[`crate::NavigationBarThemeData`], whose color
//! slots are all `Option<WidgetStateProperty<Option<Color>>>` — because
//! their own oracle theme types (`checkbox_theme.dart` and siblings)
//! genuinely type those fields as `WidgetStateProperty`, [`crate::ChipThemeData`]'s
//! fields are **plain** (`Option<Color>`, `Option<BorderSide<Pixels>>`, …).
//! This mirrors `chip_theme.dart` exactly: every `ChipThemeData` field
//! except `color` (the container fill, not ported to the theme tier here —
//! see below) is a plain, non-resolved value in the oracle too. Per-state
//! variation for label/icon/delete-icon color is entirely a property of the
//! `_ChipDefaultsM3`/`_FilterChipDefaultsM3` *default* tables (each
//! reconstructed fresh per build with the current `isEnabled`/`isSelected`
//! already closed over — so their getters return already-resolved plain
//! values, not deferred per-state properties); the theme/widget tiers above
//! that default only ever override with one fixed value, never a function
//! of state.
//!
//! This has a structural benefit beyond fidelity: with no
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
//! at all — `_ChipDefaultsM3.color` is `null`, i.e. transparent, so the
//! stroke is the only visible container boundary), so this V1 draws it
//! directly: a [`flui_widgets::CustomPaint`] wraps the [`Material`] subtree
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
//! The oracle animates a selected [`FilterChip`]'s leading slot between the
//! avatar and an overlaid checkmark (`AnimationController`-driven
//! avatar-drawer width plus a `srcATop`-blended darkening scrim under the
//! check, `chip.dart`'s `_paintSelectionOverlay`). This V1 **snaps**: no
//! animation, and the checkmark *replaces* the avatar in the leading slot
//! rather than painting an overlay on top of it (see this module's own
//! `filter_chip_leading_content`) — a further reduction than the oracle's
//! own overlay shape, chosen because painting a darkening scrim over an
//! arbitrary caller-supplied avatar widget has no home in this substrate's
//! paint primitives yet. The checkmark geometry itself (`ChipCheckmarkPainter`,
//! this module, private) is a direct, honest port of the oracle's own
//! relative-coordinate stroke path (`_paintCheck`, `chip.dart`) at its
//! fully-settled shape — the same "real stroked geometry, `t == 1.0`"
//! precedent this crate's `checkbox::CheckboxPainter`'s own module docs
//! describe.
//!
//! # Disabled content: steady-state 38% opacity, no fade
//!
//! The oracle wraps a chip's avatar (`_paintAvatar`) and separately its
//! label/delete icon (`_paintChild`) each in their own `pushOpacity` (or an
//! equivalent `saveLayer`) at `_disabledColor.alpha`, gated on
//! `!enableAnimation.isCompleted` (`chip.dart` `:2199-2231`/`:2236-2275`,
//! oracle tag `3.44.0`) — and that gate is true not only *during* an
//! enable/disable transition but for the entire steady-state lifetime of a
//! chip that is (and stays) disabled, since `enableController` never runs
//! `forward()` for it. `_disabledColor.alpha` itself resolves to
//! `_kDisabledAlpha` (`0x61`, `chip.dart`) whenever `enableAnimation` is not
//! completed, `0xff` (opaque) once it is. This V1 has no
//! `enableController`/`AnimationController` to run at all (see the
//! "Deferred" list below), but the *steady-state* alpha is still real,
//! observable behavior a disabled chip must show — not merely a transition
//! artifact safe to snap away. So [`Chip`]/[`FilterChip`] wrap their
//! composed avatar/label/delete content (never the container fill or
//! border, which the oracle's `Ink`/`ShapeDecoration` painting never wraps
//! in this opacity layer either) in one [`flui_widgets::Opacity`] at the
//! private `DISABLED_CONTENT_ALPHA` when disabled, `1.0` when enabled — a single
//! group wrap rather than the oracle's three separate `pushOpacity` calls,
//! which is equivalent here since every one of those three calls uses the
//! identical alpha value (there is no per-slot variation to preserve by
//! keeping them separate).
//!
//! # Nested tap targets: a local `GestureArenaScope`
//!
//! The delete icon's `InkWell` sits nested inside the chip body's own
//! `InkWell` — two independent tappable regions on one hit-test path. A
//! bare `GestureDetector` with no ambient `GestureArenaScope` above it
//! builds its recognizers against a private arena it closes itself, which
//! is exactly right when it's the only detector in play but means two such
//! *standalone* detectors on the same path resolve completely
//! independently: a tap on the delete icon would fire both `on_deleted` and
//! the chip's own `on_pressed`/`on_selected`. Both [`Chip`] and
//! [`FilterChip`] close this by wrapping their whole built subtree in a
//! fresh, local `GestureArenaScope` (see this module's own
//! `wrap_local_gesture_arena`) so the two `InkWell`s genuinely compete —
//! confirmed by mounting a real tree and dispatching a real tap (see
//! `tests/chip.rs`'s delete-vs-chip-tap coverage), not merely inferred from
//! the tap-vs-long-press precedent `crates/flui-widgets/tests/gesture_detector_advanced.rs`
//! establishes for a different pair of gesture types.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Avatar/delete/selection/enable animation** — every transition snaps
//!   directly to its settled end state (including the disabled-content
//!   opacity — see the "Disabled content" section above: the *value* is
//!   ported, the *fade into/out of* it is not); see the sections above.
//! - **Elevated variants** (`FilterChip.elevated`, and any chip's non-zero
//!   `elevation`/`pressElevation`) — V1 is flat-only, elevation fixed at
//!   `0.0`.
//! - **`InputChip`, `ChoiceChip`, `ActionChip`** — see the "V1 scope"
//!   section above.
//! - **Custom `delete_icon` widget override** — the delete affordance
//!   always renders the M3 default glyph (`Icons.cancel` for [`Chip`],
//!   `Icons.clear` for [`FilterChip`] — see `_kDefaultDeleteIcon` and
//!   `FilterChip.build`'s own `resolvedDeleteIcon`, both `chip.dart`/
//!   `filter_chip.dart`).
//! - **Delete-button tooltip** (`deleteButtonTooltipMessage`,
//!   `MaterialLocalizations.deleteButtonTooltip`) — no localization
//!   substrate consumes it yet.
//! - **RTL** — the content `Row` always lays out left-to-right; no
//!   `Directionality` ambient in this substrate yet (the same gap
//!   [`flui_widgets::Icon`]'s own docs already name).
//! - **`focus_node`/`autofocus`** — [`InkWell`] itself has no `autofocus`
//!   hook yet, matching every other selection-control's own deferred list.
//! - **`avatarBoxConstraints`/`deleteIconBoxConstraints`** — the avatar
//!   sizes intrinsically (no forced square constraint); the delete icon is
//!   fixed at [`CHIP_ICON_SIZE`].
//! - **Material elevation interplay, press elevation** — elevation is fixed
//!   at `0.0` for both types in V1 (matches "flat only").

use std::rc::Rc;
use std::sync::Arc;

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::Canvas;
use flui_types::geometry::px;
use flui_types::painting::{Paint, Path};
use flui_types::styling::{BorderSide, BorderStyle};
use flui_types::typography::TextStyle;
use flui_types::{Color, EdgeInsets, Pixels, Point, Size};
use flui_view::prelude::*;
use flui_widgets::icon::IconData;
use flui_widgets::{
    ConstrainedBox, CrossAxisAlignment, CustomPaint, CustomPainter, DefaultTextStyle, Icon,
    IconTheme, IconThemeData, MainAxisSize, Opacity, Padding, Row, Semantics, WidgetState,
    WidgetStates,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// The container's target height when its content fits within it. Flutter
/// parity: `_kChipHeight` (`chip.dart`, oracle tag `3.44.0`).
pub const CHIP_HEIGHT: f32 = 32.0;

/// The container's corner radius. Flutter parity: `_ChipDefaultsM3.shape` /
/// `_FilterChipDefaultsM3`'s constructor, both
/// `RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(8.0)))`.
const CORNER_RADIUS: f32 = 8.0;

/// The avatar/delete-icon/checkmark side length. Flutter parity:
/// `_ChipDefaultsM3.iconTheme`/`_FilterChipDefaultsM3.iconTheme`'s
/// `size: 18.0`.
pub const CHIP_ICON_SIZE: f32 = 18.0;

/// The default container padding. Flutter parity: `_ChipDefaultsM3.padding`/
/// `_FilterChipDefaultsM3.padding`, `EdgeInsets.all(8.0)`.
const PADDING: f32 = 8.0;

/// The default label padding (horizontal only). Flutter parity: the
/// text-scale-1x tier of `_ChipDefaultsM3.labelPadding`/
/// `_FilterChipDefaultsM3.labelPadding`, `EdgeInsets.symmetric(horizontal:
/// 8.0)` — the text-scaler-driven 8px-to-4px interpolation is a named V1
/// simplification (no `MediaQuery` text-scaling substrate consumed here,
/// the same gap [`crate::elevated_button`]'s own `scaled_padding_1x` docs
/// already name for button padding).
const LABEL_PADDING_HORIZONTAL: f32 = 8.0;

/// `Icons.cancel`'s codepoint (`MaterialIcons`), [`Chip`]'s default delete
/// glyph. Flutter parity: `_kDefaultDeleteIcon = Icon(Icons.cancel)`
/// (`chip.dart`, oracle tag `3.44.0`).
const DELETE_ICON_CANCEL_CODEPOINT: u32 = 0xE139;

/// `Icons.clear`'s codepoint (`MaterialIcons`), [`FilterChip`]'s default
/// delete glyph. Flutter parity: `FilterChip.build`'s `resolvedDeleteIcon`
/// (`const Icon(Icons.clear, size: 18)`, `filter_chip.dart`, oracle tag
/// `3.44.0`).
const DELETE_ICON_CLEAR_CODEPOINT: u32 = 0xE168;

/// The opacity a disabled chip's avatar/label/delete content settles at.
/// Flutter parity: `_kDisabledAlpha` (`chip.dart`, `0x61`) — see the module
/// docs' "Disabled content" section for why this is steady-state behavior,
/// not merely a transition artifact this V1 is entitled to snap away.
const DISABLED_CONTENT_ALPHA: f32 = 0x61 as f32 / 255.0;

/// The content opacity for a chip in `enabled`'s state — `1.0` enabled,
/// [`DISABLED_CONTENT_ALPHA`] disabled. Extracted as its own pure function
/// (not left inline in `build`) so the two-value table is unit-testable
/// without mounting a widget tree.
fn disabled_content_opacity(enabled: bool) -> f32 {
    if enabled { 1.0 } else { DISABLED_CONTENT_ALPHA }
}

// Compile-time geometry invariant (not a runtime test — every side is
// `const`): the default padding must leave room for a positive content
// height inside the target container height.
const _: () = assert!(PADDING * 2.0 < CHIP_HEIGHT);

fn cancel_icon_data() -> IconData {
    IconData::new(DELETE_ICON_CANCEL_CODEPOINT).with_font_family("MaterialIcons")
}

fn clear_icon_data() -> IconData {
    IconData::new(DELETE_ICON_CLEAR_CODEPOINT).with_font_family("MaterialIcons")
}

/// Builds the PURE (never-combined) [`WidgetStates`] query set the M3
/// default tables' `disabled > selected > else` branch order resolves
/// against.
///
/// Flutter parity: this is the same shape the private
/// `navigation_destination_states` helper (`navigation_bar.rs`) establishes,
/// applied here for the identical reason — `_ChipDefaultsM3`/
/// `_FilterChipDefaultsM3`'s label/icon/delete-icon-color getters all read
/// as a plain `isEnabled ? (isSelected ? A : B) : C` ternary (disabled
/// always wins, selected only distinguishes within the enabled branch),
/// never a state genuinely carrying both `Selected` and `Disabled` at once
/// for those fields. A combined query would risk resolving the wrong branch
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

/// The label and delete-icon color default table. Flutter parity:
/// `_FilterChipDefaultsM3.labelStyle`/`.deleteIconColor` (`filter_chip.dart`,
/// oracle tag `3.44.0`) — the real three-way (`disabled`/`selected`/`else`)
/// table. `_ChipDefaultsM3.labelStyle`/`.deleteIconColor` (`chip.dart`) has
/// **no** `isSelected` member at all — it is a plain two-way `isEnabled ?
/// onSurfaceVariant : onSurface` ternary, since a bare `Chip` is never
/// selected. That two-way table's `enabled` value is identical to this
/// function's `unselected` branch, and [`Chip`]'s own [`chip_states`] call
/// site never sets [`WidgetState::Selected`] (see [`chip_icon_color_default`]'s
/// doc comment for the same note) — so reusing this one function for both
/// [`Chip`] and [`FilterChip`] is safe, but is not itself evidence that
/// `_ChipDefaultsM3` has a selected branch.
fn chip_content_color_default(states: WidgetStates, colors: &ColorScheme) -> Color {
    resolve_pure_chip_default(
        states,
        colors.on_surface,
        colors.on_secondary_container,
        colors.on_surface_variant,
    )
}

/// The avatar and checkmark icon color default table. Flutter parity:
/// `_FilterChipDefaultsM3.iconTheme.color`/`.checkmarkColor`
/// (`filter_chip.dart`, oracle tag `3.44.0`) — the real three-way
/// (`disabled`/`selected`/`else`) table. `_ChipDefaultsM3.iconTheme.color`
/// (`chip.dart`) has **no** `isSelected` member — it is a plain two-way
/// `isEnabled ? primary : onSurface` ternary. That two-way table's `enabled`
/// value is identical to this function's `unselected` branch, and
/// [`Chip`]'s own states never carry `Selected` (see [`chip_states`]'s call
/// site in [`Chip`]'s build), so the `selected` branch is simply
/// unreachable there rather than evidence `_ChipDefaultsM3` itself has one.
fn chip_icon_color_default(states: WidgetStates, colors: &ColorScheme) -> Color {
    resolve_pure_chip_default(
        states,
        colors.on_surface,
        colors.on_secondary_container,
        colors.primary,
    )
}

/// The container border default table. Flutter parity:
/// `_FilterChipDefaultsM3.side` (flat variant only — see the module docs),
/// `filter_chip.dart`, oracle tag `3.44.0` — the real `selected`-gated
/// table. `_ChipDefaultsM3.side` (`chip.dart`) has **no** `isSelected`
/// member — it is a plain two-way `isEnabled ? outlineVariant :
/// onSurface@12%` ternary, since a bare `Chip` is never selected; that
/// two-way table agrees with this function's `enabled`/`disabled`
/// (unselected) branches, so [`Chip`] safely calls this same function with
/// `selected` pinned to `false`.
///
/// **Combined-state, not pure**: `selected` is checked FIRST and wins
/// unconditionally (a selected chip's side is transparent whether or not it
/// is also disabled) — unlike [`chip_content_color_default`]/
/// [`chip_icon_color_default`], `disabled` does NOT take priority here. A
/// disabled-and-selected chip's side is `transparent` (the selected
/// branch), not the disabled-only `onSurface@12%` a pure `disabled`-first
/// query would give — confirmed against a deliberately `disabled`-first
/// branch order, which fails
/// `default_side_selected_and_disabled_stays_transparent_not_the_disabled_color`.
/// Because of this real branch-order difference, `side` is resolved from
/// plain `(bool, bool)` parameters rather than a [`WidgetStates`] query —
/// see the module docs' "`ChipThemeData`: plain overrides" section.
fn chip_default_side(selected: bool, enabled: bool, colors: &ColorScheme) -> BorderSide<Pixels> {
    if selected {
        BorderSide::new(Color::TRANSPARENT, px(1.0), BorderStyle::Solid)
    } else if enabled {
        BorderSide::new(colors.outline_variant, px(1.0), BorderStyle::Solid)
    } else {
        BorderSide::new(
            colors.on_surface.with_opacity(0.12),
            px(1.0),
            BorderStyle::Solid,
        )
    }
}

/// The default container shape: an 8dp rounded rectangle. Flutter parity:
/// `_ChipDefaultsM3`/`_FilterChipDefaultsM3`'s constructor `shape:`.
fn chip_default_shape() -> MaterialShape {
    use flui_types::styling::BorderRadius;
    MaterialShape::RoundedRect(BorderRadius::all(flui_types::geometry::Radius::circular(
        px(CORNER_RADIUS),
    )))
}

/// The default container padding: `EdgeInsets.all(8.0)`. Flutter parity:
/// `_ChipDefaultsM3.padding`/`_FilterChipDefaultsM3.padding`.
fn chip_default_padding() -> EdgeInsets {
    EdgeInsets::all(px(PADDING))
}

/// The default label padding: `EdgeInsets.symmetric(horizontal: 8.0)` — the
/// text-scale-1x tier (see the module doc on [`LABEL_PADDING_HORIZONTAL`]).
fn chip_default_label_padding() -> EdgeInsets {
    EdgeInsets::symmetric(px(0.0), px(LABEL_PADDING_HORIZONTAL))
}

/// The container's minimum content height (excludes `padding`, includes
/// `label_padding`'s own vertical inset). Flutter parity: `_RenderChip
/// ._computeSizes`'s `contentSize` floor, `math.max(_kChipHeight -
/// theme.padding.vertical + theme.labelPadding.vertical, ...)` (`chip.dart`
/// `:1953-1956`, oracle tag `3.44.0`) — narrowed to just the floor term
/// (the `rawLabelSize.height + labelPadding.vertical` alternative is the
/// label's own intrinsic height, which this substrate's plain
/// `ConstrainedBox` + `Row` composition already accommodates by growing
/// past the floor when the label needs more room, without needing to
/// compute `rawLabelSize` up front).
fn chip_content_min_height(padding: EdgeInsets, label_padding: EdgeInsets) -> Pixels {
    let floor = CHIP_HEIGHT - padding.vertical_total().get() + label_padding.vertical_total().get();
    px(floor.max(0.0))
}

/// A tap/press callback taking no arguments. `Rc`-based (owner-local, per
/// ADR-0027) — matches [`InkWell::on_tap`]'s own callback shape.
type ChipTapCallback = Rc<dyn Fn()>;

/// A selection-change callback: the next selected value. `Rc`-based, same
/// shape as [`ChipTapCallback`].
type FilterChipSelectCallback = Rc<dyn Fn(bool)>;

/// A Material Design chip: a compact label with an optional leading avatar
/// and trailing delete affordance, outlined and unfilled by default.
///
/// See the module docs for the V1 scope (a reduced `RawChip`, an
/// [`Chip::on_pressed`] widening beyond the oracle's own non-interactive
/// `Chip`) and named deferrals.
///
/// ```rust
/// use flui_material::Chip;
/// use flui_widgets::Text;
///
/// let _info = Chip::new(Text::new("Tag"));
/// let _pressable = Chip::new(Text::new("Tag")).on_pressed(|| {});
/// let _deletable = Chip::new(Text::new("Tag")).on_deleted(|| {});
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
    pub fn on_pressed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(Rc::new(callback));
        self
    }

    /// Sets the delete handler. Presence of a handler is what shows the
    /// trailing delete icon. Flutter parity: `Chip.onDeleted`.
    #[must_use]
    pub fn on_deleted(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_deleted = Some(Rc::new(callback));
        self
    }

    /// Sets whether this chip responds to interaction. Defaults to `true`.
    /// Flutter parity: `RawChip.isEnabled` (the oracle's own `Chip` never
    /// varies this — see the module docs).
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
            .clone()
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
                delete_button = delete_button.on_tap(move || on_deleted());
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
            ink_well = ink_well.on_tap(move || {
                if let Some(handler) = &on_pressed {
                    handler();
                }
            });
        }

        let container = CustomPaint::new()
            .foreground_painter(
                Arc::new(ChipBorderPainter { side, shape }) as Arc<dyn CustomPainter>
            )
            .child(Material::new(background_color).shape(shape).child(ink_well));

        wrap_local_gesture_arena(
            Semantics::new()
                .button(self.on_pressed.is_some())
                .enabled(self.enabled)
                .child(container),
        )
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
/// use flui_widgets::Text;
///
/// let _chip = FilterChip::new(Text::new("Vegetarian"))
///     .selected(true)
///     .on_selected(|_next| { /* ... */ });
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
    /// interactive — Flutter parity: `FilterChip.isEnabled => onSelected !=
    /// null`.
    #[must_use]
    pub fn on_selected(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_selected = Some(Rc::new(callback));
        self
    }

    /// Sets the delete handler. Presence of a handler is what shows the
    /// trailing delete icon.
    #[must_use]
    pub fn on_deleted(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_deleted = Some(Rc::new(callback));
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
/// Flutter parity: `_layoutAvatar`'s `showCheckmark`/`showAvatar` branch
/// (`chip.dart`, oracle tag `3.44.0`), reduced to "checkmark wins outright
/// when selected" per the module docs' "Selection" section (the oracle
/// itself keeps both children present and blends between them; V1 shows
/// exactly one).
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
/// module docs). Flutter parity: `_FilterChipDefaultsM3.color`
/// (`filter_chip.dart` `:341-361`, oracle tag `3.44.0`).
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
        // above), matching `_FilterChipDefaultsM3.color`'s own `null`
        // fall-through for both unselected branches.
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
            .clone()
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
                    .size(Size::new(px(CHIP_ICON_SIZE), px(CHIP_ICON_SIZE)))
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
                delete_button = delete_button.on_tap(move || on_deleted());
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
            ink_well = ink_well.on_tap(move || {
                if let Some(handler) = &on_selected {
                    handler(!selected);
                }
            });
        }

        let container = CustomPaint::new()
            .foreground_painter(
                Arc::new(ChipBorderPainter { side, shape }) as Arc<dyn CustomPainter>
            )
            .child(Material::new(background_color).shape(shape).child(ink_well));

        wrap_local_gesture_arena(
            Semantics::new()
                .selected(selected)
                .button(true)
                .enabled(enabled)
                .child(container),
        )
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
        px(0.0),
        Pixels::INFINITY,
        chip_content_min_height(padding, label_padding),
        Pixels::INFINITY,
    )
}

/// Wraps a chip's whole built subtree in a fresh, local
/// [`flui_widgets::GestureArenaScope`] so its two nested [`InkWell`]s (the
/// delete button, inside the outer chip-body [`InkWell`]) genuinely compete
/// for a tap instead of both firing.
///
/// [`flui_widgets::GestureDetector`]'s own module docs document the
/// fallback this closes a real gap in: with **no** ambient
/// `GestureArenaScope` above it, a `GestureDetector` builds its recognizers
/// against a *private* arena it closes itself — correct in isolation, but
/// when TWO such standalone detectors both sit on the same hit-test path
/// (exactly what a delete icon nested inside a tappable chip produces), each
/// resolves its own tap independently, so a tap on the delete icon fires
/// BOTH `on_deleted` and the chip's own `on_pressed`/`on_selected`. Neither
/// `flui-app`'s binding nor any ancestor this crate's components mount
/// installs a `GestureArenaScope` anywhere today — confirmed by grepping the
/// workspace for it outside `flui-widgets` itself — so a real app has this
/// exact double-fire bug for any nested tap targets, not just this one.
/// Fixing that project-wide is out of scope here; this closes it locally,
/// the same way any composed widget with more than one nested tap target
/// must.
///
/// Constructing a fresh [`flui_interaction::arena::GestureArena::new`] on
/// every `build()` call is safe despite [`Chip`]/[`FilterChip`] being
/// stateless (no persistent slot to cache it in): a descendant
/// `GestureDetector` reads its ambient scope exactly once, in its own
/// `init_state` (first mount) — see that type's own "Arena acquisition" doc
/// section — never on a later rebuild. So only the arena captured at first
/// mount is ever actually used; every subsequent rebuild's
/// freshly-constructed (and immediately discarded)
/// [`flui_interaction::arena::GestureArena`] is inert. This mirrors the
/// same "cheap to reconstruct, only the first read matters" contract
/// `GestureArenaScope::update_should_notify` documents (always `false`)
/// already relies on.
fn wrap_local_gesture_arena(child: impl IntoView) -> flui_widgets::GestureArenaScope {
    flui_widgets::GestureArenaScope::new(flui_interaction::arena::GestureArena::new(), child)
}

/// Strokes a chip's resolved [`MaterialShape`] as an inset ring — the same
/// `Canvas::draw_drrect`-between-two-insets approach the private
/// `CheckboxPainter` (`checkbox.rs`) uses for its own stroked box, see the
/// module docs' "Container" section for why this substitutes for
/// [`Material`]'s own missing border-side paint path.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChipBorderPainter {
    side: BorderSide<Pixels>,
    shape: MaterialShape,
}

impl CustomPainter for ChipBorderPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !self.side.style.is_solid() || self.side.width.get() <= 0.0 {
            return;
        }
        let outer = self.shape.to_rrect(size);
        let inner = outer.inflate(px(-self.side.width.get()));
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
/// leading slot while selected — a direct port of the oracle's own
/// relative-coordinate stroke path at its fully-settled shape, scaled down
/// and centered exactly as the oracle does. Flutter parity:
/// `_RenderChip._paintSelectionOverlay`/`._paintCheck` (`chip.dart`
/// `:2174-2195`/`:2125-2172`, oracle tag `3.44.0`) evaluated at `t == 1.0`
/// (the full `start -> mid -> end` polyline, no animated partial stroke) —
/// the same "real stroked geometry, `t == 1.0`" precedent the private
/// `CheckboxPainter`'s (`checkbox.rs`) own module docs describe, using the
/// identical relative coordinates (`0.15, 0.45` / `0.4, 0.7` / `0.85,
/// 0.25`) that painter's own `draw_checkmark` uses — but, unlike that full-
/// cell checkmark, scaled to `checkSize = avatar.size.height * 0.75` and
/// offset by `avatar.size.height * 0.125` on both axes ("a little smaller
/// than the avatar", `_paintSelectionOverlay`'s own comment, `:2188-2192`):
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
        let cell = size.height.get();
        // Flutter parity: `_kCheckmarkStrokeWidth * avatar.size.height /
        // 24.0` (`chip.dart`) — the FULL cell height, not `check_size`.
        let stroke_width = 2.0 * cell / 24.0;
        let paint = Paint::stroke(self.color, stroke_width);

        // Flutter parity: `checkSize = avatar.size.height * 0.75` and the
        // `avatar.size.height * 0.125` origin offset on both axes
        // (`_paintSelectionOverlay`, `chip.dart` `:2188-2192`) — see this
        // struct's own doc comment.
        let check_size = cell * 0.75;
        let origin_offset = cell * 0.125;
        let point = |dx: f32, dy: f32| Point::new(px(origin_offset + dx), px(origin_offset + dy));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn light() -> ColorScheme {
        ColorScheme::light()
    }

    // ------------------------------------------------------------------
    // Construction / builder surface
    // ------------------------------------------------------------------

    #[test]
    fn chip_new_leaves_every_override_unset_enabled_and_not_interactive() {
        let chip = Chip::new(flui_widgets::Text::new("Tag"));
        assert!(chip.avatar.is_none());
        assert!(chip.on_pressed.is_none());
        assert!(chip.on_deleted.is_none());
        assert!(chip.enabled);
        assert!(!chip.is_pressable());
        assert!(!chip.has_delete_button());
    }

    #[test]
    fn chip_on_pressed_makes_the_chip_pressable() {
        let chip = Chip::new(flui_widgets::Text::new("Tag")).on_pressed(|| {});
        assert!(chip.is_pressable());
    }

    #[test]
    fn chip_disabled_is_never_pressable_even_with_a_handler() {
        let chip = Chip::new(flui_widgets::Text::new("Tag"))
            .on_pressed(|| {})
            .enabled(false);
        assert!(!chip.is_pressable());
    }

    #[test]
    fn chip_on_deleted_shows_the_delete_button() {
        let chip = Chip::new(flui_widgets::Text::new("Tag")).on_deleted(|| {});
        assert!(chip.has_delete_button());
    }

    #[test]
    fn filter_chip_new_is_unselected_and_disabled() {
        let chip = FilterChip::new(flui_widgets::Text::new("Tag"));
        assert!(!chip.selected);
        assert!(!chip.is_enabled());
        assert!(chip.avatar.is_none());
        assert!(chip.on_deleted.is_none());
    }

    #[test]
    fn filter_chip_on_selected_makes_it_enabled() {
        let chip = FilterChip::new(flui_widgets::Text::new("Tag")).on_selected(|_| {});
        assert!(chip.is_enabled());
    }

    // ------------------------------------------------------------------
    // chip_states — pure query set (the NavigationBar-lesson regression)
    // ------------------------------------------------------------------

    #[test]
    fn chip_states_selected_and_enabled_carries_only_selected() {
        let states = chip_states(true, true);
        assert!(states.contains_state(WidgetState::Selected));
        assert!(!states.contains_state(WidgetState::Disabled));
    }

    #[test]
    fn chip_states_unselected_and_disabled_carries_only_disabled() {
        let states = chip_states(false, false);
        assert!(states.contains_state(WidgetState::Disabled));
        assert!(!states.contains_state(WidgetState::Selected));
    }

    /// Regression: a chip that is BOTH selected and disabled must query
    /// with the PURE `{Disabled}` set, never a combined `{Selected,
    /// Disabled}` one — mirrors
    /// `navigation_bar::tests::states_selected_and_disabled_carries_only_disabled_not_both`.
    #[test]
    fn chip_states_selected_and_disabled_carries_only_disabled_not_both() {
        let states = chip_states(true, false);
        assert!(states.contains_state(WidgetState::Disabled));
        assert!(
            !states.contains_state(WidgetState::Selected),
            "a disabled chip's query states must never also carry Selected, even when the chip \
             is selected — combining them would let a theme Map resolve the wrong (selected) \
             entry for a disabled chip",
        );
    }

    /// Mutation-run: reverting `chip_states` to combine both flags whenever
    /// both apply was confirmed to make this test fail — the combined set
    /// satisfies `WidgetStateConstraint::Is(Selected)` (the first entry
    /// below), so the broken version resolves `selected_style` for a
    /// disabled+selected chip instead of `disabled_style`.
    #[test]
    fn theme_map_ordered_selected_before_disabled_still_resolves_disabled_for_a_disabled_selected_chip()
     {
        use flui_widgets::{WidgetStateConstraint, WidgetStateProperty};

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

        let states = chip_states(true, false); // selected AND disabled
        let resolved = theme.resolve(&states);

        assert_eq!(
            resolved,
            Some(disabled_style),
            "a disabled chip must resolve a theme Map's Disabled entry even when it is ALSO \
             selected — chip_states never queries a combined {{selected, disabled}} set, so a \
             Map ordered Selected-before-Disabled still gives the disabled result",
        );
    }

    // ------------------------------------------------------------------
    // chip_content_color_default / chip_icon_color_default — branch order
    // ------------------------------------------------------------------

    #[test]
    fn content_color_default_unselected_enabled_is_on_surface_variant() {
        assert_eq!(
            chip_content_color_default(WidgetStates::NONE, &light()),
            light().on_surface_variant
        );
    }

    #[test]
    fn content_color_default_selected_is_on_secondary_container() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            chip_content_color_default(states, &light()),
            light().on_secondary_container
        );
    }

    #[test]
    fn content_color_default_disabled_wins_over_selected() {
        // Branch-order pin: `chip_states` never actually produces a
        // combined set (see the regression tests above), but
        // `resolve_pure_chip_default` itself must still check `disabled`
        // before `selected` in case a future caller feeds it one directly.
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        assert_eq!(
            chip_content_color_default(states, &light()),
            light().on_surface
        );
    }

    #[test]
    fn icon_color_default_unselected_enabled_is_primary() {
        assert_eq!(
            chip_icon_color_default(WidgetStates::NONE, &light()),
            light().primary
        );
    }

    #[test]
    fn icon_color_default_selected_is_on_secondary_container() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            chip_icon_color_default(states, &light()),
            light().on_secondary_container
        );
    }

    #[test]
    fn icon_color_default_disabled_is_on_surface() {
        let states = WidgetStates::from(WidgetState::Disabled);
        assert_eq!(
            chip_icon_color_default(states, &light()),
            light().on_surface
        );
    }

    // ------------------------------------------------------------------
    // chip_default_side — Selected wins over Disabled (combined, not pure)
    // ------------------------------------------------------------------

    #[test]
    fn default_side_unselected_enabled_is_outline_variant() {
        let side = chip_default_side(false, true, &light());
        assert_eq!(side.color, light().outline_variant);
        assert_eq!(side.width, px(1.0));
    }

    #[test]
    fn default_side_unselected_disabled_is_faded_on_surface() {
        let side = chip_default_side(false, false, &light());
        assert_eq!(side.color, light().on_surface.with_opacity(0.12));
    }

    #[test]
    fn default_side_selected_enabled_is_transparent() {
        let side = chip_default_side(true, true, &light());
        assert_eq!(side.color, Color::TRANSPARENT);
    }

    /// The whole point of `chip_default_side` taking plain bools instead of
    /// a `WidgetStates` set: `selected` wins over `disabled` outright, the
    /// opposite priority from `chip_content_color_default`/
    /// `chip_icon_color_default`. Mutation-run: swapping this function to
    /// check `enabled` before `selected` (a `disabled`-first branch order)
    /// was confirmed to make this test fail — it would resolve
    /// `onSurface@12%` instead of `transparent`.
    #[test]
    fn default_side_selected_and_disabled_stays_transparent_not_the_disabled_color() {
        let side = chip_default_side(true, false, &light());
        assert_eq!(
            side.color,
            Color::TRANSPARENT,
            "selected must win over disabled for `side` — the oracle's own `!isSelected` gate \
             short-circuits before `isEnabled` is ever consulted",
        );
    }

    // ------------------------------------------------------------------
    // filter_chip_default_background_color — the genuine 3-way branch
    // ------------------------------------------------------------------

    #[test]
    fn default_background_unselected_enabled_is_transparent() {
        assert_eq!(
            filter_chip_default_background_color(false, true, &light()),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn default_background_selected_enabled_is_secondary_container() {
        assert_eq!(
            filter_chip_default_background_color(true, true, &light()),
            light().secondary_container
        );
    }

    #[test]
    fn default_background_unselected_disabled_is_transparent() {
        assert_eq!(
            filter_chip_default_background_color(false, false, &light()),
            Color::TRANSPARENT
        );
    }

    /// The genuine third branch: disabled-and-selected is NEITHER plain
    /// `disabled` (`transparent`) NOR plain `selected`
    /// (`secondaryContainer`) — a distinct color only a combined query can
    /// produce. Mutation-run: collapsing this arm to fall through to either
    /// neighbor was confirmed to make this test fail.
    #[test]
    fn default_background_selected_and_disabled_is_its_own_distinct_value() {
        let combined = filter_chip_default_background_color(true, false, &light());
        assert_eq!(combined, light().on_surface.with_opacity(0.12));
        assert_ne!(
            combined,
            filter_chip_default_background_color(false, false, &light())
        );
        assert_ne!(
            combined,
            filter_chip_default_background_color(true, true, &light())
        );
    }

    // ------------------------------------------------------------------
    // filter_chip_leading_content — the avatar/checkmark swap
    // ------------------------------------------------------------------

    #[test]
    fn leading_content_selected_is_always_checkmark() {
        assert_eq!(
            filter_chip_leading_content(true, true),
            FilterChipLeading::Checkmark
        );
        assert_eq!(
            filter_chip_leading_content(true, false),
            FilterChipLeading::Checkmark
        );
    }

    #[test]
    fn leading_content_unselected_with_avatar_shows_the_avatar() {
        assert_eq!(
            filter_chip_leading_content(false, true),
            FilterChipLeading::Avatar
        );
    }

    #[test]
    fn leading_content_unselected_without_avatar_shows_nothing() {
        assert_eq!(
            filter_chip_leading_content(false, false),
            FilterChipLeading::None
        );
    }

    // ------------------------------------------------------------------
    // Geometry
    // ------------------------------------------------------------------

    #[test]
    fn content_min_height_with_default_padding_is_16() {
        let height = chip_content_min_height(chip_default_padding(), chip_default_label_padding());
        assert_eq!(height, px(CHIP_HEIGHT - 2.0 * PADDING));
    }

    #[test]
    fn content_min_height_never_goes_negative_under_oversized_padding() {
        let oversized = EdgeInsets::all(px(100.0));
        let height = chip_content_min_height(oversized, chip_default_label_padding());
        assert_eq!(height, px(0.0));
    }

    // ------------------------------------------------------------------
    // disabled_content_opacity — the steady-state 38% alpha, not a fade
    // ------------------------------------------------------------------

    #[test]
    fn disabled_content_opacity_enabled_is_fully_opaque() {
        assert_eq!(disabled_content_opacity(true), 1.0);
    }

    /// Flutter parity: `_kDisabledAlpha` (`0x61`, `chip.dart`) as a `0.0..=1.0`
    /// fraction. Mutation-run: hardcoding this to `1.0` (i.e. dropping the
    /// disabled dimming entirely, the pre-fix shape) was confirmed to make
    /// this test fail.
    #[test]
    fn disabled_content_opacity_disabled_is_the_m3_disabled_alpha() {
        let opacity = disabled_content_opacity(false);
        assert!((opacity - 0x61 as f32 / 255.0).abs() < f32::EPSILON);
        assert_ne!(opacity, 1.0);
    }

    #[test]
    fn default_shape_is_an_8dp_rounded_rectangle() {
        let size = Size::new(px(80.0), px(CHIP_HEIGHT));
        let rrect = chip_default_shape().to_rrect(size);
        assert_eq!(
            rrect.top_left,
            flui_types::geometry::Radius::circular(px(CORNER_RADIUS))
        );
    }

    #[test]
    fn default_padding_is_8dp_all_sides() {
        let padding = chip_default_padding();
        assert_eq!(padding.top, px(PADDING));
        assert_eq!(padding.left, px(PADDING));
    }

    #[test]
    fn default_label_padding_is_horizontal_only() {
        let padding = chip_default_label_padding();
        assert_eq!(padding.top, px(0.0));
        assert_eq!(padding.left, px(LABEL_PADDING_HORIZONTAL));
    }

    // Theme tier beats default (the widget/theme/default cascade for
    // `label_color`/`side`) is proven through a REAL mount + `Chip::build`
    // call in `tests/chip.rs` (`theme_label_color_reaches_the_mounted_paragraph_beating_the_default`/
    // `theme_side_reaches_the_mounted_border_painter_beating_the_default`,
    // plus their `no_theme_override_paints_the_m3_default_*` companions),
    // not here: `ChipThemeData`'s fields are plain overrides (see the
    // module docs), so a unit-level probe of the cascade can only
    // re-implement `Option::or_else`/`unwrap_or_else` inline — exercising
    // `std::option`, not `chip.rs`'s own `build()` wiring. A previous
    // version of this test module carried exactly that vacuous shape; it
    // was replaced, not merely supplemented, because it could not fail
    // against a real wiring bug (confirmed: mutating `Chip::build` to drop
    // the `chip_theme.label_color`/`.side` read entirely left the old
    // in-module tests green).

    // ------------------------------------------------------------------
    // Painters
    // ------------------------------------------------------------------

    #[test]
    fn border_painter_draws_nothing_for_a_zero_width_side() {
        use flui_painting::DisplayListCore;

        let painter = ChipBorderPainter {
            side: BorderSide::new(Color::BLACK, px(0.0), BorderStyle::Solid),
            shape: chip_default_shape(),
        };
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Size::new(px(80.0), px(32.0)));
        assert!(canvas.display_list().is_empty());
    }

    #[test]
    fn border_painter_draws_a_ring_for_a_visible_side() {
        use flui_painting::display_list::DrawCommand;

        let painter = ChipBorderPainter {
            side: BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid),
            shape: chip_default_shape(),
        };
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Size::new(px(80.0), px(32.0)));
        assert!(
            canvas
                .display_list()
                .iter()
                .any(|command| matches!(command, DrawCommand::DrawDRRect { .. }))
        );
    }

    #[test]
    fn border_painter_should_repaint_is_true_when_the_side_changes() {
        let old = ChipBorderPainter {
            side: BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid),
            shape: chip_default_shape(),
        };
        let mut new = old;
        new.side.color = Color::WHITE;
        assert!(new.should_repaint(&old));
    }

    #[test]
    fn checkmark_painter_draws_a_path() {
        use flui_painting::display_list::DrawCommand;

        let painter = ChipCheckmarkPainter {
            color: Color::BLACK,
        };
        let mut canvas = Canvas::new();
        painter.paint(
            &mut canvas,
            Size::new(px(CHIP_ICON_SIZE), px(CHIP_ICON_SIZE)),
        );
        assert!(
            canvas
                .display_list()
                .iter()
                .any(|command| matches!(command, DrawCommand::DrawPath { .. }))
        );
    }

    /// Pins the oracle's `checkSize = avatar.size.height * 0.75` scale-down
    /// and `avatar.size.height * 0.125` origin offset
    /// (`_paintSelectionOverlay`, `chip.dart` `:2188-2192`, oracle tag
    /// `3.44.0`) — the mark must sit strictly inside a `0.125..0.875`
    /// (75%-wide, centered) window of the full cell, not fill it edge to
    /// edge. Mutation-run: reverting `ChipCheckmarkPainter::paint` to the
    /// pre-fix version (`check_size`/`origin_offset` both dropped, the full
    /// `cell` used directly, matching the original shipped bug) was
    /// confirmed to make this test fail: `min x: got 2.7, expected 4.275`
    /// (the mutant's un-scaled `0.15 * 18.0` vs. this test's scaled-and-
    /// offset expectation) — the first of the four bound assertions below,
    /// which stops the run before the other three (`max_x = 15.3` vs.
    /// `13.725`, etc.) are reached, but the same un-scaled arithmetic
    /// diverges from every one of them identically.
    #[test]
    fn checkmark_painter_scales_to_75_percent_and_centers_within_the_cell() {
        use flui_painting::display_list::DrawCommand;

        let cell = CHIP_ICON_SIZE;
        let painter = ChipCheckmarkPainter {
            color: Color::BLACK,
        };
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Size::new(px(cell), px(cell)));

        let mut path = canvas
            .display_list()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawPath { path, .. } => Some(path.clone()),
                _ => None,
            })
            .expect("checkmark painter must emit a DrawPath command");
        let bounds = path.bounds();

        let check_size = cell * 0.75;
        let origin_offset = cell * 0.125;
        // The oracle's three stroke points: start (0.15, 0.45), mid (0.4,
        // 0.7), end (0.85, 0.25) — the tight bounding box over all three is
        // exactly [start.x, end.x] x [end.y, mid.y] (`0.15`/`0.85` bound x,
        // `0.25`/`0.7` bound y).
        let expected_min_x = origin_offset + check_size * 0.15;
        let expected_max_x = origin_offset + check_size * 0.85;
        let expected_min_y = origin_offset + check_size * 0.25;
        let expected_max_y = origin_offset + check_size * 0.7;

        let epsilon = 0.01;
        assert!(
            (bounds.min_x().get() - expected_min_x).abs() < epsilon,
            "min x: got {}, expected {expected_min_x}",
            bounds.min_x().get()
        );
        assert!(
            (bounds.max_x().get() - expected_max_x).abs() < epsilon,
            "max x: got {}, expected {expected_max_x}",
            bounds.max_x().get()
        );
        assert!(
            (bounds.min_y().get() - expected_min_y).abs() < epsilon,
            "min y: got {}, expected {expected_min_y}",
            bounds.min_y().get()
        );
        assert!(
            (bounds.max_y().get() - expected_max_y).abs() < epsilon,
            "max y: got {}, expected {expected_max_y}",
            bounds.max_y().get()
        );

        // The whole mark must stay strictly inside the cell — never
        // touching the full-cell edges the pre-fix version reached.
        assert!(bounds.max_x().get() < cell);
        assert!(bounds.max_y().get() < cell);
        assert!(bounds.min_x().get() > 0.0);
        assert!(bounds.min_y().get() > 0.0);
    }

    #[test]
    fn checkmark_painter_should_repaint_is_false_for_an_identical_delegate() {
        let old = ChipCheckmarkPainter {
            color: Color::BLACK,
        };
        let new = ChipCheckmarkPainter {
            color: Color::BLACK,
        };
        assert!(!new.should_repaint(&old));
    }
}
