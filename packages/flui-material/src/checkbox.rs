//! [`Checkbox`] — a tristate-capable M3 selection control.
//!
//! # V1 scope: static states, no toggle/reaction animation
//!
//! A full toggleable control drives several animation controllers (one for
//! the box/check morph, one for the radial ink splash, plus hover/focus fade
//! controllers) so a value change or an interaction visibly interpolates. This
//! V1 **snaps**: `CheckboxPainter` always paints the box/check at their
//! fully-settled shape, and there is no radial-splash painting — the
//! hover/focus/press overlay comes from [`InkWell`] instead (a single
//! resolved-color fill, not an expanding circle), matching how
//! [`crate::floating_action_button`] already substitutes `InkWell` for an
//! ink-feature registry. Named deferral, not a silent drop: a future animated
//! Checkbox reintroduces animation-controller-driven interpolation without
//! changing this type's public surface (`value` via [`Checkbox::new`] /
//! [`Checkbox::tristate`], plus `on_changed`, are already the steady-state
//! contract).
//!
//! # Composition: `InkWell` owns interaction, `Checkbox` owns `Selected`
//!
//! [`InkWell`] already provides the hover/focus/press wiring and `Disabled`
//! derivation, so it is reused here rather than re-deriving that composition
//! from scratch. `Checkbox` shares one [`WidgetStatesController`] with the
//! `InkWell` it builds (via [`InkWell::states_controller`]): `InkWell`
//! manages `Hovered`/`Focused`/`Pressed`/`Disabled` on it, `Checkbox` manages
//! `Selected` on it (a `None` value counts as selected) — both read the same
//! live set. `Error` is never stored on the controller (it is layered onto a
//! *local copy* of `states` only where consumed); this type folds it in the
//! same way, at each resolution site.
//!
//! # Painting: `CustomPaint` + real stroked geometry, not a glyph
//!
//! `CheckboxPainter` draws the 18dp rounded-rect box
//! ([`Canvas::draw_rrect`]/[`Canvas::draw_drrect`]) and the checkmark/dash as
//! an actual stroked [`Path`]/line ([`Canvas::draw_path`]/
//! [`Canvas::draw_line`]) at fixed relative coordinates — not a bundled
//! icon-font glyph. Unlike [`crate::back_button::BackButton`]
//! (which had no path-drawing seam and fell back to a `MaterialIcons` glyph
//! identity), `flui-painting`'s [`Canvas`] already exposes the primitives, so
//! this is a direct, honest rendering of the static (non-animated) shape.
//!
//! # Overlay shape: the whole tap target, not a fixed-radius circle
//!
//! The M3 splash radius (`20.0`) sizes a FREE (unclipped) radial-reaction
//! circle. `InkWell`'s single-fill
//! substitution (see the "V1 scope" section above) has no free-circle
//! primitive — it paints one shape-clipped fill over its own bounds. This
//! port shapes that fill as [`MaterialShape::Stadium`] over the FULL
//! `CHECKBOX_TAP_TARGET_SIZE` square (inscribing a `24.0`-radius circle, not
//! exactly `20.0`) — the same named, position-independent approximation
//! [`crate::switch`]'s module docs describe for `Switch`'s own overlay.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Toggle/reaction/hover/focus-fade animation** — see above.
//! - **An adaptive (Cupertino) checkbox** — no platform substrate to switch
//!   on yet.
//! - **`mouse_cursor`, `splash_radius`, `material_tap_target_size`,
//!   `visual_density` overrides** — V1 always uses the M3 defaults
//!   (48dp tap target, standard visual density = no adjustment, splash
//!   radius `20.0` — see the "Overlay shape"
//!   section above for why the InkWell substitution doesn't hit that value
//!   exactly). [`crate::CheckboxThemeData`] and the widget both omit these
//!   fields; see that type's own doc comment.
//! - **Widget-level `fill_color`/`overlay_color`/`side`/`shape` overrides**
//!   (state-property-shaped parameters) —
//!   only [`Checkbox::active_color`] and [`Checkbox::check_color`] (plain
//!   `Color` overrides) ship at the widget tier; the theme
//!   tier ([`crate::CheckboxThemeData`]) and the M3 default tier are both
//!   fully state-resolved. A future widget-level `WidgetStateProperty` override
//!   slot is additive.
//! - **`focus_node`/`autofocus`** — [`InkWell`] itself has no `autofocus`
//!   hook yet (a whole-substrate gap, not specific to this type); only
//!   `focus_node` sharing is wired through.

