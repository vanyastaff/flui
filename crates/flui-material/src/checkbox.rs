//! [`Checkbox`] — a tristate-capable M3 selection control.
//!
//! # Flutter parity
//!
//! `material/checkbox.dart`, `material/checkbox_theme.dart`, and
//! `widgets/toggleable.dart`'s `ToggleableStateMixin`/`ToggleablePainter`
//! (oracle tag `3.44.0`).
//!
//! # V1 scope: static states, no toggle/reaction animation
//!
//! The oracle's `ToggleableStateMixin` drives four `AnimationController`s
//! (`positionController` for the box/check morph, `reactionController` for
//! the radial ink splash, plus hover/focus fade controllers) so a value
//! change or an interaction visibly interpolates. This V1 **snaps**: no
//! `positionController`, so `CheckboxPainter` always paints the box/check
//! at their fully-settled `t == 1.0` shape (`_CheckboxPainter._drawCheck`/
//! `_drawDash` evaluated at `t = 1.0`, `checkbox.dart` `:750-783`), and no
//! `reactionController`/radial-splash painting — the hover/focus/press
//! overlay comes from [`InkWell`] instead (a single resolved-color fill, not
//! an expanding circle), matching how [`crate::floating_action_button`]
//! already substitutes `InkWell` for `RawMaterialButton`'s ink-feature
//! registry. Named deferral, not a silent drop: a future animated Checkbox
//! reintroduces `AnimationController`-driven interpolation without changing
//! this type's public surface (`value`/`tristate`/`on_changed` are already
//! the oracle's steady-state contract).
//!
//! # Composition: `InkWell` owns interaction, `Checkbox` owns `Selected`
//!
//! [`InkWell`] already ports the oracle's hover/focus/press wiring and
//! `Disabled` derivation (`ink_well.dart`'s `_InkResponseState`, reused here
//! rather than re-deriving `ToggleableStateMixin`'s
//! `FocusableActionDetector`/`GestureDetector` composition from scratch).
//! `Checkbox` shares one [`WidgetStatesController`] with the `InkWell` it
//! builds (via [`InkWell::states_controller`]): `InkWell` manages
//! `Hovered`/`Focused`/`Pressed`/`Disabled` on it, `Checkbox` manages
//! `Selected` on it (mirroring `ToggleableStateMixin.states`'s `if (value ??
//! true) WidgetState.selected`) — both read the same live set. `Error` is
//! never stored on the controller (the oracle's `isError` is layered onto a
//! *local copy* of `states` only where consumed, `checkbox.dart` `:538-541`
//! /`:564-566`/`:574-576`); this port folds it in the same way, at each
//! resolution site.
//!
//! # Painting: `CustomPaint` + real stroked geometry, not a glyph
//!
//! `CheckboxPainter` draws the 18dp rounded-rect box
//! ([`Canvas::draw_rrect`]/[`Canvas::draw_drrect`]) and the checkmark/dash as
//! an actual stroked [`Path`]/line ([`Canvas::draw_path`]/
//! [`Canvas::draw_line`]) at the oracle's exact relative coordinates
//! (`_CheckboxPainter._drawCheck`/`_drawDash`, `checkbox.dart` `:750-783`) —
//! not a bundled icon-font glyph. Unlike [`crate::back_button::BackButton`]
//! (which had no path-drawing seam and fell back to a `MaterialIcons` glyph
//! identity), `flui-painting`'s [`Canvas`] already exposes the primitives the
//! oracle's own painter uses, so this is a direct, honest port of the static
//! (non-animated) shape.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Toggle/reaction/hover/focus-fade animation** — see above.
//! - **`Checkbox.adaptive`** (`CupertinoCheckbox` platform switch) — no
//!   `TargetPlatform` substrate to switch on yet.
//! - **`mouse_cursor`, `splash_radius`, `material_tap_target_size`,
//!   `visual_density` overrides** — V1 always uses the M3 defaults
//!   (`kMinInteractiveDimension` = 40dp tap target, `VisualDensity.standard`
//!   = no adjustment, `splashRadius = 20.0`). [`crate::CheckboxThemeData`]
//!   and the widget both omit these fields; see that type's own doc comment.
//! - **Widget-level `fill_color`/`overlay_color`/`side`/`shape` overrides**
//!   (the oracle's `WidgetStateProperty`-shaped constructor parameters) —
//!   only [`Checkbox::active_color`] and [`Checkbox::check_color`] (the
//!   oracle's plain-`Color` overrides) ship at the widget tier; the theme
//!   tier ([`crate::CheckboxThemeData`]) and the M3 default tier are both
//!   fully state-resolved. A future widget-level `WidgetStateProperty` override
//!   slot is additive.
//! - **`focus_node`/`autofocus`** — [`InkWell`] itself has no `autofocus`
//!   hook yet (a whole-substrate gap, not specific to this type); only
//!   `focus_node` sharing is wired through.

