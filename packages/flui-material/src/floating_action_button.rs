//! [`FloatingActionButton`] — a circular-in-spirit, M3 rounded-square button
//! that hovers over content to promote a primary action.

use flui_sdk::foundation::{Listenable, Subscription};
use flui_sdk::geometry::Radius;
use flui_sdk::geometry::Size;
use flui_sdk::painting::styling::BorderRadius;
use flui_sdk::painting::styling::Color;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::semantics::SemanticsRole;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::__private::node::Description;
use flui_sdk::widgets::{
    Center, ConstrainedBox, IconTheme, IconThemeData, WidgetState, WidgetStateProperty,
    WidgetStates, WidgetStatesController,
};

use crate::button_style_button::PressCallback;
use crate::elevated_button::pressed_hovered_overlay;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The regular (non-mini) floating action button's side length: tight
/// constraints of 56.0 × 56.0.
pub(crate) const FAB_SIZE: f64 = 56.0;

/// The regular variant's icon side length.
pub(crate) const FAB_ICON_SIZE: f64 = 24.0;

/// The enabled default AND disabled elevation (they coincide — see the
/// [`FloatingActionButton`]'s docs). The M3 default is `elevation: 6.0`. The
/// `enabled` value this constant provides is itself theme-overridable (see
/// [`crate::theme_data::FabThemeData::elevation`]) — [`resolve_elevation`]
/// takes the (possibly overridden) effective value as a parameter rather
/// than closing over this constant directly, so a theme override reaches
/// both the `disabled` tier and the enabled-default fallback tier, following
/// the `disabledElevation ?? … ?? elevation` fallback chain (not a flat
/// `0.0` — see [`FloatingActionButton`]'s "The elevation chain" section) — the M3
/// defaults never override `disabledElevation`, so it resolves to the same
/// (possibly theme-resolved) enabled value.
const ELEVATION_DEFAULT: f64 = 6.0;
/// The M3 default `focusElevation: 6.0`. No
/// `focus_elevation` theme slot exists (named deferral, see
/// `crate::theme_data::FabThemeData`'s doc comment), so this constant is
/// never theme-overridden.
const ELEVATION_FOCUSED: f64 = 6.0;
/// The M3 default `hoverElevation: 8.0`. Same named
/// deferral as [`ELEVATION_FOCUSED`] — no `hover_elevation` theme slot.
const ELEVATION_HOVERED: f64 = 8.0;
/// The pressed elevation (`highlightElevation: 6.0`). Same named deferral as
/// [`ELEVATION_FOCUSED`] — no `highlight_elevation` theme slot.
const ELEVATION_PRESSED: f64 = 6.0;

/// The regular M3 FAB's shape: a rectangle with a 16dp corner radius —
/// **not** a circle. See [`FloatingActionButton`]'s "M3 shape" section.
fn fab_shape() -> MaterialShape {
    MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(16.0)))
}