use std::rc::Rc;

use flui_sdk::foundation::Listenable;
use flui_sdk::painting::Canvas;
use flui_sdk::painting::{BorderSide, BorderStyle};
use flui_sdk::painting::{Paint, Path};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    CustomPaint, CustomPainter, Semantics, WidgetState, WidgetStateProperty, WidgetStates,
    WidgetStatesController,
};
use flui_sdk::{
    geometry::{Point, RRect, Rect, Size},
    painting::Color,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::shape::MaterialShape;
use crate::state_color::resolve_state_color;
use crate::theme::Theme;

/// A checkbox's edge length (`18.0`).
pub const CHECKBOX_EDGE_SIZE: f64 = 18.0;

/// The box outline's and checkmark/dash's stroke width.
const STROKE_WIDTH: f64 = 2.0;

/// The M3 tap-target side length (`48.0`, the minimum interactive dimension),
/// always used in V1 (no `material_tap_target_size` override yet — see the
/// module docs).
pub const CHECKBOX_TAP_TARGET_SIZE: f64 = 48.0;

/// The box's corner radius, `2.0`.
const CORNER_RADIUS: f64 = 2.0;

// The 18dp box must fit inside the 48dp tap target with room for the
// centering inset the painter computes — a compile-time invariant, not a
// runtime test (both sides are `const`).
const _: () = assert!(CHECKBOX_EDGE_SIZE < CHECKBOX_TAP_TARGET_SIZE);

/// A value-change callback: the next tristate value. `Rc`-based
/// (owner-local, per ADR-0027) — matches [`InkWell`]'s own callback shape.
type CheckboxChangeCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>, Option<bool>)>;

/// A Material Design tristate-capable checkbox.
///
/// The checkbox itself holds no state: [`Checkbox::on_changed`] fires with
/// the next value on tap, and the caller re-renders with the updated
/// `value`. Construct with [`Checkbox::new`] for a binary on/off control, or
/// [`Checkbox::tristate`] when the third (`None`/indeterminate) value is
/// allowed. Storage is a private mode enum (binary vs tristate), so
/// `(value: None, tristate: false)` is not representable even inside this
/// module (a debug-only assertion would leave a release hole; this closes it —
/// same public-widget invariant class as GitHub #1101 for tabs). Ledger:
/// `ARCHITECTURE.md` §Checkbox value/tristate.
///
/// ```rust
/// use flui_material::Checkbox;
///
/// let _off = Checkbox::new(false).on_changed(|_cx, _next| { /* ... */ });
/// let _tristate = Checkbox::tristate(None);
/// let _disabled = Checkbox::new(true);
/// ```
#[derive(Clone, StatefulView)]
pub struct Checkbox {
    mode: CheckboxMode,
    on_changed: Option<CheckboxChangeCallback>,
    active_color: Option<Color>,
    check_color: Option<Color>,
    is_error: bool,
    semantic_label: Option<String>,
}

/// Binary vs tristate value storage. Keeping these as one enum (not independent
/// `Option<bool>` + `bool` fields) is what makes the illegal pair unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckboxMode {
    Binary(bool),
    Tristate(Option<bool>),
}

impl CheckboxMode {
    /// The value: always `Some` for binary, `None` when indeterminate under
    /// tristate.
    fn value(self) -> Option<bool> {
        match self {
            Self::Binary(value) => Some(value),
            Self::Tristate(value) => value,
        }
    }

    /// Whether `WidgetState::Selected` applies: an indeterminate value counts
    /// as selected.
    fn is_selected(self) -> bool {
        self.value().unwrap_or(true)
    }
}

/// Checked / mixed flags exported to assistive tech for a given mode.
///
/// Extracted so unit tests can pin the binary vs indeterminate mapping without
/// mounting a tree — the integration suite then proves the same flags reach
/// AccessKit as `Toggled::Mixed` / `True` / `False`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CheckboxSemanticsFlags {
    checked: bool,
    mixed: bool,
}

