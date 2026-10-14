//! Material 3 press feedback: the state layer and the ripple an [`InkWell`]
//! paints under its child.
//!
//! An `InkWell` owns its feedback (an *indication*, in Compose's terms):
//! nothing is registered on an ancestor `Material`, so a ripple can never
//! paint outside the well that started it. The indication has two parts:
//!
//! - **State layer.** One fill of the well's shape whose alpha follows the
//!   interaction states through [`StateLayerAlpha`]: hover 0.08, focus 0.10,
//!   pressed 0.10, dragged 0.16. Entering hover takes 15 ms, entering focus or
//!   a drag 45 ms, and leaving any state 150 ms, all linear.
//! - **Ripple.** A short queue of waves. Each wave starts at the pointer-down
//!   position with radius `0.3 × max(width, height)` and grows over 225 ms
//!   (fast-out-slow-in) to the distance from its origin to the farthest corner
//!   plus 10 dp. It fades in over 75 ms; once released it fades out over
//!   150 ms, but a release before the fade-in completes waits for it, so a
//!   quick tap still shows the full press alpha. A cancelled contact fades out
//!   at once from where its fade-in had got to, and so does a contact that
//!   stops being a tap (past the touch slop, or lost to a scrolling
//!   ancestor). A keyboard activation grows a released wave from the centre.
//!   A box too large for a wave's extent to be finite grows no wave.
//!
//! While a wave runs it carries the pressed alpha, so the state layer leaves
//! `Pressed` out; a press with no wave (a shared states controller, an
//! assistive activation) shows on the state layer. Under reduced motion there
//! is no expanding circle: the state layer shows `Pressed` from the contact's
//! down to its up, and through the tap's pressed window after.
//!
//! An idle well attaches no painter and records nothing; a state change or a
//! press rebuilds it with one.
//!
//! [`RippleScope`] provides the ambient [`RippleConfiguration`]; a scope with
//! no configuration turns the whole indication off.
//!
//! [`InkWell`]: crate::InkWell

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_sdk::animation::prelude::*;
use flui_sdk::animation::{Vsync, curves};
use flui_sdk::foundation::{ChangeNotifier, Listenable};
use flui_sdk::geometry::{Point, Rect, Size};
use flui_sdk::painting::{Canvas, Clip, ClipOp, Color, Paint};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, RebuildHandle, impl_inherited_view};
use flui_sdk::widgets::{
    CustomPainter, WidgetState, WidgetStateProperty, WidgetStates, WidgetStatesController,
};

use crate::shape::MaterialShape;

/// The state layer's alpha per interaction state, applied to the ripple
/// colour's own alpha.
///
/// When several states hold at once, the strongest wins: dragged, then
/// pressed, then focused, then hovered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateLayerAlpha {
    /// While a pointer hovers the surface.
    pub hovered: f64,
    /// While the surface has keyboard focus.
    pub focused: f64,
    /// While the surface is pressed; also the ripple's alpha.
    pub pressed: f64,
    /// While the surface is dragged.
    pub dragged: f64,
}

impl StateLayerAlpha {
    /// The Material 3 state-layer opacity tokens.
    pub const M3: Self = Self {
        hovered: 0.08,
        focused: 0.10,
        pressed: 0.10,
        dragged: 0.16,
    };

    /// The alpha for `states`: the strongest active interaction state's, or 0
    /// when none is active.
    #[must_use]
    pub fn resolve(&self, states: WidgetStates) -> f64 {
        if states.contains_state(WidgetState::Dragged) {
            self.dragged
        } else if states.contains_state(WidgetState::Pressed) {
            self.pressed
        } else if states.contains_state(WidgetState::Focused) {
            self.focused
        } else if states.contains_state(WidgetState::Hovered) {
            self.hovered
        } else {
            0.0
        }
    }
}

impl Default for StateLayerAlpha {
    fn default() -> Self {
        Self::M3
    }
}

