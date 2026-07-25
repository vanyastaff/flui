//! [`SnackBar`] — a lightweight message with an optional action, shown via
//! [`crate::ScaffoldMessengerHandle::show_snack_bar`] — and
//! [`SnackBarAction`], its single-fire action button.
//!
//! # Flutter parity
//!
//! `material/snack_bar.dart`'s `SnackBar`/`SnackBarAction`/`_SnackBarState`
//! (oracle tag `3.44.0`), narrowed to `SnackBarBehavior.fixed` — see the
//! module docs' "V1 scope" section. M3 token values are `_SnackbarDefaultsM3`
//! (`snack_bar.dart:941-999`), cited per value below.
//!
//! ## Entrance animation
//!
//! Fixed behavior always takes the oracle's LAST transition branch
//! (`_SnackBarState.build`, `:850-875`): `accessibleNavigation` is not ported
//! (no `MediaQuery.accessibleNavigation` consumer exists yet), and
//! `isFloatingSnackBar` is always `false` (fixed-only), so the
//! `ValueListenableBuilder`-over-`_heightAnimation` branch is the only one
//! this substrate ever takes — an `Align(alignment: topStart, heightFactor:
//! value)` grown by [`Curves::FastOutSlowIn`] (`_snackBarHeightCurve`,
//! `:30`), applied symmetrically on entrance and exit (the oracle sets no
//! `reverseCurve` on `_heightAnimation`, `:576`). Wrapped in
//! [`AnimatedBuilder`] rather than a `ValueListenableBuilder`+`CurvedAnimation`
//! pair — this substrate applies the curve inline in the builder closure,
//! which is equivalent and needs no extra `CurvedAnimation` object.
//! **`FadeTransition` is never used**: the oracle's inner `Material`-wrapping
//! fade (`_fadeOutAnimation`) is skipped whenever `accessibleNavigation ||
//! useMaterial3` (`:809`) — this substrate is M3-only, so that condition is
//! always true.
//!
//! ## V1 scope
//!
//! `SnackBarBehavior` is **not exposed** — fixed only, wrapped in
//! `SafeArea(top: false)` unconditionally (oracle: `:790-792`), no
//! `width`/`margin`/floating positioning, no action-overflow-to-a-second-row
//! layout (`actionOverflowThreshold`, `:735-740` — the action always shares
//! the content's row; a very long label may visually crowd the content,
//! which the oracle avoids by wrapping — a named, cosmetic-only
//! simplification). No close icon (`showCloseIcon` — pulls in
//! `SnackBarClosedReason::Dismiss`, which this substrate has no other
//! producer for yet either). No `Hero`/`Dismissible`/swipe-to-dismiss — see
//! `crate::scaffold_messenger`'s module doc for the reachable
//! [`crate::SnackBarClosedReason`] set. `SnackBarThemeData` — no such
//! component-theme extension slot exists in [`crate::ThemeData`] yet (same
//! deferral shape as `crate::drawer`'s `DrawerTheme` note); every value below
//! is the hand-coded M3 default with no theme/instance override lens beyond
//! [`SnackBar::background_color`]/[`SnackBar::elevation`].

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Curve, Curves};
use flui_foundation::Listenable;
use flui_types::geometry::px;
use flui_types::painting::Clip;
use flui_types::{Alignment, Color, EdgeInsets};
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_widgets::{
    Align, AnimatedBuilder, ClipRect, DefaultTextStyle, Expanded, Padding, Row, SafeArea,
    WidgetStateProperty,
};

use crate::button_style::ButtonStyle;
use crate::material::Material;
use crate::scaffold_messenger::{ScaffoldMessengerScope, SnackBarClosedReason};
use crate::text_button::TextButton;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// `_snackBarDisplayDuration` (`snack_bar.dart:29`) — the default
/// [`SnackBar::duration`].
const DEFAULT_DISPLAY_DURATION: Duration = Duration::from_secs(4);
/// `_singleLineVerticalPadding` (`snack_bar.dart:27`).
const SINGLE_LINE_VERTICAL_PADDING: f32 = 14.0;
/// `_SnackbarDefaultsM3.elevation` (`snack_bar.dart:980`).
const DEFAULT_ELEVATION: f32 = 6.0;
/// Fixed behavior's content horizontal padding — `isFloatingSnackBar ? 16.0 :
/// 24.0` (`snack_bar.dart:687`), always the `false` branch here.
const HORIZONTAL_PADDING: f32 = 24.0;

/// A lightweight message with an optional action, briefly shown near the
/// bottom of the screen via
/// [`crate::ScaffoldMessengerHandle::show_snack_bar`].
///
/// Flutter parity: `SnackBar` (`snack_bar.dart`, oracle tag `3.44.0`). See
/// the module docs for the fixed-only V1 scope.
///
/// # Examples
///
/// ```rust
/// use flui_material::SnackBar;
/// use flui_widgets::Text;
///
/// let _snack_bar = SnackBar::new(Text::new("Saved")).duration(std::time::Duration::from_secs(2));
/// ```
#[derive(Clone)]
pub struct SnackBar {
    content: BoxedView,
    action: Option<SnackBarAction>,
    duration: Duration,
    background_color: Option<Color>,
    elevation: Option<f32>,
}