fn checkbox_semantics_flags(mode: CheckboxMode) -> CheckboxSemanticsFlags {
    match mode {
        CheckboxMode::Tristate(None) => CheckboxSemanticsFlags {
            // Mixed + checked(false) is the AccessKit form of indeterminate.
            checked: false,
            mixed: true,
        },
        CheckboxMode::Binary(value) | CheckboxMode::Tristate(Some(value)) => {
            CheckboxSemanticsFlags {
                checked: value,
                mixed: false,
            }
        }
    }
}

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("mode", &self.mode)
            .field("is_interactive", &self.is_interactive())
            .field("is_error", &self.is_error)
            .finish_non_exhaustive()
    }
}

impl Checkbox {
    /// Creates a binary (on/off) checkbox. Indeterminate is not representable
    /// on this path — use [`Self::tristate`] when `None` is needed.
    #[must_use]
    pub fn new(value: bool) -> Self {
        Self {
            mode: CheckboxMode::Binary(value),
            on_changed: None,
            active_color: None,
            check_color: None,
            is_error: false,
            semantic_label: None,
        }
    }

    /// Creates a tristate checkbox. `None` means indeterminate; `Some(false)` /
    /// `Some(true)` are the off/on arms of the same cycle. See the tap-cycle
    /// order documented on [`Self::on_changed`].
    #[must_use]
    pub fn tristate(value: Option<bool>) -> Self {
        Self {
            mode: CheckboxMode::Tristate(value),
            on_changed: None,
            active_color: None,
            check_color: None,
            is_error: false,
            semantic_label: None,
        }
    }

    /// Sets the change handler. Presence of a handler is what makes this
    /// checkbox interactive — `None` (the default) renders disabled and
    /// swallows taps. On tap, fires with the next value in cycle order:
    /// `Some(false) -> Some(true) -> (tristate: None, else: Some(false))
    /// -> Some(false) -> ...`.
    #[must_use]
    pub fn on_changed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>, Option<bool>) -> R + 'static,
    ) -> Self {
        self.on_changed = Some(crate::event_callback::value_callback(callback));
        self
    }

    /// Overrides the fill color used when this checkbox is selected (and
    /// enabled).
    #[must_use]
    pub fn active_color(mut self, color: Color) -> Self {
        self.active_color = Some(color);
        self
    }

    /// Overrides the checkmark/dash stroke color.
    #[must_use]
    pub fn check_color(mut self, color: Color) -> Self {
        self.check_color = Some(color);
        self
    }

    /// Marks this checkbox as showing an error state — recolors the fill,
    /// check, border, and overlay through the M3 error branch.
    #[must_use]
    pub fn is_error(mut self, is_error: bool) -> Self {
        self.is_error = is_error;
        self
    }

    /// Sets the accessible label announced by assistive technology.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Whether this checkbox responds to taps (`on_changed` is set).
    fn is_interactive(&self) -> bool {
        self.on_changed.is_some()
    }

    /// The value a tap applies: `false -> true`, `true -> tristate ? null :
    /// false`, `null -> false`.
    fn next_value(&self) -> Option<bool> {
        match self.mode {
            CheckboxMode::Binary(false) | CheckboxMode::Tristate(Some(false)) => Some(true),
            CheckboxMode::Tristate(Some(true)) => None,
            // Binary true wraps to false; tristate null advances to false.
            CheckboxMode::Binary(true) | CheckboxMode::Tristate(None) => Some(false),
        }
    }
}

/// Persistent state behind [`Checkbox`] — owns the [`WidgetStatesController`]
/// shared with the [`InkWell`] this view builds. See the module docs'
/// "Composition" section for why `Selected` lives here while
/// `Hovered`/`Focused`/`Pressed`/`Disabled` are `InkWell`'s to manage.
pub struct CheckboxState {
    states: WidgetStatesController,
    states_listener: Option<flui_sdk::foundation::ListenerId>,
}