use std::rc::Rc;

use flui_foundation::Listenable;
use flui_rendering::pipeline::Canvas;
use flui_types::geometry::px;
use flui_types::painting::{Paint, Path};
use flui_types::styling::{BorderSide, BorderStyle};
use flui_types::{Color, Pixels, Point, RRect, Rect, Size};
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_widgets::{
    CustomPaint, CustomPainter, Semantics, WidgetState, WidgetStateProperty, WidgetStates,
    WidgetStatesController,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// A checkbox's edge length. Flutter parity: `Checkbox.width` (`18.0`,
/// `checkbox.dart`, oracle tag `3.44.0`).
pub const CHECKBOX_EDGE_SIZE: f32 = 18.0;

/// The box outline's and checkmark/dash's stroke width. Flutter parity:
/// `_kStrokeWidth` (`checkbox.dart`).
const STROKE_WIDTH: f32 = 2.0;

/// The M3 tap-target side length. Flutter parity: `kMinInteractiveDimension`
/// (`constants.dart`), the `MaterialTapTargetSize.padded` branch
/// `_CheckboxState.build` always takes in V1 (no `materialTapTargetSize`
/// override yet — see the module docs).
pub const CHECKBOX_TAP_TARGET_SIZE: f32 = 40.0;

/// The box's corner radius. Flutter parity: `_CheckboxDefaultsM3.shape`,
/// `RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(2.0)))`.
const CORNER_RADIUS: f32 = 2.0;

// The 18dp box must fit inside the 40dp tap target with room for the
// centering inset the painter computes — a compile-time invariant, not a
// runtime test (both sides are `const`).
const _: () = assert!(CHECKBOX_EDGE_SIZE < CHECKBOX_TAP_TARGET_SIZE);

/// A value-change callback: the next tristate value. `Rc`-based
/// (owner-local, per ADR-0027) — matches [`InkWell`]'s own callback shape.
type CheckboxChangeCallback = Rc<dyn Fn(Option<bool>)>;

/// A Material Design tristate-capable checkbox.
///
/// The checkbox itself holds no state: [`Checkbox::on_changed`] fires with
/// the next value on tap, and the caller re-renders with the updated
/// `value` — see the module docs for what "next value" means under
/// [`Checkbox::tristate`].
///
/// ```rust
/// use flui_material::Checkbox;
///
/// let _off = Checkbox::new(Some(false)).on_changed(|_next| { /* ... */ });
/// let _tristate = Checkbox::new(None).tristate(true);
/// let _disabled = Checkbox::new(Some(true));
/// ```
#[derive(Clone, StatefulView)]
pub struct Checkbox {
    value: Option<bool>,
    tristate: bool,
    on_changed: Option<CheckboxChangeCallback>,
    active_color: Option<Color>,
    check_color: Option<Color>,
    is_error: bool,
    semantic_label: Option<String>,
}

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("value", &self.value)
            .field("tristate", &self.tristate)
            .field("is_interactive", &self.is_interactive())
            .field("is_error", &self.is_error)
            .finish_non_exhaustive()
    }
}