/// A circular-in-spirit floating action button — hovers over content to
/// promote a primary action, typically mounted via
/// [`Scaffold::floating_action_button`](crate::scaffold::Scaffold::floating_action_button).
///
/// See [`FloatingActionButton`]'s docs for the M3 default token table this crate resolves
/// verbatim, the elevation state chain, and the deferred variant list.
///
/// # Examples
///
/// ```rust
/// use flui_material::FloatingActionButton;
/// use flui_sdk::widgets::Text;
///
/// let _fab = FloatingActionButton::new().child(Text::new("+")).on_pressed(|_ctx| {});
/// let _disabled = FloatingActionButton::new().child(Text::new("+"));
/// ```
///
/// # Scope
///
/// The regular (non-mini, non-large, non-extended) variant —
/// `small`/`large`/`extended` are named deferrals; see below. The M3 FAB
/// has an elevation *state chain* — a five-way
/// `disabled`/`pressed`/`hovered`/`focused`/default cascade — which a plain
/// `ButtonStyle::elevation` slot can't express any more precisely than
/// `crate::button_style_button::ButtonStyleButtonCore`'s existing per-button
/// tables already do. So this type composes [`Material`] + [`InkWell`]
/// directly (`ConstrainedBox` → `Material(elevation, shape, color)` →
/// `InkWell`) rather than routing through `ButtonStyleButtonCore`.
///
/// # M3 shape: a rounded rectangle, not a circle
///
/// The regular variant's shape is
/// `RoundedRectangleBorder(borderRadius: BorderRadius.circular(16.0))` —
/// **not** a circle (that was the M2 shape). This crate's `fab_shape`
/// function carries this exactly.
///
/// # The elevation chain
///
/// Elevation resolves in strict precedence order — **disabled, then pressed
/// (`highlightElevation`), then hovered, then focused, then the enabled
/// default** (`elevation`) — and `resolve_elevation` preserves that exact
/// if-chain. For the regular M3 FAB every tier but hover happens to share the
/// same `6.0` value (`elevation: 6.0, focusElevation: 6.0, hoverElevation:
/// 8.0, highlightElevation: 6.0`, and the M3 defaults never override
/// `disabledElevation`, so the
/// `disabledElevation ?? floatingActionButtonTheme.disabledElevation ??
/// defaults.disabledElevation ?? elevation` fallback resolves it to `6.0`
/// too, rather than a flat `0.0`) — so
/// *values* rarely distinguish the branches, but the *order* still does: a
/// combined pressed+hovered state must resolve through the pressed branch
/// (still `6.0`), not fall through past it to hover's `8.0`. Disabling a
/// floating action button is discouraged as it gives the user no visual
/// indication — `disabledElevation` really does equal the
/// enabled default here, unlike every `ButtonStyleButtonCore` button (whose
/// M3 default elevation zeroes out when disabled).
///
/// # Colors: static, not state-resolved
///
/// Unlike `elevation`, the M3 FAB `foregroundColor`/`backgroundColor`
/// carry no per-state branch at all — `onPrimaryContainer`/`primaryContainer`
/// regardless of `disabled`/`pressed`/`hovered`/`focused`. Only the overlay
/// (splash/hover/focus tint) and the elevation vary with state; both
/// constants are applied directly, no `WidgetStateProperty` needed for them.
///
/// # Overlay ramp
///
/// `splashColor`/`focusColor` both resolve to `onPrimaryContainer` at 10%
/// opacity and `hoverColor` to 8%. The pressed(10%)/hovered(8%) shape
/// `crate::elevated_button::pressed_hovered_overlay` implements is
/// reused here rather than duplicated; the focus colour is drawn as the focus
/// ring instead of a layer (ADR-0079). See that function's own doc for the
/// pressed-before-hovered order it preserves.
///
/// # Deferred, and named
///
/// - **`small`/`large`/`extended` variants** — distinct size constraints,
///   corner radii, icon sizes, and (for `extended`) an icon+label row
///   composition. Only the regular 56×56 variant ships.
/// - **`disabled_elevation`/`focus_elevation`/`hover_elevation`/
///   `highlight_elevation` overrides**, `heroTag`/`Hero` wrapping, `tooltip`/
///   `Tooltip` wrapping, `mouseCursor`, `clipBehavior`, focus node/autofocus,
///   `materialTapTargetSize`, `enableFeedback` — no override surface yet;
///   every FAB uses the M3 default token table verbatim.
/// - **Own `foreground_color`/`background_color` overrides** — Flutter's
///   constructor-level `Color?` fields; this crate always resolves the M3
///   defaults.
#[derive(Clone, StatefulView)]
pub struct FloatingActionButton {
    on_pressed: Option<PressCallback>,
    child: Option<BoxedView>,
}

impl Default for FloatingActionButton {
    fn default() -> Self {
        Self::new()
    }
}

impl FloatingActionButton {
    /// A regular floating action button with no child. Without
    /// [`Self::on_pressed`] the button is disabled (see [`FloatingActionButton`]'s
    /// elevation-chain section for why that carries no visual indication
    /// here either). Flutter's `FloatingActionButton.child` is nullable: an
    /// empty button is its 56×56 surface.
    #[must_use]
    pub fn new() -> Self {
        Self {
            on_pressed: None,
            child: None,
        }
    }

    /// Sets the button's content, typically an
    /// [`Icon`](flui_sdk::widgets::Icon), centered and themed with the
    /// button's foreground color.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(BoxedView::new(child.into_view()));
        self
    }

    /// Enables the button and sets its event handler.
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventContext<'_>) -> R + 'static,
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
/// disabled→pressed→hovered→focused→default if-chain — see [`FloatingActionButton`]'s docs.
/// `enabled_elevation` is the already-resolved widget/theme/default cascade
/// for the enabled tier (see [`resolve_colors`]'s sibling resolution and
/// [`FloatingActionButtonState::build`]'s call site); both the `disabled`
/// branch and the unconditional fallback return it, matching Flutter's
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
    } else if states.contains_state(WidgetState::FocusVisible) {
        ELEVATION_FOCUSED
    } else {
        enabled_elevation
    }
}