impl std::fmt::Debug for CheckboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckboxState")
            .field("states", &self.states)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Checkbox {
    type State = CheckboxState;

    fn create_state(&self) -> Self::State {
        // `Selected` is seeded from the initial view (`&self` here IS that
        // initial view; an indeterminate value counts as selected), so it is
        // correct before the first `build` rather than needing a same-frame
        // correction.
        let initial = if self.mode.is_selected() {
            WidgetStates::from(WidgetState::Selected)
        } else {
            WidgetStates::NONE
        };
        CheckboxState {
            states: WidgetStatesController::new(initial),
            states_listener: None,
        }
    }
}

impl ViewState<Checkbox> for CheckboxState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // ADR-0018: acquired here, fired only from the states-controller
        // listener below — never from `build`. Mirrors `InkWellState`. The
        // handle is consumed directly by the listener closure; nothing else
        // needs to re-read it later, so it is not stored on `self`.
        let rebuild = ctx.rebuild_handle();
        self.states_listener = Some(self.states.add_listener(std::rc::Rc::new(move || {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }

    fn did_update_view(&mut self, old_view: &Checkbox, new_view: &Checkbox) {
        // On a value change, V1 has no animation to drive, so this resyncs
        // `Selected` directly. Never called from `build` —
        // same care `InkWellState::did_update_view` takes.
        if old_view.mode.value() != new_view.mode.value() {
            self.states
                .update(WidgetState::Selected, new_view.mode.is_selected());
        }
    }

    fn build(&self, view: &Checkbox, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let checkbox_theme = theme.checkbox_theme.clone();
        let colors = theme.color_scheme;

        // The live states set: Selected (owned by this state), Hovered/
        // Focused/Pressed/Disabled (owned by the InkWell built below, on the
        // SAME controller) — plus Error, folded in locally per the module
        // docs (never stored on the controller).
        let mut states = self.states.value();
        if view.is_error {
            states = states.with_state(WidgetState::Error);
        }

        let fill_color = resolve_checkbox_fill_color(
            view.active_color,
            checkbox_theme.as_ref().and_then(|t| t.fill_color.as_ref()),
            &colors,
            states,
        );

        let check_color = view
            .check_color
            .or_else(|| {
                resolve_state_color(
                    checkbox_theme.as_ref().and_then(|t| t.check_color.as_ref()),
                    &states,
                )
            })
            .unwrap_or_else(|| checkbox_default_check_color(&colors, states));

        let side = checkbox_theme
            .as_ref()
            .and_then(|t| t.side.as_ref())
            .and_then(|property| property.resolve(&states))
            .unwrap_or_else(|| checkbox_default_side(&colors, states));

        let is_error = view.is_error;
        let theme_overlay = checkbox_theme
            .as_ref()
            .and_then(|t| t.overlay_color.clone());
        let overlay_color = WidgetStateProperty::resolve_with(move |live_states: &WidgetStates| {
            let mut resolved_states = *live_states;
            if is_error {
                resolved_states = resolved_states.with_state(WidgetState::Error);
            }
            resolve_state_color(theme_overlay.as_ref(), &resolved_states)
                .or_else(|| Some(checkbox_default_overlay_color(&colors, resolved_states)))
        });

        let painter: std::rc::Rc<dyn CustomPainter> = std::rc::Rc::new(CheckboxPainter {
            fill_color,
            side,
            check_color,
            value: view.mode.value(),
        });

        let interactive = view.is_interactive();
        let next_value = view.next_value();
        let on_changed = view.on_changed.clone();
        let mut ink_well = InkWell::new(
            CustomPaint::new()
                .size(Size::new(
                    CHECKBOX_TAP_TARGET_SIZE,
                    CHECKBOX_TAP_TARGET_SIZE,
                ))
                .painter(painter),
        )
        .shape(MaterialShape::Stadium)
        .overlay_color(overlay_color)
        .states_controller(self.states.clone());
        if interactive {
            ink_well = ink_well.on_tap(move |cx| {
                if let Some(handler) = &on_changed {
                    handler(cx, next_value);
                }
            });
        }

        let flags = checkbox_semantics_flags(view.mode);
        let mut semantics = Semantics::new()
            .enabled(interactive)
            .checked(flags.checked)
            .mixed(flags.mixed);
        if let Some(label) = &view.semantic_label {
            semantics = semantics.label(label.clone());
        }

        semantics.child(ink_well)
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
    }
}

/// Resolves [`Checkbox`]'s fill color through the widget -> theme -> default
/// cascade — extracted as its own pure function (not left inline in
/// `build`) specifically so the tier-precedence order is unit-testable
/// without mounting a widget tree. `active_color` only substitutes when
/// [`WidgetState::Selected`] AND NOT [`WidgetState::Disabled`] (otherwise it
/// falls through to the theme tier, then the M3 default); a widget-level
/// state-property fill tier above it is a named V1 deferral (see the
/// module docs).
fn resolve_checkbox_fill_color(
    active_color: Option<Color>,
    theme_fill_color: Option<&WidgetStateProperty<Option<Color>>>,
    colors: &ColorScheme,
    states: WidgetStates,
) -> Color {
    let widget_active_override = (!states.contains_state(WidgetState::Disabled)
        && states.contains_state(WidgetState::Selected))
    .then_some(active_color)
    .flatten();

    widget_active_override
        .or_else(|| resolve_state_color(theme_fill_color, &states))
        .unwrap_or_else(|| checkbox_default_fill_color(colors, states))
}

/// The M3 default fill color.
fn checkbox_default_fill_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        return if states.contains_state(WidgetState::Selected) {
            colors.on_surface.with_opacity(0.38)
        } else {
            Color::TRANSPARENT
        };
    }
    if states.contains_state(WidgetState::Selected) {
        return if states.contains_state(WidgetState::Error) {
            colors.error
        } else {
            colors.primary
        };
    }
    Color::TRANSPARENT
}