/// How the [`InkWell`](crate::InkWell)s below a [`RippleScope`] show
/// interaction.
///
/// `color` is the ripple and state-layer colour. A well's own
/// [`overlay_color`](crate::InkWell::overlay_color) wins over it; without
/// either, the well uses its [`Theme`](crate::Theme)'s `on_surface`, and
/// without a theme it paints no feedback.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RippleConfiguration {
    /// The feedback colour, or `None` for the well's own default.
    pub color: Option<Color>,
    /// The alpha each interaction state applies to the colour.
    pub alpha: StateLayerAlpha,
}

/// Provides a [`RippleConfiguration`] to the [`InkWell`](crate::InkWell)s
/// below it. `None` turns their ripple and state layer off.
///
/// Without a scope, wells use [`RippleConfiguration::default`].
///
/// ```rust
/// use flui_material::{RippleConfiguration, RippleScope};
/// use flui_sdk::painting::Color;
/// use flui_sdk::widgets::SizedBox;
///
/// let tinted = RippleConfiguration { color: Some(Color::RED), ..Default::default() };
/// let _scope = RippleScope::new(Some(tinted), SizedBox::shrink());
/// let _without_ripples = RippleScope::new(None, SizedBox::shrink());
/// ```
#[derive(Clone)]
pub struct RippleScope {
    configuration: Option<RippleConfiguration>,
    child: BoxedView,
}

impl RippleScope {
    /// Provide `configuration` to `child`'s subtree.
    #[must_use]
    pub fn new(configuration: Option<RippleConfiguration>, child: impl IntoView) -> Self {
        Self {
            configuration,
            child: child.into_view().boxed(),
        }
    }

    /// The nearest scope's configuration, registering a dependency; the
    /// default configuration without a scope.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Option<RippleConfiguration> {
        ctx.depend_on::<Self, _>(|scope| scope.configuration)
            .unwrap_or_else(|| Some(RippleConfiguration::default()))
    }
}

impl std::fmt::Debug for RippleScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RippleScope")
            .field("configuration", &self.configuration)
            .finish_non_exhaustive()
    }
}

impl InheritedView for RippleScope {
    type Data = Option<RippleConfiguration>;

    fn data(&self) -> &Self::Data {
        &self.configuration
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.configuration != old.configuration
    }
}

impl_inherited_view!(RippleScope);

/// State-layer fade into hover.
const HOVER_ENTER: Duration = Duration::from_millis(15);
/// State-layer fade into focus or a drag.
const FOCUS_ENTER: Duration = Duration::from_millis(45);
/// State-layer fade out of any state.
const STATE_EXIT: Duration = Duration::from_millis(150);
/// Wave fade-in, linear.
const WAVE_FADE_IN_MS: f64 = 75.0;
/// Wave radius growth, fast-out-slow-in.
const WAVE_RADIUS: Duration = Duration::from_millis(225);
/// Wave fade-out after release or cancel, linear.
const WAVE_FADE_OUT_MS: f64 = 150.0;
/// How far past the farthest corner a wave ends, in logical pixels.
const WAVE_END_OVERSHOOT: f64 = 10.0;
/// The start radius as a fraction of the longer side.
const WAVE_START_FRACTION: f64 = 0.3;
/// Waves kept at once; a press beyond it retires the oldest.
const MAX_WAVES: usize = 4;

/// What the indication resolves its colours from: the well's configuration
/// and the ambient values its state reads in `did_change_dependencies`.
#[derive(Clone)]
pub(crate) struct IndicationStyle {
    /// The well's explicit per-state colour, alpha included.
    pub(crate) overlay_color: WidgetStateProperty<Option<Color>>,
    /// The ambient configuration; `None` turns the indication off.
    pub(crate) configuration: Option<RippleConfiguration>,
    /// The colour used when neither the well nor the configuration names one.
    pub(crate) content_color: Option<Color>,
    /// Reduced motion: no waves, and the state layer shows `Pressed`.
    pub(crate) reduce_motion: bool,
}

impl IndicationStyle {
    fn ripples(&self) -> bool {
        self.configuration.is_some() && !self.reduce_motion
    }

    /// The state layer's target colour for `states`, or `None` for no layer.
    fn layer_color(&self, states: WidgetStates) -> Option<Color> {
        let configuration = self.configuration?;
        if let Some(color) = self.overlay_color.resolve(&states) {
            return Some(color);
        }
        let base = configuration.color.or(self.content_color)?;
        let alpha = configuration.alpha.resolve(states);
        (alpha > 0.0).then(|| base.with_opacity(f64::from(base.opacity()) * alpha))
    }