impl Checkbox {
    /// Creates a checkbox at `value`, tristate disabled, no change handler
    /// (disabled), no overrides.
    ///
    /// `value` may only be `None` once [`Self::tristate`] is enabled —
    /// checked with `debug_assert!`, matching the oracle's own
    /// `assert(tristate || value != null)` (`checkbox.dart`, a debug-only
    /// constructor invariant in both Dart and Rust).
    #[must_use]
    pub fn new(value: Option<bool>) -> Self {
        Self {
            value,
            tristate: false,
            on_changed: None,
            active_color: None,
            check_color: None,
            is_error: false,
            semantic_label: None,
        }
    }

    /// Enables the third (`None`/indeterminate) value. See the tap-cycle
    /// order documented on `Checkbox::on_changed`.
    #[must_use]
    pub fn tristate(mut self, tristate: bool) -> Self {
        self.tristate = tristate;
        self
    }

    /// Sets the change handler. Presence of a handler is what makes this
    /// checkbox interactive — `None` (the default) renders disabled and
    /// swallows taps. On tap, fires with the next value in cycle order:
    /// `Some(false) -> Some(true) -> (tristate: None, else: Some(false))
    /// -> Some(false) -> ...`. Flutter parity: `Checkbox.onChanged`.
    #[must_use]
    pub fn on_changed(mut self, callback: impl Fn(Option<bool>) + 'static) -> Self {
        self.on_changed = Some(Rc::new(callback));
        self
    }

    /// Overrides the fill color used when this checkbox is selected (and
    /// enabled). Flutter parity: `Checkbox.activeColor`.
    #[must_use]
    pub fn active_color(mut self, color: Color) -> Self {
        self.active_color = Some(color);
        self
    }

    /// Overrides the checkmark/dash stroke color. Flutter parity:
    /// `Checkbox.checkColor`.
    #[must_use]
    pub fn check_color(mut self, color: Color) -> Self {
        self.check_color = Some(color);
        self
    }

    /// Marks this checkbox as showing an error state — recolors the fill,
    /// check, border, and overlay through the M3 error branch. Flutter
    /// parity: `Checkbox.isError`.
    #[must_use]
    pub fn is_error(mut self, is_error: bool) -> Self {
        self.is_error = is_error;
        self
    }

    /// Sets the accessible label announced by assistive technology. Flutter
    /// parity: `Checkbox.semanticLabel`.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Whether this checkbox responds to taps. Flutter parity:
    /// `ToggleableStateMixin.isInteractive` (`onChanged != null`).
    fn is_interactive(&self) -> bool {
        self.on_changed.is_some()
    }

    /// The value a tap applies. Flutter parity: `_handleTap`'s `switch
    /// (value)` (`checkbox.dart` `:241-248`): `false -> true`, `true ->
    /// tristate ? null : false`, `null -> false`.
    fn next_value(&self) -> Option<bool> {
        match self.value {
            Some(false) => Some(true),
            Some(true) => {
                if self.tristate {
                    None
                } else {
                    Some(false)
                }
            }
            None => Some(false),
        }
    }
}

/// Persistent state behind [`Checkbox`] — owns the [`WidgetStatesController`]
/// shared with the [`InkWell`] this view builds. See the module docs'
/// "Composition" section for why `Selected` lives here while
/// `Hovered`/`Focused`/`Pressed`/`Disabled` are `InkWell`'s to manage.
pub struct CheckboxState {
    states: WidgetStatesController,
    states_listener: Option<flui_foundation::ListenerId>,
    rebuild: Option<RebuildHandle>,
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
        // Flutter parity: `ToggleableStateMixin.states`'s `if (value ?? true)
        // WidgetState.selected` — seeded from the initial view (`&self`
        // here IS that initial view), so `Selected` is correct before the
        // first `build` rather than needing a same-frame correction.
        let selected = self.value.unwrap_or(true);
        let initial = if selected {
            WidgetStates::from(WidgetState::Selected)
        } else {
            WidgetStates::NONE
        };
        CheckboxState {
            states: WidgetStatesController::new(initial),
            states_listener: None,
            rebuild: None,
        }
    }
}