impl SnackBar {
    /// A snack bar showing `content`, `DEFAULT_DISPLAY_DURATION` (4000ms),
    /// no action, M3 default colors/elevation.
    #[must_use]
    pub fn new(content: impl IntoView) -> Self {
        Self {
            content: content.into_view().boxed(),
            action: None,
            duration: DEFAULT_DISPLAY_DURATION,
            background_color: None,
            elevation: None,
        }
    }

    /// Sets the action button, shown at the row's end.
    #[must_use]
    pub fn action(mut self, action: SnackBarAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Overrides how long the snack bar stays visible before auto-dismissing.
    /// Defaults to `DEFAULT_DISPLAY_DURATION` (4000ms) — Flutter's
    /// `snackBar.duration`.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Overrides the background color. Defaults to
    /// `ColorScheme.inverseSurface`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the elevation (must be non-negative). Defaults to
    /// `DEFAULT_ELEVATION` (6.0).
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        debug_assert!(elevation >= 0.0, "SnackBar elevation must be non-negative");
        self.elevation = Some(elevation);
        self
    }

    /// The configured display duration — what
    /// `crate::scaffold_messenger::MessengerCore::start_display_timer` drives
    /// the per-snackbar timer controller with.
    #[must_use]
    pub(crate) fn configured_duration(&self) -> Duration {
        self.duration
    }
}

impl std::fmt::Debug for SnackBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackBar")
            .field("has_action", &self.action.is_some())
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// A button for a [`SnackBar`], known as an "action". Single-fire: a second
/// press after the first is ignored, and the button visually disables
/// (`disabledTextColor`) once pressed.
///
/// Flutter parity: `SnackBarAction`/`_SnackBarActionState` (`snack_bar.dart`,
/// oracle tag `3.44.0`).
///
/// # Examples
///
/// ```rust
/// use flui_material::SnackBarAction;
///
/// let _action = SnackBarAction::new("UNDO", || {});
/// ```
#[derive(Clone, StatefulView)]
pub struct SnackBarAction {
    label: String,
    on_pressed: std::rc::Rc<dyn Fn()>,
}

impl SnackBarAction {
    /// An action labeled `label`, calling `on_pressed` at most once when
    /// pressed.
    #[must_use]
    pub fn new(label: impl Into<String>, on_pressed: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            on_pressed: std::rc::Rc::new(on_pressed),
        }
    }
}

impl std::fmt::Debug for SnackBarAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackBarAction")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// State for [`SnackBarAction`] — the `_haveTriggeredAction` single-fire
/// latch. `triggered` is `Rc<Cell<bool>>`, not a plain `bool`: the press
/// closure is `'static` (outlives the `&self` borrow `build` takes) and must
/// flip the latch itself, matching the oracle's `setState(() {
/// _haveTriggeredAction = true; })` inside `_handlePressed`.
#[derive(Debug, Default)]
pub struct SnackBarActionState {
    triggered: Rc<Cell<bool>>,
    /// Acquired in [`init_state`](ViewState::init_state), per ADR-0018
    /// (trigger #22: a frame-phase-only capability may not be acquired from
    /// `build`, even to hand straight to a press closure) — `None` only in
    /// the window between `create_state` and the first `init_state`, never
    /// observed by `build`.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for SnackBarAction {
    type State = SnackBarActionState;

    fn create_state(&self) -> Self::State {
        SnackBarActionState::default()
    }
}

impl ViewState<SnackBarAction> for SnackBarActionState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &SnackBarAction, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        // `_SnackbarDefaultsM3.actionTextColor`/`.disabledActionTextColor`
        // (`snack_bar.dart:952-970`) both resolve to `inversePrimary` — the
        // disabled state is distinguished visually by the button becoming
        // unpressable, not by a different disabled color, matching the
        // oracle's own (slightly surprising, but verbatim) token table.
        let foreground_color = WidgetStateProperty::all(Some(colors.inverse_primary));
        let style = ButtonStyle {
            foreground_color: Some(foreground_color),
            ..ButtonStyle::default()
        };

        let mut button = TextButton::new(flui_widgets::Text::new(view.label.clone())).style(style);
        if !self.triggered.get() {
            let messenger = ScaffoldMessengerScope::maybe_of(ctx);
            let on_pressed = view.on_pressed.clone();
            let triggered = Rc::clone(&self.triggered);
            let rebuild = self
                .rebuild
                .clone()
                .expect("BUG: init_state runs before the first build");
            button = button.on_pressed(move || {
                triggered.set(true);
                on_pressed();
                if let Some(messenger) = &messenger {
                    messenger.hide_current_snack_bar_because(SnackBarClosedReason::Action);
                }
                rebuild.schedule(flui_view::RebuildReason::StateChange);
            });
        }
        button
    }
}

