//! [`FloatingActionButton`] — a circular-in-spirit, M3 rounded-square button
//! that hovers over content to promote a primary action.
//!
//! # Flutter parity
//!
//! `material/floating_action_button.dart`'s `FloatingActionButton` (oracle
//! tag `3.44.0`), the regular (non-mini, non-large, non-extended) variant —
//! `small`/`large`/`extended` are named V1 deferrals; see below. The oracle
//! builds a `RawMaterialButton` (`button.dart`) around the resolved
//! `_FABDefaultsM3` token table; this substrate has no `RawMaterialButton`
//! port (only the M3 button family rides `crate::button_style_button::ButtonStyleButtonCore`,
//! which is `ButtonStyle`-shaped and has no elevation *state chain* — FAB's
//! is a five-way `disabled`/`pressed`/`hovered`/`focused`/default cascade a
//! plain `ButtonStyle::elevation` slot can't express any more precisely than
//! `ButtonStyleButtonCore`'s existing per-button tables already do). So this
//! type composes [`Material`] + [`InkWell`] directly, mirroring
//! `RawMaterialButton`'s own composition (`ConstrainedBox` →
//! `Material(elevation, shape, color)` → `InkWell`) rather than routing
//! through `ButtonStyleButtonCore`.
//!
//! # M3 shape: a rounded rectangle, not a circle
//!
//! `_FABDefaultsM3.shape` for the regular variant is
//! `RoundedRectangleBorder(borderRadius: BorderRadius.circular(16.0))`
//! (`floating_action_button.dart`, the generated `_FABDefaultsM3` token
//! block) — **not** `CircleBorder` (that was M2's `_FABDefaultsM2.shape`).
//! this substrate's `fab_shape` function carries this exactly.
//!
//! # The elevation chain
//!
//! `_RawMaterialButtonState._effectiveElevation` (`button.dart`, tag
//! `3.44.0`) resolves in strict precedence order — **disabled, then pressed
//! (`highlightElevation`), then hovered, then focused, then the enabled
//! default** (`elevation`) — and `resolve_elevation` preserves that exact
//! if-chain. For the regular M3 FAB every tier but hover happens to share the
//! same `6.0` value (`elevation: 6.0, focusElevation: 6.0, hoverElevation:
//! 8.0, highlightElevation: 6.0`, and `disabledElevation` is never
//! overridden by `_FABDefaultsM3`, so `RawMaterialButton`'s constructor-level
//! `disabledElevation ?? elevation` fallback resolves it to `6.0` too) — so
//! *values* rarely distinguish the branches, but the *order* still does: a
//! combined pressed+hovered state must resolve through the pressed branch
//! (still `6.0`), not fall through past it to hover's `8.0`. This doubles as
//! Flutter parity for the oracle's own warning: "It is highly discouraged to
//! disable a floating action button as there is no indication to the user
//! that the button is disabled" — `disabledElevation` really does equal the
//! enabled default here, unlike every `ButtonStyleButtonCore` button (whose
//! `_TokenDefaultsM3.elevation` zeroes out when disabled).
//!
//! # Colors: static, not state-resolved
//!
//! Unlike `elevation`, `_FABDefaultsM3.foregroundColor`/`backgroundColor`
//! carry no per-state branch at all — `onPrimaryContainer`/`primaryContainer`
//! regardless of `disabled`/`pressed`/`hovered`/`focused`. Only the overlay
//! (splash/hover/focus tint) and the elevation vary with state; both
//! constants are applied directly, no `WidgetStateProperty` needed for them.
//!
//! # Overlay ramp
//!
//! `splashColor`/`focusColor` both resolve to `onPrimaryContainer` at 10%
//! opacity and `hoverColor` to 8% — the exact pressed(10%)/hovered(8%)/
//! focused(10%) shape `crate::elevated_button::pressed_hovered_focused_overlay`
//! already implements for the button family, reused here rather than
//! duplicated (see that function's own doc for the pressed-before-hovered
//! order it preserves).
//!
//! # Deferred, and named
//!
//! - **`small`/`large`/`extended` variants** — distinct size constraints,
//!   corner radii, icon sizes, and (for `extended`) an icon+label row
//!   composition. Only the regular 56×56 variant ships.
//! - **`disabled_elevation`/`focus_elevation`/`hover_elevation`/
//!   `highlight_elevation` overrides**, `heroTag`/`Hero` wrapping, `tooltip`/
//!   `Tooltip` wrapping, `mouseCursor`, `clipBehavior`, focus node/autofocus,
//!   `materialTapTargetSize`, `enableFeedback` — no override surface yet;
//!   every FAB uses the M3 default token table verbatim.
//! - **Own `foreground_color`/`background_color` overrides** — the oracle's
//!   constructor-level `Color?` fields; this V1 always resolves the M3
//!   defaults.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::{Listenable, ListenerId};
use flui_rendering::constraints::BoxConstraints;
use flui_types::Size;
use flui_types::geometry::{Radius, px};
use flui_types::styling::BorderRadius;
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_widgets::{
    ConstrainedBox, IconTheme, IconThemeData, WidgetState, WidgetStateProperty, WidgetStates,
    WidgetStatesController,
};