    /// A wave's colour at full strength.
    fn wave_color(&self, states: WidgetStates) -> Option<Color> {
        let configuration = self.configuration?;
        if let Some(color) = self
            .overlay_color
            .resolve(&states.with_state(WidgetState::Pressed))
        {
            return Some(color);
        }
        let base = configuration.color.or(self.content_color)?;
        Some(base.with_opacity(f64::from(base.opacity()) * configuration.alpha.pressed))
    }
}

/// The state layer's fade into `states`.
fn enter_duration(states: WidgetStates) -> Duration {
    if states.contains_state(WidgetState::Dragged) || states.contains_state(WidgetState::Focused) {
        FOCUS_ENTER
    } else {
        HOVER_ENTER
    }
}

/// One expanding wave of the ripple.
struct Wave {
    /// Where the wave grows from, in the well's space; `None` is the centre.
    origin: Option<Point<f64>>,
    /// The contact that started it; `None` for a keyboard activation.
    pointer: Option<NonZeroU64>,
    /// Unit progress over [`WAVE_RADIUS`] since the press.
    enter: DrivenController,
    /// The fade-out, once released or cancelled.
    exit: Option<WaveExit>,
    /// The fade-in a cancel froze.
    fade_in_cap: f64,
}

struct WaveExit {
    /// Unit progress over `total_ms`.
    run: DrivenController,
    /// The wait before the fade-out begins: the rest of the fade-in.
    delay_ms: f64,
    total_ms: f64,
}

impl Wave {
    fn elapsed_ms(&self) -> f64 {
        self.enter.controller().value().clamp(0.0, 1.0) * duration_ms(WAVE_RADIUS)
    }

    fn fade_in(&self) -> f64 {
        (self.elapsed_ms() / WAVE_FADE_IN_MS)
            .min(1.0)
            .min(self.fade_in_cap)
    }

    fn alpha(&self) -> f64 {
        let fade_out = self.exit.as_ref().map_or(1.0, |exit| {
            let elapsed = exit.run.controller().value().clamp(0.0, 1.0) * exit.total_ms;
            1.0 - ((elapsed - exit.delay_ms) / WAVE_FADE_OUT_MS).clamp(0.0, 1.0)
        });
        self.fade_in() * fade_out
    }

    fn finished(&self) -> bool {
        self.exit
            .as_ref()
            .is_some_and(|exit| exit.run.controller().status() == AnimationStatus::ForwardSettled)
    }

