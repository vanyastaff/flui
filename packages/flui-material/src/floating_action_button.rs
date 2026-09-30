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
//! overridden by `_FABDefaultsM3`, so `FloatingActionButton.build`'s own
//! `disabledElevation ?? floatingActionButtonTheme.disabledElevation ??
//! defaults.disabledElevation ?? elevation` fallback resolves it to `6.0`
//! too — **not** `RawMaterialButton`'s constructor, whose own
//! `disabledElevation` parameter defaults flatly to `0.0` with no fallback
//! chain at all; `FloatingActionButton.build` always passes an explicit,
//! already-resolved value, overriding that flat default) — so
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

use std::sync::Arc;

use flui_sdk::foundation::{Listenable, ListenerId};
use flui_sdk::geometry::Radius;
use flui_sdk::geometry::Size;
use flui_sdk::painting::BorderRadius;
use flui_sdk::painting::Color;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Center, ConstrainedBox, IconTheme, IconThemeData, WidgetState, WidgetStateProperty,
    WidgetStates, WidgetStatesController,
};

use crate::button_style_button::PressCallback;
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The regular (non-mini) floating action button's side length. Flutter
/// parity: `_FABDefaultsM3.sizeConstraints`, `BoxConstraints.tightFor(width:
/// 56.0, height: 56.0)`.
pub const FAB_SIZE: f64 = 56.0;

/// The regular variant's icon side length. Flutter parity:
/// `_FABDefaultsM3.iconSize` for `_FloatingActionButtonType.regular`.
pub const FAB_ICON_SIZE: f64 = 24.0;

/// The enabled default AND disabled elevation (they coincide — see the
/// module docs). Flutter parity: `_FABDefaultsM3`'s `elevation: 6.0`. The
/// `enabled` value this constant provides is itself theme-overridable (see
/// [`crate::theme_data::FabThemeData::elevation`]) — [`resolve_elevation`]
/// takes the (possibly overridden) effective value as a parameter rather
/// than closing over this constant directly, so a theme override reaches
/// both the `disabled` tier and the enabled-default fallback tier, matching
/// `FloatingActionButton.build`'s own `disabledElevation ?? … ?? elevation`
/// fallback chain (NOT `RawMaterialButton`'s constructor, whose own
/// `disabledElevation` parameter flatly defaults to `0.0` — see the module
/// docs' "The elevation chain" section) — `_FABDefaultsM3` never overrides
/// `disabledElevation`, so it resolves to the same (possibly theme-resolved)
/// enabled value.
const ELEVATION_DEFAULT: f64 = 6.0;
/// Flutter parity: `_FABDefaultsM3`'s `focusElevation: 6.0`. No
/// `focus_elevation` theme slot exists (named deferral, see
/// `crate::theme_data::FabThemeData`'s doc comment), so this constant is
/// never theme-overridden.
const ELEVATION_FOCUSED: f64 = 6.0;
/// Flutter parity: `_FABDefaultsM3`'s `hoverElevation: 8.0`. Same named
/// deferral as [`ELEVATION_FOCUSED`] — no `hover_elevation` theme slot.
const ELEVATION_HOVERED: f64 = 8.0;
/// The pressed elevation — Flutter's `highlightElevation`. Flutter parity:
/// `_FABDefaultsM3`'s `highlightElevation: 6.0`. Same named deferral as
/// [`ELEVATION_FOCUSED`] — no `highlight_elevation` theme slot.
const ELEVATION_PRESSED: f64 = 6.0;

/// The regular M3 FAB's shape: a rectangle with a 16dp corner radius —
/// **not** a circle. See the module docs' "M3 shape" section.
fn fab_shape() -> MaterialShape {
    MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(16.0)))
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
/// use flui_sdk::widgets::Text;
///
/// let _fab = FloatingActionButton::new(Text::new("+")).on_pressed(|_cx| {});
/// let _disabled = FloatingActionButton::new(Text::new("+"));
/// ```
#[derive(Clone, StatefulView)]
pub struct FloatingActionButton {
    on_pressed: Option<PressCallback>,
    child: BoxedView,
}