impl ViewState<Checkbox> for CheckboxState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // ADR-0018: acquired here, fired only from the states-controller
        // listener below — never from `build`. Mirrors `InkWellState`.
        let rebuild = ctx.rebuild_handle();
        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(std::sync::Arc::new(move || {
            rebuild_for_listener.schedule();
        })));
        self.rebuild = Some(rebuild);
    }

    fn did_update_view(&mut self, old_view: &Checkbox, new_view: &Checkbox) {
        // Flutter parity: `didUpdateWidget` -> `animateToValue()` on a value
        // change (`checkbox.dart` `:424-430`); V1 has no animation to drive,
        // so this resyncs `Selected` directly. Never called from `build` —
        // same care `InkWellState::did_update_view` takes.
        if old_view.value != new_view.value {
            let selected = new_view.value.unwrap_or(true);
            self.states.update(WidgetState::Selected, selected);
        }
    }

    fn build(&self, view: &Checkbox, ctx: &dyn BuildContext) -> impl IntoView {
        debug_assert!(
            view.tristate || view.value.is_some(),
            "BUG: Checkbox::value must not be None unless Checkbox::tristate(true) is set"
        );

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

        // Flutter parity: `_widgetFillColor` (`checkbox.dart` `:450-460`) —
        // `activeColor` only substitutes when selected and enabled; the
        // `widget.fillColor` tier above it is a named V1 deferral (see the
        // module docs), so the cascade starts one tier lower.
        let widget_active_override = (!states.contains_state(WidgetState::Disabled)
            && states.contains_state(WidgetState::Selected))
        .then_some(view.active_color)
        .flatten();

        let fill_color = widget_active_override
            .or_else(|| {
                resolve_state_color(
                    checkbox_theme.as_ref().and_then(|t| t.fill_color.as_ref()),
                    &states,
                )
            })
            .unwrap_or_else(|| checkbox_default_fill_color(&colors, states));

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

        let painter: std::sync::Arc<dyn CustomPainter> = std::sync::Arc::new(CheckboxPainter {
            fill_color,
            side,
            check_color,
            value: view.value,
        });

        let interactive = view.is_interactive();
        let next_value = view.next_value();
        let on_changed = view.on_changed.clone();
        let mut ink_well = InkWell::new(
            CustomPaint::new()
                .size(Size::new(
                    px(CHECKBOX_TAP_TARGET_SIZE),
                    px(CHECKBOX_TAP_TARGET_SIZE),
                ))
                .painter(painter),
        )
        .shape(MaterialShape::Stadium)
        .overlay_color(overlay_color)
        .states_controller(self.states.clone());
        if interactive {
            ink_well = ink_well.on_tap(move || {
                if let Some(handler) = &on_changed {
                    handler(next_value);
                }
            });
        }

        let mut semantics = Semantics::new()
            .checked(view.value.unwrap_or(false))
            .enabled(interactive);
        if view.tristate {
            semantics = semantics.mixed(view.value.is_none());
        }
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

/// Resolves `property` against `states`, flattening the "no property" and
/// "property present but resolves to `None`" cases into one `None` —
/// exactly the fall-through-to-next-tier shape every cascade in this module
/// wants (`widget?.resolve(states) ?? theme?.resolve(states) ?? default`).
fn resolve_state_color(
    property: Option<&WidgetStateProperty<Option<Color>>>,
    states: &WidgetStates,
) -> Option<Color> {
    property.and_then(|p| p.resolve(states))
}

/// `_CheckboxDefaultsM3.fillColor` (`checkbox.dart`, oracle tag `3.44.0`).
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

/// `_CheckboxDefaultsM3.checkColor` (`checkbox.dart`, oracle tag `3.44.0`).
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

/// `_CheckboxDefaultsM3.side` (`checkbox.dart`, oracle tag `3.44.0`).
fn checkbox_default_side(colors: &ColorScheme, states: WidgetStates) -> BorderSide<Pixels> {
    let side = |color: Color, width: f32| BorderSide::new(color, px(width), BorderStyle::Solid);

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

/// `_CheckboxDefaultsM3.overlayColor` (`checkbox.dart`, oracle tag `3.44.0`).
/// Returns `None` where the oracle returns `Colors.transparent` — see
/// [`InkWell`]'s own "`None` resolution = no overlay layer at all" contract.
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
/// the module docs' V1-scope section). Flutter parity: `_CheckboxPainter`
/// (`checkbox.dart` `:653-837`), evaluated at `t = 1.0` throughout (no
/// `position`/`previousValue` interpolation) with no
/// `ToggleablePainter.paintRadialReaction` call (the overlay comes from
/// [`InkWell`] instead — see the module docs' "Composition" section).
#[derive(Debug, Clone, PartialEq)]
struct CheckboxPainter {
    fill_color: Color,
    side: BorderSide<Pixels>,
    check_color: Color,
    value: Option<bool>,
}

impl CustomPainter for CheckboxPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let center_x = size.width.get() / 2.0;
        let center_y = size.height.get() / 2.0;
        let half_edge = CHECKBOX_EDGE_SIZE / 2.0;
        let origin_x = center_x - half_edge;
        let origin_y = center_y - half_edge;

        let outer_rect = Rect::from_ltrb(
            px(origin_x),
            px(origin_y),
            px(origin_x + CHECKBOX_EDGE_SIZE),
            px(origin_y + CHECKBOX_EDGE_SIZE),
        );
        let outer_rrect = RRect::from_rect_circular(outer_rect, px(CORNER_RADIUS));

        canvas.draw_rrect(outer_rrect, &Paint::fill(self.fill_color));

        if self.side.style.is_solid() && self.side.width.get() > 0.0 {
            let inner_rrect = outer_rrect.inflate(px(-self.side.width.get()));
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

/// The settled (`t = 1.0`) checkmark stroke. Flutter parity:
/// `_CheckboxPainter._drawCheck` (`checkbox.dart` `:750-771`) at `t = 1.0`:
/// the full `start -> mid -> end` polyline.
fn draw_checkmark(canvas: &mut Canvas, origin_x: f32, origin_y: f32, paint: &Paint) {
    let point = |dx: f32, dy: f32| Point::new(px(origin_x + dx), px(origin_y + dy));
    let mut path = Path::new();
    path.move_to(point(CHECKBOX_EDGE_SIZE * 0.15, CHECKBOX_EDGE_SIZE * 0.45));
    path.line_to(point(CHECKBOX_EDGE_SIZE * 0.4, CHECKBOX_EDGE_SIZE * 0.7));
    path.line_to(point(CHECKBOX_EDGE_SIZE * 0.85, CHECKBOX_EDGE_SIZE * 0.25));
    canvas.draw_path(&path, paint);
}

/// The settled (`t = 1.0`) indeterminate dash: a full-width horizontal line.
/// Flutter parity: `_CheckboxPainter._drawDash` (`checkbox.dart` `:773-783`)
/// at `t = 1.0`.
fn draw_dash(canvas: &mut Canvas, origin_x: f32, origin_y: f32, paint: &Paint) {
    let point = |dx: f32| Point::new(px(origin_x + dx), px(origin_y + CHECKBOX_EDGE_SIZE * 0.5));
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

    #[test]
    fn new_leaves_every_override_unset_and_is_not_interactive() {
        let checkbox = Checkbox::new(Some(false));
        assert!(checkbox.active_color.is_none());
        assert!(checkbox.check_color.is_none());
        assert!(!checkbox.tristate);
        assert!(!checkbox.is_error);
        assert!(!checkbox.is_interactive());
    }

    #[test]
    fn on_changed_makes_the_checkbox_interactive() {
        let checkbox = Checkbox::new(Some(false)).on_changed(|_| {});
        assert!(checkbox.is_interactive());
    }

    // ------------------------------------------------------------------
    // Tristate tap-cycle semantics (mutation-honest: each arm pinned)
    // ------------------------------------------------------------------

    #[test]
    fn next_value_toggles_false_to_true_regardless_of_tristate() {
        assert_eq!(Checkbox::new(Some(false)).next_value(), Some(true));
        assert_eq!(
            Checkbox::new(Some(false)).tristate(true).next_value(),
            Some(true)
        );
    }

    #[test]
    fn next_value_true_goes_to_false_when_not_tristate() {
        assert_eq!(Checkbox::new(Some(true)).next_value(), Some(false));
    }

    #[test]
    fn next_value_true_goes_to_null_when_tristate() {
        assert_eq!(Checkbox::new(Some(true)).tristate(true).next_value(), None);
    }

    #[test]
    fn next_value_null_goes_to_false() {
        assert_eq!(Checkbox::new(None).tristate(true).next_value(), Some(false));
    }

    // ------------------------------------------------------------------
    // M3 default token tables — per-state probes, oracle branch order
    // ------------------------------------------------------------------

    fn light() -> ColorScheme {
        ColorScheme::light()
    }

    #[test]
    fn default_fill_color_unselected_enabled_is_transparent() {
        assert_eq!(
            checkbox_default_fill_color(&light(), WidgetStates::NONE),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn default_fill_color_selected_enabled_is_primary() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            checkbox_default_fill_color(&light(), states),
            light().primary
        );
    }

    #[test]
    fn default_fill_color_selected_error_is_error_color() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Error);
        assert_eq!(checkbox_default_fill_color(&light(), states), light().error);
    }

    #[test]
    fn default_fill_color_selected_disabled_is_faded_on_surface() {
        // Combined-state pin: Disabled must win over Selected's own branch,
        // matching the oracle's `if (disabled) { ... } if (selected) { ...
        // }` order (disabled checked first).
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        assert_eq!(
            checkbox_default_fill_color(&light(), states),
            light().on_surface.with_opacity(0.38)
        );
    }

    #[test]
    fn default_fill_color_unselected_disabled_is_transparent() {
        let states = WidgetStates::from(WidgetState::Disabled);
        assert_eq!(
            checkbox_default_fill_color(&light(), states),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn default_check_color_selected_enabled_is_on_primary() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            checkbox_default_check_color(&light(), states),
            light().on_primary
        );
    }

    #[test]
    fn default_check_color_selected_error_is_on_error() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Error);
        assert_eq!(
            checkbox_default_check_color(&light(), states),
            light().on_error
        );
    }

    #[test]
    fn default_check_color_selected_disabled_is_surface() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        assert_eq!(
            checkbox_default_check_color(&light(), states),
            light().surface
        );
    }

    #[test]
    fn default_side_selected_is_zero_width_transparent() {
        let states = WidgetStates::from(WidgetState::Selected);
        let side = checkbox_default_side(&light(), states);
        assert_eq!(side.color, Color::TRANSPARENT);
        assert_eq!(side.width, px(0.0));
    }

    #[test]
    fn default_side_unselected_enabled_default_is_on_surface_variant() {
        let side = checkbox_default_side(&light(), WidgetStates::NONE);
        assert_eq!(side.color, light().on_surface_variant);
        assert_eq!(side.width, px(2.0));
    }

    #[test]
    fn default_side_unselected_hovered_is_on_surface() {
        // Branch-order pin: hovered (unselected, enabled) resolves BEFORE
        // the catch-all default, per `_CheckboxDefaultsM3.side`'s oracle
        // order (disabled, selected, error, pressed, hovered, focused,
        // default).
        let states = WidgetStates::from(WidgetState::Hovered);
        let side = checkbox_default_side(&light(), states);
        assert_eq!(side.color, light().on_surface);
    }

    #[test]
    fn default_side_unselected_disabled_is_faded_on_surface() {
        let states = WidgetStates::from(WidgetState::Disabled);
        let side = checkbox_default_side(&light(), states);
        assert_eq!(side.color, light().on_surface.with_opacity(0.38));
    }

    #[test]
    fn default_side_selected_disabled_is_zero_width_transparent() {
        // Combined pin: Disabled+Selected takes the disabled-selected
        // branch, not the plain-selected branch (same color, but a
        // different code path — width still 2.0 here, unlike plain
        // selected's 0.0).
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        let side = checkbox_default_side(&light(), states);
        assert_eq!(side.color, Color::TRANSPARENT);
        assert_eq!(side.width, px(2.0));
    }

    #[test]
    fn default_overlay_color_selected_hovered_is_primary_at_8_percent() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Hovered);
        assert_eq!(
            checkbox_default_overlay_color(&light(), states),
            light().primary.with_opacity(0.08)
        );
    }

    #[test]
    fn default_overlay_color_selected_pressed_is_on_surface_at_10_percent() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Pressed);
        assert_eq!(
            checkbox_default_overlay_color(&light(), states),
            light().on_surface.with_opacity(0.1)
        );
    }

    #[test]
    fn default_overlay_color_unselected_pressed_is_primary_at_10_percent() {
        let states = WidgetStates::from(WidgetState::Pressed);
        assert_eq!(
            checkbox_default_overlay_color(&light(), states),
            light().primary.with_opacity(0.1)
        );
    }

    #[test]
    fn default_overlay_color_error_pressed_wins_over_selected_branch() {
        // Branch-order pin: Error is checked BEFORE Selected in the oracle
        // (`_CheckboxDefaultsM3.overlayColor`), so an error+selected+pressed
        // combination resolves through the error branch's color, not
        // selected's.
        let states = WidgetStates::from(WidgetState::Error)
            .with_state(WidgetState::Selected)
            .with_state(WidgetState::Pressed);
        assert_eq!(
            checkbox_default_overlay_color(&light(), states),
            light().error.with_opacity(0.1)
        );
    }

    #[test]
    fn default_overlay_color_selected_default_is_transparent() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(
            checkbox_default_overlay_color(&light(), states),
            Color::TRANSPARENT
        );
    }

    // ------------------------------------------------------------------
    // Tap-target geometry
    // ------------------------------------------------------------------

    /// `InkWell`'s [`MaterialShape::Stadium`] over the square tap target
    /// inscribes a circle whose radius equals `_CheckboxDefaultsM3.splashRadius`
    /// (`40.0 / 2`, `checkbox.dart`, oracle tag `3.44.0`) — the geometric
    /// justification for reusing `Stadium` instead of a dedicated `Circle`
    /// shape (see the module docs).
    #[test]
    fn stadium_shape_on_the_tap_target_inscribes_the_m3_splash_radius() {
        let tap_target = Size::new(px(CHECKBOX_TAP_TARGET_SIZE), px(CHECKBOX_TAP_TARGET_SIZE));
        let rrect = MaterialShape::Stadium.to_rrect(tap_target);
        assert_eq!(rrect.top_left.x, px(CHECKBOX_TAP_TARGET_SIZE / 2.0));
    }

    // ------------------------------------------------------------------
    // Painter should_repaint / geometry
    // ------------------------------------------------------------------

    fn painter(value: Option<bool>) -> CheckboxPainter {
        CheckboxPainter {
            fill_color: Color::BLACK,
            side: BorderSide::new(Color::WHITE, px(2.0), BorderStyle::Solid),
            check_color: Color::WHITE,
            value,
        }
    }

    #[test]
    fn should_repaint_is_false_for_an_identical_delegate() {
        let old = painter(Some(true));
        let new = painter(Some(true));
        assert!(!new.should_repaint(&old));
    }

    #[test]
    fn should_repaint_is_true_when_the_value_changes() {
        let old = painter(Some(false));
        let new = painter(Some(true));
        assert!(new.should_repaint(&old));
    }

    #[test]
    fn should_repaint_is_true_against_a_foreign_painter_type() {
        #[derive(Debug)]
        struct Other;
        impl CustomPainter for Other {
            fn paint(&self, _canvas: &mut Canvas, _size: Size) {}
            fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
                true
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        let new = painter(Some(true));
        assert!(new.should_repaint(&Other));
    }

    // ------------------------------------------------------------------
    // resolve_state_color fallthrough contract
    // ------------------------------------------------------------------

    #[test]
    fn resolve_state_color_is_none_with_no_property() {
        assert_eq!(resolve_state_color(None, &WidgetStates::NONE), None);
    }

    #[test]
    fn resolve_state_color_resolves_a_present_property() {
        let property = WidgetStateProperty::all(Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            resolve_state_color(Some(&property), &WidgetStates::NONE),
            Some(Color::rgb(1, 2, 3))
        );
    }
}