    /// Centre and radius in a box of `size`, or `None` when the box is too
    /// large for the wave's extent to be finite: a wave publishes no
    /// geometry it cannot represent, so such a well shows its state layer
    /// only.
    fn circle(&self, size: Size) -> Option<(Point<f64>, f64)> {
        let center = self
            .origin
            .unwrap_or_else(|| Point::new(size.width / 2.0, size.height / 2.0));
        let farthest = [
            (0.0, 0.0),
            (size.width, 0.0),
            (0.0, size.height),
            (size.width, size.height),
        ]
        .into_iter()
        .map(|(x, y)| (x - center.x).hypot(y - center.y))
        .fold(0.0, f64::max);
        let start = WAVE_START_FRACTION * size.width.max(size.height);
        let end = farthest + WAVE_END_OVERSHOOT;
        let progress = curves::FAST_OUT_SLOW_IN.transform(self.enter.controller().value());
        let radius = start + (end - start) * progress;
        let extent = [
            center.x - radius,
            center.x + radius,
            center.y - radius,
            center.y + radius,
        ];
        (radius.is_finite() && extent.iter().all(|edge| edge.is_finite()))
            .then_some((center, radius))
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// A unit run over `duration` on `vsync`, repainting through `repaint`.
/// When it settles forward, `settled` re-syncs the state layer.
fn start_run(
    label: &'static str,
    duration: Duration,
    vsync: Option<&Vsync>,
    repaint: &Rc<ChangeNotifier>,
    settled: Option<Weak<Indication>>,
) -> DrivenController {
    let owner = AnimationController::builder(duration)
        .debug_label(label)
        .build_on(vsync);
    let notifier = Rc::clone(repaint);
    // The listener lives as long as the controller, which this run owns.
    let _listener = owner
        .controller()
        .add_listener(Rc::new(move || notifier.notify_listeners()));
    if let Some(indication) = settled {
        // Weak: the run must not keep its owner alive.
        owner
            .controller()
            .subscribe_status(Rc::new(move |status| {
                if status == AnimationStatus::ForwardSettled
                    && let Some(indication) = indication.upgrade()
                {
                    indication.sync();
                }
            }))
            .detach();
    }
    if let Err(error) = owner.controller().forward_from(Some(0.0)) {
        tracing::debug!(?error, label, "InkWell ripple run failed to start");
    }
    owner
}

/// An [`InkWell`](crate::InkWell)'s state layer and ripple, shared by its
/// state, its pointer callbacks and its painter.
pub(crate) struct Indication {
    states: RefCell<WidgetStatesController>,
    style: RefCell<IndicationStyle>,
    /// The state layer's alpha.
    layer: RefCell<AnimatedValue<f64>>,
    /// The colour of the layer's latest visible target; its alpha is
    /// replaced by the animated one, so a fade-out keeps its hue.
    layer_color: Cell<Option<Color>>,
    waves: RefCell<Vec<Wave>>,
    /// Contacts held on the well: pressed for the state layer while no
    /// wave carries the press.
    contacts: RefCell<Vec<NonZeroU64>>,
    vsync: RefCell<Option<Vsync>>,
    /// Notified on every animation tick and style change.
    repaint: Rc<ChangeNotifier>,
    /// Rebuilds the well, so a press attaches the painter.
    rebuild: RefCell<Option<RebuildHandle>>,
    /// Whether the last build attached the painter.
    painted: Cell<bool>,
    /// This indication, for the runs' settle callbacks.
    this: Weak<Self>,
}

impl std::fmt::Debug for Indication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Indication")
            .field("waves", &self.waves.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Indication {
    pub(crate) fn new(states: WidgetStatesController, style: IndicationStyle) -> Rc<Self> {
        let repaint = Rc::new(ChangeNotifier::new());
        let layer = AnimatedValue::with_debug_label(
            "InkWell.state_layer",
            0.0,
            MotionSpec::default(),
            None,
        )
        .expect("BUG: a resting state layer starts at the finite alpha 0");
        let notifier = Rc::clone(&repaint);
        // The listener lives as long as the layer's controller.
        let _listener = layer
            .animation()
            .add_listener(Rc::new(move || notifier.notify_listeners()));
        Rc::new_cyclic(|this| Self {
            states: RefCell::new(states),
            style: RefCell::new(style),
            layer: RefCell::new(layer),
            layer_color: Cell::new(None),
            waves: RefCell::new(Vec::new()),
            contacts: RefCell::new(Vec::new()),
            vsync: RefCell::new(None),
            repaint,
            rebuild: RefCell::new(None),
            painted: Cell::new(false),
            this: Weak::clone(this),
        })
    }

    /// The handle a press uses to attach the painter.
    pub(crate) fn set_rebuild(&self, rebuild: RebuildHandle) {
        *self.rebuild.borrow_mut() = Some(rebuild);
    }

    /// Whether anything is, or is about to be, visible: an idle well
    /// attaches no painter and records nothing.
    pub(crate) fn is_active(&self) -> bool {
        let layer = self.layer.borrow();
        layer.value() > 0.0
            || *layer.target() > 0.0
            || self.waves.borrow().iter().any(|wave| !wave.finished())
    }

    /// Record whether this build attached the painter.
    pub(crate) fn set_painted(&self, painted: bool) {
        self.painted.set(painted);
    }

    /// The states the state layer shows: `Pressed` while a contact is held,
    /// unless a running wave carries the press.
    fn layer_states(&self, ripples: bool) -> WidgetStates {
        let mut states = self.states.borrow().value();
        if !self.contacts.borrow().is_empty() {
            states = states.with_state(WidgetState::Pressed);
        }
        if ripples && self.waves.borrow().iter().any(|wave| !wave.finished()) {
            states = states.without_state(WidgetState::Pressed);
        }
        states
    }

    /// Follow `states` from now on.
    pub(crate) fn set_states(&self, states: WidgetStatesController) {
        *self.states.borrow_mut() = states;
    }

    /// Move every run to `vsync`.
    pub(crate) fn bind(&self, vsync: Option<Vsync>) {
        if let Err(error) = self.layer.borrow_mut().rebind(vsync.as_ref()) {
            tracing::error!(%error, "InkWell state layer lost its frame registry");
        }
        for wave in self.waves.borrow_mut().iter_mut() {
            let exit = wave.exit.as_mut().map(|exit| &mut exit.run);
            for run in std::iter::once(&mut wave.enter).chain(exit) {
                if let Err(error) = run.rebind(vsync.as_ref()) {
                    tracing::error!(%error, "InkWell ripple lost its frame registry");
                }
            }
        }
        *self.vsync.borrow_mut() = vsync;
    }

    /// Replace the style, then move the state layer toward it.
    pub(crate) fn update_style(&self, update: impl FnOnce(&mut IndicationStyle)) {
        update(&mut self.style.borrow_mut());
        self.sync();
    }

    /// Move the state layer toward the current states and repaint.
    pub(crate) fn sync(&self) {
        let ripples = self.style.borrow().ripples();
        let effective = self.layer_states(ripples);
        let target = self.style.borrow().layer_color(effective);
        let target_alpha = target.map_or(0.0, |color| f64::from(color.opacity()));
        if let Some(color) = target {
            self.layer_color.set(Some(color));
        }
        let moved = {
            let mut layer = self.layer.borrow_mut();
            if (*layer.target() - target_alpha).abs() <= f64::EPSILON {
                None
            } else {
                let duration = if target_alpha > layer.value() {
                    enter_duration(effective)
                } else {
                    STATE_EXIT
                };
                let motion = MotionSpec::default()
                    .with_duration(duration)
                    .with_curve(curves::LINEAR);
                Some(layer.retarget(target_alpha, motion))
            }
        };
        if let Some(Err(error)) = moved {
            tracing::warn!(%error, "InkWell state layer refused its target");
        }
        self.repaint.notify_listeners();
    }

    /// Press at `origin` (the centre when `None`) for `pointer`: a wave while
    /// ripples run, and a held contact for the state layer.
    pub(crate) fn press(&self, origin: Option<Point<f64>>, pointer: Option<NonZeroU64>) {
        let ripples = {
            let style = self.style.borrow();
            style.configuration.is_some().then(|| style.ripples())
        };
        let Some(ripples) = ripples else {
            return;
        };
        if let Some(pointer) = pointer {
            self.contacts.borrow_mut().push(pointer);
        }
        if ripples {
            let vsync = self.vsync.borrow().clone();
            let wave = Wave {
                origin,
                pointer,
                enter: start_run(
                    "InkWell.ripple",
                    WAVE_RADIUS,
                    vsync.as_ref(),
                    &self.repaint,
                    None,
                ),
                exit: None,
                fade_in_cap: 1.0,
            };
            let retired = {
                let mut waves = self.waves.borrow_mut();
                let (finished, mut kept): (Vec<_>, Vec<_>) =
                    waves.drain(..).partition(Wave::finished);
                let overflow = kept.len().saturating_sub(MAX_WAVES - 1);
                let oldest: Vec<_> = kept.drain(..overflow).collect();
                kept.push(wave);
                *waves = kept;
                (finished, oldest)
            };
            // Retired runs drop after the queue's borrow is released.
            drop(retired);
        }
        self.sync();
        if !self.painted.get() {
            let rebuild = self.rebuild.borrow().clone();
            if let Some(rebuild) = rebuild {
                rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
            }
        }
    }

    /// End `pointer`'s press: its waves fade out once their fade-in
    /// completes.
    pub(crate) fn release(&self, pointer: Option<NonZeroU64>) {
        self.end(|held| held == pointer, false);
    }

    /// End `pointer`'s press now: its waves fade out at once.
    pub(crate) fn cancel(&self, pointer: Option<NonZeroU64>) {
        self.end(|held| held == pointer, true);
    }

    /// End every held contact's press at once: the contact stopped being a
    /// tap (moved past the slop, or lost the gesture arena).
    pub(crate) fn cancel_contacts(&self) {
        self.end(|held| held.is_some(), true);
    }

    fn end(&self, matches: impl Fn(Option<NonZeroU64>) -> bool, cancelled: bool) {
        self.contacts
            .borrow_mut()
            .retain(|&held| !matches(Some(held)));
        let vsync = self.vsync.borrow().clone();
        let mut ending = Vec::new();
        {
            let waves = self.waves.borrow();
            for (index, wave) in waves.iter().enumerate() {
                if wave.exit.is_none() && matches(wave.pointer) {
                    let delay_ms = if cancelled {
                        0.0
                    } else {
                        (WAVE_FADE_IN_MS - wave.elapsed_ms()).max(0.0)
                    };
                    ending.push((index, delay_ms, wave.fade_in()));
                }
            }
        }
        for (index, delay_ms, fade_in) in ending {
            let total_ms = delay_ms + WAVE_FADE_OUT_MS;
            let run = start_run(
                "InkWell.ripple_exit",
                Duration::from_secs_f64(total_ms / 1000.0),
                vsync.as_ref(),
                &self.repaint,
                Some(Weak::clone(&self.this)),
            );
            let mut waves = self.waves.borrow_mut();
            if let Some(wave) = waves.get_mut(index) {
                if cancelled {
                    wave.fade_in_cap = fade_in;
                }
                wave.exit = Some(WaveExit {
                    run,
                    delay_ms,
                    total_ms,
                });
            }
        }
        self.sync();
    }

    /// Stop every run.
    pub(crate) fn dispose(&self) {
        let waves = std::mem::take(&mut *self.waves.borrow_mut());
        drop(waves);
        self.layer.borrow_mut().dispose();
    }

    /// The state layer's colour now, or `None` when it is invisible.
    fn layer_paint(&self) -> Option<Color> {
        let alpha = self.layer.borrow().value();
        let color = self.layer_color.get()?;
        (alpha > 0.0).then(|| color.with_opacity(alpha))
    }

    /// Each visible wave's centre, radius and colour in a box of `size`.
    fn wave_paints(&self, size: Size) -> Vec<(Point<f64>, f64, Color)> {
        let states = self.states.borrow().value();
        let color = {
            let style = self.style.borrow();
            if !style.ripples() {
                return Vec::new();
            }
            style.wave_color(states)
        };
        let Some(color) = color else {
            return Vec::new();
        };
        let full = f64::from(color.opacity());
        self.waves
            .borrow()
            .iter()
            .filter_map(|wave| {
                let alpha = wave.alpha();
                if alpha <= 0.0 {
                    return None;
                }
                let (center, radius) = wave.circle(size)?;
                Some((center, radius, color.with_opacity(full * alpha)))
            })
            .collect()
    }
}

/// Paints an [`Indication`] under the well's child, clipped to its shape.
#[derive(Debug)]
pub(crate) struct IndicationPainter {
    pub(crate) indication: Rc<Indication>,
    pub(crate) shape: MaterialShape,
}

impl CustomPainter for IndicationPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !(size.width.is_finite() && size.height.is_finite())
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return;
        }
        let layer = self.indication.layer_paint();
        let waves = self.indication.wave_paints(size);
        if layer.is_none() && waves.is_empty() {
            return;
        }
        canvas.save();
        canvas.clip_rrect_ext(
            self.shape.to_rrect(size),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        if let Some(color) = layer {
            canvas.draw_rect(
                Rect::from_ltwh(0.0, 0.0, size.width, size.height),
                &Paint::fill(color),
            );
        }
        for (center, radius, color) in waves {
            canvas.draw_circle(center, radius, &Paint::fill(color));
        }
        canvas.restore();
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any().downcast_ref::<Self>().is_none_or(|old| {
            !Rc::ptr_eq(&old.indication, &self.indication) || old.shape != self.shape
        })
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::clone(&self.indication.repaint) as Rc<dyn Listenable>)
    }

    /// The feedback is decoration: hits belong to the child and the well.
    fn hit_test(&self, _position: flui_sdk::geometry::Offset) -> Option<bool> {
        Some(false)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