/// [`FloatingActionButton`]'s theme-resolved background/foreground colors
/// and enabled-tier elevation — see [`resolve_elevation`] for how the
/// elevation value feeds the state chain. The cascade is `this
/// .foregroundColor ?? floatingActionButtonTheme.foregroundColor ??
/// defaults.foregroundColor!` (and the `backgroundColor`/`elevation`
/// equivalents), narrowed to FLUI's `FabThemeData` slots. No per-instance widget-level
/// override exists yet for any of the three (a named deferral — see the
/// [`FloatingActionButton`]'s docs), so this cascade is theme → default only.
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
/// [`WidgetStatesController`] shared with the inner [`InkWell`], seeded with
/// `Disabled` before its listener attaches, as `ButtonStyleButtonCoreState`
/// is (see that type's docs for why the ordering is load-bearing), so
/// `resolve_elevation` sees the real `Disabled` bit on the first build.
pub struct FloatingActionButtonState {
    states: WidgetStatesController,
    /// Rebuilds the button on every change of `states`; detaches when the
    /// state drops.
    _states_listener: Subscription,
}

impl std::fmt::Debug for FloatingActionButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatingActionButtonState")
            .field("states", &self.states)
            .finish_non_exhaustive()
    }
}

impl StatefulView for FloatingActionButton {
    type State = FloatingActionButtonState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let initial = if self.is_interactive() {
            WidgetStates::NONE
        } else {
            WidgetStates::from(WidgetState::Disabled)
        };
        let states = WidgetStatesController::new(initial);
        let rebuild = ctx.rebuild_handle();
        let states_listener = states.listen(std::rc::Rc::new(move || {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        }));
        FloatingActionButtonState {
            states,
            _states_listener: states_listener,
        }
    }
}

impl ViewState<FloatingActionButton> for FloatingActionButtonState {
    /// Re-syncs `WidgetState::Disabled` when `on_pressed` presence changes
    /// across a swap — mirroring `ButtonStyleButtonCoreState::did_update_view`.
    /// **Currently unobservable through any wired consumer**: the inner
    /// `InkWell`'s tap gating derives from `view.on_pressed` directly (read
    /// fresh every `build`), not from this shared bit, and `resolve_elevation`'s
    /// `disabled` branch resolves the SAME value as the enabled default (see
    /// [`FloatingActionButton`]'s "The elevation chain" section) — so nothing
    /// currently distinguishes this resync having run from it never running.
    /// Kept for structural parity with Flutter's own `didUpdateWidget`
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
        // `splashColor`/`focusColor`/`hoverColor` — independent Flutter
        // fields (see `pressed_hovered_overlay`'s call site here),
        // NOT derived from the resolved `foreground_color` above. FLUI
        // exposes no theme slot for them (named deferral, see
        // `crate::theme_data::FabThemeData`'s doc comment), so this stays
        // pinned to the M3 default regardless of a `foreground_color`
        // theme/widget override.
        let overlay_base = theme.color_scheme.on_primary_container;
        let overlay_color = WidgetStateProperty::resolve_with(move |states: &WidgetStates| {
            pressed_hovered_overlay(states, overlay_base)
        });

        // Center the icon within the FAB's tight 56×56 box; without it the
        // icon pins to the top-left of the constrained box instead of the
        // middle.
        let mut center = Center::new();
        if let Some(child) = &view.child {
            center = center.child(IconTheme::new(
                IconThemeData::default()
                    .color(foreground_color)
                    .size(FAB_ICON_SIZE),
                child.clone(),
            ));
        }
        let mut ink_well = InkWell::new()
            .child(center)
            .shape(shape)
            .overlay_color(overlay_color)
            .states_controller(self.states.clone())
            // The switch this button seeds `Disabled` from, given to the
            // well whose node writes it.
            .enabled(view.is_interactive())
            // Flutter's `RawMaterialButton` is `Semantics(button: true)`; the
            // well's switch says whether it is enabled.
            .description(Description::new(SemanticsRole::Button));
        if let Some(on_pressed) = view.on_pressed.clone() {
            ink_well = ink_well.on_tap(move |ctx| on_pressed(ctx));
        }

        let constraints = BoxConstraints::tight(Size::new(FAB_SIZE, FAB_SIZE));

        ConstrainedBox::new(constraints).child(
            Material::new()
                .color(background_color)
                .elevation(elevation)
                .shape(shape)
                .child(ink_well),
        )
    }
}