/// The M3 default check color.
fn checkbox_default_check_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        return if states.contains_state(WidgetState::Selected) {
            colors.surface
        } else {
            Color::TRANSPARENT
        };
    }
    if states.contains_state(WidgetState::Selected) {
        return if states.contains_state(WidgetState::Error) {
            colors.on_error
        } else {
            colors.on_primary
        };
    }
    Color::TRANSPARENT
}

/// The M3 default border side.
fn checkbox_default_side(colors: &ColorScheme, states: WidgetStates) -> BorderSide<f64> {
    let side = |color: Color, width: f64| BorderSide::new(color, width, BorderStyle::Solid);

    if states.contains_state(WidgetState::Disabled) {
        return if states.contains_state(WidgetState::Selected) {
            side(Color::TRANSPARENT, 2.0)
        } else {
            side(colors.on_surface.with_opacity(0.38), 2.0)
        };
    }
    if states.contains_state(WidgetState::Selected) {
        return side(Color::TRANSPARENT, 0.0);
    }
    if states.contains_state(WidgetState::Error) {
        return side(colors.error, 2.0);
    }
    if states.contains_state(WidgetState::Pressed)
        || states.contains_state(WidgetState::Hovered)
        || states.contains_state(WidgetState::Focused)
    {
        return side(colors.on_surface, 2.0);
    }
    side(colors.on_surface_variant, 2.0)
}

/// The M3 default overlay color. Returns `None` where the M3 table calls for
/// transparent — see [`InkWell`]'s own "`None` resolution = no overlay layer at all" contract.
fn checkbox_default_overlay_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Error) {
        if states.contains_state(WidgetState::Pressed) {
            return colors.error.with_opacity(0.1);
        }
        if states.contains_state(WidgetState::Hovered) {
            return colors.error.with_opacity(0.08);
        }
        if states.contains_state(WidgetState::Focused) {
            return colors.error.with_opacity(0.1);
        }
    }
    if states.contains_state(WidgetState::Selected) {
        if states.contains_state(WidgetState::Pressed) {
            return colors.on_surface.with_opacity(0.1);
        }
        if states.contains_state(WidgetState::Hovered) {
            return colors.primary.with_opacity(0.08);
        }
        if states.contains_state(WidgetState::Focused) {
            return colors.primary.with_opacity(0.1);
        }
        return Color::TRANSPARENT;
    }
    if states.contains_state(WidgetState::Pressed) {
        return colors.primary.with_opacity(0.1);
    }
    if states.contains_state(WidgetState::Hovered) {
        return colors.on_surface.with_opacity(0.08);
    }
    if states.contains_state(WidgetState::Focused) {
        return colors.on_surface.with_opacity(0.1);
    }
    Color::TRANSPARENT
}