use crate::button_style_button::PressCallback;
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// The regular (non-mini) floating action button's side length. Flutter
/// parity: `_FABDefaultsM3.sizeConstraints`, `BoxConstraints.tightFor(width:
/// 56.0, height: 56.0)`.
pub const FAB_SIZE: f32 = 56.0;

/// The regular variant's icon side length. Flutter parity:
/// `_FABDefaultsM3.iconSize` for `_FloatingActionButtonType.regular`.
pub const FAB_ICON_SIZE: f32 = 24.0;

/// The enabled default AND disabled elevation (they coincide — see the
/// module docs). Flutter parity: `_FABDefaultsM3`'s `elevation: 6.0`.
const ELEVATION_DEFAULT: f32 = 6.0;
/// Flutter parity: `_FABDefaultsM3`'s `focusElevation: 6.0`.
const ELEVATION_FOCUSED: f32 = 6.0;
/// Flutter parity: `_FABDefaultsM3`'s `hoverElevation: 8.0`.
const ELEVATION_HOVERED: f32 = 8.0;
/// The pressed elevation — Flutter's `highlightElevation`. Flutter parity:
/// `_FABDefaultsM3`'s `highlightElevation: 6.0`.
const ELEVATION_PRESSED: f32 = 6.0;
/// Flutter parity: `RawMaterialButton`'s constructor-level `disabledElevation
/// ?? elevation` fallback — `_FABDefaultsM3` never overrides
/// `disabledElevation`, so it resolves to the enabled default.
const ELEVATION_DISABLED: f32 = ELEVATION_DEFAULT;

/// The regular M3 FAB's shape: a rectangle with a 16dp corner radius —
/// **not** a circle. See the module docs' "M3 shape" section.
fn fab_shape() -> MaterialShape {
    MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(px(16.0))))
}

/// A circular-in-spirit floating action button — hovers over content to
/// promote a primary action, typically mounted via
/// [`Scaffold::floating_action_button`](crate::scaffold::Scaffold::floating_action_button).
///
/// See the module docs for the M3 default token table this V1 resolves
/// verbatim, the elevation state chain, and the deferred variant list.
///
/// # Examples
///
/// ```rust
/// use flui_material::FloatingActionButton;
/// use flui_widgets::Text;
///
/// let _fab = FloatingActionButton::new(Some(|| {}), Text::new("+"));
/// let _disabled: flui_material::FloatingActionButton =
///     FloatingActionButton::new(None::<fn()>, Text::new("+"));
/// ```
#[derive(Clone, StatefulView)]
pub struct FloatingActionButton {
    on_pressed: Option<PressCallback>,
    child: BoxedView,
}

impl FloatingActionButton {
    /// A regular floating action button around `child` (typically an
    /// [`Icon`](flui_widgets::Icon)). `on_pressed` being `None` disables the
    /// button — Flutter parity: "If the `onPressed` callback is null, then
    /// the button will be disabled" (see the module docs' elevation-chain
    /// section for why that carries no visual indication here either).
    #[must_use]
    pub fn new(on_pressed: Option<impl Fn() + 'static>, child: impl IntoView) -> Self {
        Self {
            on_pressed: on_pressed.map(|callback| Rc::new(callback) as PressCallback),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    fn is_interactive(&self) -> bool {
        self.on_pressed.is_some()
    }
}

impl std::fmt::Debug for FloatingActionButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatingActionButton")
            .field("enabled", &self.is_interactive())
            .finish_non_exhaustive()
    }
}