impl FloatingActionButton {
    /// A regular floating action button around `child` (typically an
    /// [`Icon`](flui_sdk::widgets::Icon)). Without [`Self::on_pressed`] the
    /// button — Flutter parity: "If the `onPressed` callback is null, then
    /// the button will be disabled" (see the module docs' elevation-chain
    /// section for why that carries no visual indication here either).
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            on_pressed: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Enables the button and sets its event handler.
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(crate::event_callback::press_callback(callback));
        self
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
/// `enabled_elevation` is the already-resolved widget/theme/default cascade
/// for the enabled tier (see [`resolve_colors`]'s sibling resolution and
/// [`FloatingActionButtonState::build`]'s call site); both the `disabled`
/// branch and the unconditional fallback return it, matching the oracle's
/// own `disabledElevation ?? … ?? elevation` chain (see [`ELEVATION_DEFAULT`]'s
/// doc comment). Kept as a free function (not inlined into `build`) so the
/// chain order is independently unit-testable against hand-built
/// [`WidgetStates`] values.
fn resolve_elevation(states: &WidgetStates, enabled_elevation: f64) -> f64 {
    if states.contains_state(WidgetState::Disabled) {
        enabled_elevation
    } else if states.contains_state(WidgetState::Pressed) {
        ELEVATION_PRESSED
    } else if states.contains_state(WidgetState::Hovered) {
        ELEVATION_HOVERED
    } else if states.contains_state(WidgetState::Focused) {
        ELEVATION_FOCUSED
    } else {
        enabled_elevation
    }
}

/// [`FloatingActionButton`]'s theme-resolved background/foreground colors
/// and enabled-tier elevation — see [`resolve_elevation`] for how the
/// elevation value feeds the state chain. Flutter parity: `this
/// .foregroundColor ?? floatingActionButtonTheme.foregroundColor ??
/// defaults.foregroundColor!` (and the `backgroundColor`/`elevation`
/// equivalents), `floating_action_button.dart`, oracle tag `3.44.0`,
/// narrowed to FLUI's `FabThemeData` slots. No per-instance widget-level
/// override exists yet for any of the three (a named V1 deferral — see the
/// module docs), so this cascade is theme → default only.
struct ResolvedFabStyle {
    background_color: Color,
    foreground_color: Color,
    elevation: f64,
}

fn resolve_colors(theme: &ThemeData) -> ResolvedFabStyle {
    let colors = theme.color_scheme;
    let fab_theme = theme.floating_action_button_theme.as_ref();

    ResolvedFabStyle {
        background_color: fab_theme
            .and_then(|t| t.background_color)
            .unwrap_or(colors.primary_container),
        foreground_color: fab_theme
            .and_then(|t| t.foreground_color)
            .unwrap_or(colors.on_primary_container),
        elevation: fab_theme
            .and_then(|t| t.elevation)
            .unwrap_or(ELEVATION_DEFAULT),
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rebuild = ctx.rebuild_handle();

        self.states
            .update(WidgetState::Disabled, !self.initially_enabled);

        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(Arc::new(move || {
            rebuild_for_listener.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));

        self.rebuild = Some(rebuild);
    }

    /// Re-syncs `WidgetState::Disabled` when `on_pressed` presence changes
    /// across a swap — mirroring `ButtonStyleButtonCoreState::did_update_view`.
    /// **Currently unobservable through any wired consumer**: the inner
    /// `InkWell`'s tap gating derives from `view.on_pressed` directly (read
    /// fresh every `build`), not from this shared bit, and `resolve_elevation`'s
    /// `disabled` branch resolves the SAME value as the enabled default (see
    /// the module docs' "The elevation chain" section) — so nothing
    /// currently distinguishes this resync having run from it never running.
    /// Kept for structural parity with the oracle's own `didUpdateWidget`
    /// and in case a future consumer of this states controller needs it.
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
        let ResolvedFabStyle {
            background_color,
            foreground_color,
            elevation: enabled_elevation,
        } = resolve_colors(&theme);
        let states = self.states.value();

        let elevation = resolve_elevation(&states, enabled_elevation);
        let shape = fab_shape();

        // The overlay ramp's base color is `_FABDefaultsM3`'s own
        // `splashColor`/`focusColor`/`hoverColor` — independent oracle
        // fields (see `pressed_hovered_focused_overlay`'s call site here),
        // NOT derived from the resolved `foreground_color` above. FLUI
        // exposes no theme slot for them (named deferral, see
        // `crate::theme_data::FabThemeData`'s doc comment), so this stays
        // pinned to the M3 default regardless of a `foreground_color`
        // theme/widget override.
        let overlay_base = theme.color_scheme.on_primary_container;
        let overlay_color = WidgetStateProperty::resolve_with(move |states: &WidgetStates| {
            pressed_hovered_focused_overlay(states, overlay_base)
        });

        let icon = IconTheme::new(
            IconThemeData {
                color: Some(foreground_color),
                size: Some(FAB_ICON_SIZE),
                ..IconThemeData::default()
            },
            view.child.clone(),
        );

        // Center the icon within the FAB's tight 56×56 box. Flutter's
        // `RawMaterialButton` wraps the child in `Center(widthFactor: 1.0,
        // heightFactor: 1.0)`; without it the icon pins to the top-left of the
        // constrained box instead of the middle.
        let mut ink_well = InkWell::new(Center::new().child(icon))
            .shape(shape)
            .overlay_color(overlay_color)
            .states_controller(self.states.clone());
        if let Some(on_pressed) = view.on_pressed.clone() {
            ink_well = ink_well.on_tap(move |cx| on_pressed(cx));
        }

        let constraints = BoxConstraints::tight(Size::new(FAB_SIZE, FAB_SIZE));

        ConstrainedBox::new(constraints).child(
            Material::new(background_color)
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