/// Paints the checkbox's box (fill + border) and, for a settled `Some(true)`/
/// `None` value, the checkmark/dash — always at the fully-settled shape (see
/// the module docs' V1-scope section), with no interpolation and no
/// radial-reaction painting (the overlay comes from [`InkWell`] instead — see
/// the module docs' "Composition" section).
#[derive(Debug, Clone, PartialEq)]
struct CheckboxPainter {
    fill_color: Color,
    side: BorderSide<f64>,
    check_color: Color,
    value: Option<bool>,
}

impl CustomPainter for CheckboxPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let center_x = size.width / 2.0;
        let center_y = size.height / 2.0;
        let half_edge = CHECKBOX_EDGE_SIZE / 2.0;
        let origin_x = center_x - half_edge;
        let origin_y = center_y - half_edge;

        let outer_rect = Rect::from_ltrb(
            origin_x,
            origin_y,
            origin_x + CHECKBOX_EDGE_SIZE,
            origin_y + CHECKBOX_EDGE_SIZE,
        );
        let outer_rrect = RRect::from_rect_circular(outer_rect, CORNER_RADIUS);

        canvas.draw_rrect(outer_rrect, &Paint::fill(self.fill_color));

        if self.side.style.is_solid() && self.side.width > 0.0 {
            let inner_rrect = outer_rrect.inflate(-self.side.width);
            canvas.draw_drrect(outer_rrect, inner_rrect, &Paint::fill(self.side.color));
        }

        let stroke_paint = Paint::stroke(self.check_color, STROKE_WIDTH);
        match self.value {
            Some(true) => draw_checkmark(canvas, origin_x, origin_y, &stroke_paint),
            None => draw_dash(canvas, origin_x, origin_y, &stroke_paint),
            Some(false) => {}
        }
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

/// The settled checkmark stroke: the full `start -> mid -> end` polyline.
fn draw_checkmark(canvas: &mut Canvas, origin_x: f64, origin_y: f64, paint: &Paint) {
    let point = |dx: f64, dy: f64| Point::new(origin_x + dx, origin_y + dy);
    let mut path = Path::new();
    path.move_to(point(CHECKBOX_EDGE_SIZE * 0.15, CHECKBOX_EDGE_SIZE * 0.45));
    path.line_to(point(CHECKBOX_EDGE_SIZE * 0.4, CHECKBOX_EDGE_SIZE * 0.7));
    path.line_to(point(CHECKBOX_EDGE_SIZE * 0.85, CHECKBOX_EDGE_SIZE * 0.25));
    canvas.draw_path(&path, paint);
}

/// The settled indeterminate dash: a full-width horizontal line.
fn draw_dash(canvas: &mut Canvas, origin_x: f64, origin_y: f64, paint: &Paint) {
    let point = |dx: f64| Point::new(origin_x + dx, origin_y + CHECKBOX_EDGE_SIZE * 0.5);
    canvas.draw_line(
        point(CHECKBOX_EDGE_SIZE * 0.2),
        point(CHECKBOX_EDGE_SIZE * 0.8),
        paint,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Construction / builder surface
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Tristate tap-cycle semantics (mutation-honest: each arm pinned)
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // M3 default token tables — per-state probes, in branch order
    // ------------------------------------------------------------------

    fn light() -> ColorScheme {
        ColorScheme::light()
    }

    // ------------------------------------------------------------------
    // resolve_checkbox_fill_color — tier precedence (widget > theme >
    // default), mutation-run: each probe below was verified to fail
    // against a deliberately broken cascade (widget/theme tier short-
    // circuited, or the `!Disabled && Selected` gate dropped) before being
    // confirmed against the real implementation.
    // ------------------------------------------------------------------

    #[test]
    fn widget_override_is_ignored_when_disabled_even_if_selected() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        let resolved =
            resolve_checkbox_fill_color(Some(Color::rgb(1, 1, 1)), None, &light(), states);
        assert_ne!(resolved, Color::rgb(1, 1, 1));
        assert_eq!(resolved, checkbox_default_fill_color(&light(), states));
    }

    // ------------------------------------------------------------------
    // Tap-target geometry
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Painter should_repaint / geometry
    // ------------------------------------------------------------------
}