/// Resolves `_RawMaterialButtonState._effectiveElevation`'s exact
/// disabled→pressed→hovered→focused→default if-chain — see the module docs.
/// Kept as a free function (not inlined into `build`) so the chain order is
/// independently unit-testable against hand-built [`WidgetStates`] values.
fn resolve_elevation(states: &WidgetStates) -> f32 {
    if states.contains_state(WidgetState::Disabled) {
        ELEVATION_DISABLED
    } else if states.contains_state(WidgetState::Pressed) {
        ELEVATION_PRESSED
    } else if states.contains_state(WidgetState::Hovered) {
        ELEVATION_HOVERED
    } else if states.contains_state(WidgetState::Focused) {
        ELEVATION_FOCUSED
    } else {
        ELEVATION_DEFAULT
    }
}

/// Persistent state behind [`FloatingActionButton`] — owns the
/// [`WidgetStatesController`] shared with the inner [`InkWell`], mirroring
/// `ButtonStyleButtonCoreState`'s sync-before-listen pattern (see that
/// type's docs for why the ordering is load-bearing) so `resolve_elevation`
/// always sees the real, lifecycle-synced `Disabled` bit on the first build.
pub struct FloatingActionButtonState {
    states: WidgetStatesController,
    states_listener: Option<ListenerId>,
    initially_enabled: bool,
    rebuild: Option<RebuildHandle>,
}

impl std::fmt::Debug for FloatingActionButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatingActionButtonState")
            .field("states", &self.states)
            .field("initially_enabled", &self.initially_enabled)
            .finish_non_exhaustive()
    }
}

impl StatefulView for FloatingActionButton {
    type State = FloatingActionButtonState;

    fn create_state(&self) -> Self::State {
        FloatingActionButtonState {
            states: WidgetStatesController::default(),
            states_listener: None,
            initially_enabled: self.is_interactive(),
            rebuild: None,
        }
    }
}