/// Builds the static (non-animated) visual content of a [`SnackBar`]:
/// `Material(color, elevation) > SafeArea(top: false) > Padding > Row[content,
/// action?]`. Flutter parity: `_SnackBarState.build`'s content assembly
/// (`snack_bar.dart:622-826`), narrowed to the fixed-behavior branches (see
/// the module docs).
fn build_content(snack_bar: &SnackBar, theme: &ThemeData) -> BoxedView {
    let colors = theme.color_scheme;
    let background_color = snack_bar.background_color.unwrap_or(colors.inverse_surface);
    let elevation = snack_bar.elevation.unwrap_or(DEFAULT_ELEVATION);
    let content_text_style = theme
        .text_theme
        .body_medium
        .clone()
        .unwrap_or_default()
        .with_color(colors.on_inverse_surface);

    let has_action = snack_bar.action.is_some();
    let content_padding = EdgeInsets::new(
        px(0.0),
        if has_action {
            px(0.0)
        } else {
            px(HORIZONTAL_PADDING)
        },
        px(0.0),
        px(HORIZONTAL_PADDING),
    );

    let mut row_children: Vec<BoxedView> = vec![
        Expanded::new(
            Padding::new(EdgeInsets::symmetric(
                px(SINGLE_LINE_VERTICAL_PADDING),
                px(0.0),
            ))
            .child(DefaultTextStyle::new(
                content_text_style,
                snack_bar.content.clone(),
            )),
        )
        .boxed(),
    ];
    if let Some(action) = &snack_bar.action {
        row_children.push(
            Padding::new(EdgeInsets::symmetric(px(0.0), px(HORIZONTAL_PADDING / 2.0)))
                .child(action.clone())
                .boxed(),
        );
    }

    let padded = Padding::new(content_padding).child(Row::new(row_children));
    let safe_area = SafeArea::new().top(false).child(padded);

    Material::new(background_color)
        .elevation(elevation)
        .clip_behavior(Clip::HardEdge)
        .child(safe_area)
        .boxed()
}

/// The [`SnackBar`] presenter mounted at `crate::scaffold::SLOT_SNACK_BAR` —
/// wraps [`build_content`] in the [`Curves::FastOutSlowIn`] `heightFactor`
/// entrance/exit animation, re-rendering on every `animation` tick via
/// [`AnimatedBuilder`]. See the module docs' "Entrance animation" section.
#[derive(Clone, StatelessView)]
pub(crate) struct SnackBarPresenter {
    snack_bar: SnackBar,
    animation: AnimationController,
}

impl SnackBarPresenter {
    pub(crate) fn new(snack_bar: SnackBar, animation: AnimationController) -> Self {
        Self {
            snack_bar,
            animation,
        }
    }
}

impl std::fmt::Debug for SnackBarPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackBarPresenter").finish_non_exhaustive()
    }
}

impl StatelessView for SnackBarPresenter {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let content = build_content(&self.snack_bar, &theme);
        let animation = self.animation.clone();
        let listenable = Arc::new(self.animation.clone()) as Arc<dyn Listenable>;

        // Flutter parity: `_SnackBarState.build`'s outermost wrap is
        // `ClipRect(clipBehavior: widget.clipBehavior, child: snackBarTransition)`
        // (`snack_bar.dart:877-881`), around the exact `Align(heightFactor:)`
        // transition this builds. `Align`/`RenderPositionedBox` lays its
        // child out LOOSE at that child's full natural height regardless of
        // `heightFactor` — only `Align`'s own REPORTED size shrinks — and
        // paints the child unclipped at that full size in `Align`'s local
        // coordinate space. Mid-entrance (a partially-grown `heightFactor`),
        // that full-height paint bleeds past `Align`'s own (still-shrunk)
        // box into whatever paints below it (an adjacent scaffold's own
        // snack bar, in the multi-scaffold case) unless something clips to
        // `Align`'s reported box. `ClipRect`'s own layout is a pass-through
        // (it reports exactly its child's size), so wrapping it here clips
        // paint to the CURRENT animated height every tick, not a stale one.
        ClipRect::new().child(AnimatedBuilder::new(listenable, move || {
            let height_factor = Curves::FastOutSlowIn.transform(animation.value().clamp(0.0, 1.0));
            Align::new(Alignment::TOP_LEFT)
                .height_factor(height_factor)
                .child(content.clone())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_snack_bar_defaults_to_the_display_duration_and_no_action() {
        let snack_bar = SnackBar::new(flui_widgets::Text::new("hi"));
        assert_eq!(snack_bar.configured_duration(), DEFAULT_DISPLAY_DURATION);
        assert!(snack_bar.action.is_none());
    }

    #[test]
    fn duration_builder_overrides_the_default() {
        let snack_bar =
            SnackBar::new(flui_widgets::Text::new("hi")).duration(Duration::from_secs(2));
        assert_eq!(snack_bar.configured_duration(), Duration::from_secs(2));
    }

    #[test]
    fn action_builder_attaches_the_action() {
        let snack_bar =
            SnackBar::new(flui_widgets::Text::new("hi")).action(SnackBarAction::new("UNDO", || {}));
        assert!(snack_bar.action.is_some());
    }
}