impl ViewState<FloatingActionButton> for FloatingActionButtonState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let rebuild = ctx.rebuild_handle();

        self.states
            .update(WidgetState::Disabled, !self.initially_enabled);

        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(Arc::new(move || {
            rebuild_for_listener.schedule();
        })));

        self.rebuild = Some(rebuild);
    }

    fn did_update_view(
        &mut self,
        old_view: &FloatingActionButton,
        new_view: &FloatingActionButton,
    ) {
        if new_view.is_interactive() != old_view.is_interactive() {
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());
        }
    }

    fn build(&self, view: &FloatingActionButton, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        let states = self.states.value();

        let elevation = resolve_elevation(&states);
        let shape = fab_shape();

        let overlay_base = colors.on_primary_container;
        let overlay_color = WidgetStateProperty::resolve_with(move |states: &WidgetStates| {
            pressed_hovered_focused_overlay(states, overlay_base)
        });

        let icon = IconTheme::new(
            IconThemeData {
                color: Some(colors.on_primary_container),
                size: Some(FAB_ICON_SIZE),
                ..IconThemeData::default()
            },
            view.child.clone(),
        );

        let mut ink_well = InkWell::new(icon)
            .shape(shape)
            .overlay_color(overlay_color)
            .states_controller(self.states.clone());
        if let Some(on_pressed) = view.on_pressed.clone() {
            ink_well = ink_well.on_tap(move || on_pressed());
        }

        let constraints = BoxConstraints::tight(Size::new(px(FAB_SIZE), px(FAB_SIZE)));

        ConstrainedBox::new(constraints).child(
            Material::new(colors.primary_container)
                .elevation(elevation)
                .shape(shape)
                .child(ink_well),
        )
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_some_handler_is_interactive() {
        let fab = FloatingActionButton::new(Some(|| {}), flui_widgets::SizedBox::shrink());
        assert!(fab.is_interactive());
    }

    #[test]
    fn new_with_none_handler_is_not_interactive() {
        let fab = FloatingActionButton::new(None::<fn()>, flui_widgets::SizedBox::shrink());
        assert!(!fab.is_interactive());
    }

    #[test]
    fn debug_reports_whether_the_button_is_enabled_without_the_closure() {
        let debug = format!(
            "{:?}",
            FloatingActionButton::new(Some(|| {}), flui_widgets::SizedBox::shrink())
        );
        assert!(debug.contains("enabled: true"));
    }

    #[test]
    fn fab_shape_is_a_16dp_rounded_rectangle_not_a_circle_or_stadium() {
        let shape = fab_shape();
        match shape {
            MaterialShape::RoundedRect(radius) => {
                assert_eq!(radius.top_left, Radius::circular(px(16.0)));
                assert_eq!(radius.top_right, Radius::circular(px(16.0)));
                assert_eq!(radius.bottom_right, Radius::circular(px(16.0)));
                assert_eq!(radius.bottom_left, Radius::circular(px(16.0)));
            }
            MaterialShape::Stadium => panic!(
                "the regular M3 FAB's shape must be a 16dp RoundedRect, not the M2 CircleBorder \
                 approximated by Stadium"
            ),
        }
    }

    /// Oracle citation: `_RawMaterialButtonState._effectiveElevation`
    /// (`button.dart`, tag `3.44.0`) resolved against `_FABDefaultsM3`'s
    /// per-state elevation table — the parity core this test exists to
    /// prove, including the case (`disabled`) where FLUI's own button
    /// family's own `_TokenDefaultsM3` tables would zero out but the FAB's
    /// does not.
    #[test]
    fn resolve_elevation_matches_the_fab_defaults_m3_state_table() {
        let none = WidgetStates::NONE;
        let disabled = WidgetStates::from(WidgetState::Disabled);
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let hovered = WidgetStates::from(WidgetState::Hovered);
        let focused = WidgetStates::from(WidgetState::Focused);

        assert_eq!(resolve_elevation(&none), 6.0, "enabled default is 6.0");
        assert_eq!(
            resolve_elevation(&disabled),
            6.0,
            "disabled elevation falls back to the enabled default (RawMaterialButton's \
             disabledElevation ?? elevation), NOT zero — matching the oracle's own warning that \
             a disabled FAB has no visual indication",
        );
        assert_eq!(
            resolve_elevation(&pressed),
            6.0,
            "highlightElevation is 6.0"
        );
        assert_eq!(resolve_elevation(&hovered), 8.0, "hoverElevation is 8.0");
        assert_eq!(resolve_elevation(&focused), 6.0, "focusElevation is 6.0");
    }

    /// Mutation-honest ordered-chain coverage: `disabled` must be checked
    /// BEFORE `pressed`/`hovered`/`focused` — a disabled-and-hovered state
    /// (e.g. a stale hover left over from before the handler was removed)
    /// must still resolve the disabled tier, not hover's `8.0`.
    #[test]
    fn disabled_takes_precedence_over_a_combined_hovered_state() {
        let disabled_and_hovered =
            WidgetStates::from(WidgetState::Disabled).with_state(WidgetState::Hovered);
        assert_eq!(resolve_elevation(&disabled_and_hovered), 6.0);
    }

    /// Mutation-honest ordered-chain coverage: `pressed` must be checked
    /// BEFORE `hovered` — a combined pressed+hovered state resolves through
    /// the pressed branch (`6.0`), not hover's higher `8.0`. This is the one
    /// combined-state assertion that actually distinguishes two DIFFERENT
    /// values in this table (every other adjacent pair shares `6.0`), so it
    /// is the strongest single proof the if-chain order (not just its
    /// values) is preserved.
    #[test]
    fn pressed_takes_precedence_over_a_combined_hovered_state() {
        let pressed_and_hovered =
            WidgetStates::from(WidgetState::Pressed).with_state(WidgetState::Hovered);
        assert_eq!(
            resolve_elevation(&pressed_and_hovered),
            ELEVATION_PRESSED,
            "pressed (highlightElevation, 6.0) must win over hovered (hoverElevation, 8.0) — \
             deleting the pressed branch, or reordering it after hovered, would resolve this to \
             8.0 instead",
        );
    }

    /// Mutation-honest ordered-chain coverage: `hovered` must be checked
    /// BEFORE `focused`.
    #[test]
    fn hovered_takes_precedence_over_a_combined_focused_state() {
        let hovered_and_focused =
            WidgetStates::from(WidgetState::Hovered).with_state(WidgetState::Focused);
        assert_eq!(resolve_elevation(&hovered_and_focused), ELEVATION_HOVERED);
    }
}
