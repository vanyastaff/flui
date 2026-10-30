//! [`SymbolEffect`] and [`PathEffect`]: motion vocabularies for icons, after SF Symbols'
//! effects.
//!
//! An effect moves the icon's painted box (scale, rotation, opacity) without changing its
//! layout, or, for a vector icon, draws its paths on or off. The icon keeps its label;
//! the motion is never announced.

use std::cell::RefCell;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animatable, Animation, AnimationController, ConstantAnimation, DrivenController, Iterations,
    Keyframes, Linear, MotionPolicy, RepeatMode, RepeatSpec, ReverseAnimation, curves,
};
use flui_foundation::Listenable;
use flui_objects::StackFit;
use flui_painting::Alignment;
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};

use super::{AnimatedIconData, IconData};
use crate::animated::VsyncScope;
use crate::{
    AnimatedBuilder, ExcludeSemantics, FadeTransition, MediaQuery, OverflowBox, Positioned,
    RotationTransition, ScaleTransition, Stack, TickerMode,
};

/// The longest run a slow [`Speed`] stretches an effect to.
const LONGEST_RUN: Duration = Duration::from_hours(24);

/// How fast an effect plays: a finite, positive multiplier.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Speed(f64);

impl Speed {
    /// The effect's own timing.
    pub const NORMAL: Self = Self(1.0);

    /// A multiplier of the effect's own timing, or `None` for zero, a negative value, `NaN`
    /// or an infinity.
    #[must_use]
    pub fn try_new(speed: f64) -> Option<Self> {
        (speed.is_finite() && speed > 0.0).then_some(Self(speed))
    }

    /// The multiplier.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }

    /// `base` played at this speed; a very slow speed stops at a day.
    fn scale(self, base: Duration) -> Duration {
        Duration::try_from_secs_f64(base.as_secs_f64() / self.0)
            .map_or(LONGEST_RUN, |run| run.min(LONGEST_RUN))
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Which motion an effect plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Bounce,
    Wiggle,
    Rotate,
    Pulse,
    Breathe,
    Appear,
    Disappear,
    Replace,
}

/// What plays the motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// Once each time the value changes.
    Change(u64),
    /// Over and over while true.
    Active(bool),
    /// Once when the effect is first applied.
    Applied,
    /// Each time the icon's data changes.
    Content,
}

impl Trigger {
    /// Whether two triggers are the same kind, whatever their values.
    fn same_kind(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// A motion applied to an icon, a font glyph or vector paths, after SF Symbols' effects.
///
/// Three kinds:
/// - **one-shot** ([`bounce`](Self::bounce), [`wiggle`](Self::wiggle),
///   [`rotate`](Self::rotate)), played once each time a value changes, as many times as
///   [`repeat`](Self::repeat) says;
/// - **repeating** ([`pulse`](Self::pulse), [`breathe`](Self::breathe), `rotate`), played
///   while active and stopped at the end of the cycle in progress;
/// - **transitions** ([`appear`](Self::appear), [`disappear`](Self::disappear)), played once
///   when the effect is first applied, on mount or when it takes another effect's place,
///   unless [`animate_on_appear`](Self::animate_on_appear) is off; and
///   [`replace`](Self::replace), a cross-fade each time the icon's data changes.
///
/// The motion moves the icon's painted box about its centre and never its layout. Without
/// a [`VsyncScope`] or under a muted [`TickerMode`] the icon is still, at the end state of a
/// transition. Under reduced motion one-shot and repeating effects leave the icon still,
/// and transitions land on their end state on the next frame. The icon's label is the only
/// thing assistive technology hears; after a replace only the new icon's.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::{Icon, IconData, SymbolEffect};
/// let heart = IconData::new(0xe87d);
/// let _ = Icon::new(heart.clone()).symbol_effect(SymbolEffect::bounce().on_change(3));
/// let _ = Icon::new(heart).symbol_effect(SymbolEffect::pulse().active(true));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SymbolEffect {
    motion: Motion,
    trigger: Trigger,
    repeat: NonZeroU32,
    speed: Speed,
    animate_on_appear: bool,
}

impl SymbolEffect {
    // Apple publishes no durations for SF Symbols' effects. The design table gives bounce,
    // pulse, replace and draw on; the rest sit beside them: a wiggle as long as a bounce, a
    // full turn in a second, a breath twice a pulse, and appearing as quick as a replace.

    /// One bounce: up, a small dip, back.
    pub const BOUNCE_DURATION: Duration = Duration::from_millis(500);
    /// One wiggle: a few small turns either way.
    pub const WIGGLE_DURATION: Duration = Duration::from_millis(500);
    /// One full turn.
    pub const ROTATE_DURATION: Duration = Duration::from_millis(1000);
    /// One pulse of the opacity.
    pub const PULSE_DURATION: Duration = Duration::from_millis(1000);
    /// One breath of the scale.
    pub const BREATHE_DURATION: Duration = Duration::from_millis(2000);
    /// Appearing or disappearing.
    pub const APPEAR_DURATION: Duration = Duration::from_millis(250);
    /// The cross-fade of a replaced icon.
    pub const REPLACE_DURATION: Duration = Duration::from_millis(250);

    fn of(motion: Motion, trigger: Trigger) -> Self {
        Self {
            motion,
            trigger,
            repeat: NonZeroU32::MIN,
            speed: Speed::NORMAL,
            animate_on_appear: true,
        }
    }

    /// The icon grows, dips and settles; see [`Self::BOUNCE_DURATION`].
    pub fn bounce() -> DiscreteEffect {
        DiscreteEffect(Motion::Bounce)
    }

    /// The icon turns a little either way a few times; see [`Self::WIGGLE_DURATION`].
    pub fn wiggle() -> DiscreteEffect {
        DiscreteEffect(Motion::Wiggle)
    }

    /// The icon makes a full turn, once per change or for as long as it is active; see
    /// [`Self::ROTATE_DURATION`].
    pub fn rotate() -> RotateEffect {
        RotateEffect(())
    }

    /// The icon's opacity dips and returns, over and over; see [`Self::PULSE_DURATION`].
    pub fn pulse() -> IndefiniteEffect {
        IndefiniteEffect(Motion::Pulse)
    }

    /// The icon grows a little and returns, over and over; see [`Self::BREATHE_DURATION`].
    pub fn breathe() -> IndefiniteEffect {
        IndefiniteEffect(Motion::Breathe)
    }

    /// The icon scales and fades in when the effect is first applied; see
    /// [`Self::APPEAR_DURATION`].
    #[must_use]
    pub fn appear() -> Self {
        Self::of(Motion::Appear, Trigger::Applied)
    }

    /// The icon scales and fades out when the effect is first applied, and stays hidden;
    /// see [`Self::APPEAR_DURATION`].
    #[must_use]
    pub fn disappear() -> Self {
        Self::of(Motion::Disappear, Trigger::Applied)
    }

    /// When the icon's data changes, the old icon fades out as the new one fades in; see
    /// [`Self::REPLACE_DURATION`]. Only the new icon is in the semantics tree.
    #[must_use]
    pub fn replace() -> Self {
        Self::of(Motion::Replace, Trigger::Content)
    }

    /// Plays a one-shot effect `count` times for each change; the default is once. A
    /// repeating effect runs until it is inactive and a transition plays once, so neither
    /// reads it.
    #[must_use]
    pub fn repeat(mut self, count: NonZeroU32) -> Self {
        self.repeat = count;
        self
    }

    /// Plays the effect at `speed`; the default is [`Speed::NORMAL`].
    #[must_use]
    pub fn speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    /// Whether a transition plays when it is first applied on mount; the default is `true`.
    /// Off, a newly mounted icon starts at the transition's end state. A transition that
    /// takes another effect's place still plays.
    #[must_use]
    pub fn animate_on_appear(mut self, animate: bool) -> Self {
        self.animate_on_appear = animate;
        self
    }

    fn base_duration(&self) -> Duration {
        match self.motion {
            Motion::Bounce => Self::BOUNCE_DURATION,
            Motion::Wiggle => Self::WIGGLE_DURATION,
            Motion::Rotate => Self::ROTATE_DURATION,
            Motion::Pulse => Self::PULSE_DURATION,
            Motion::Breathe => Self::BREATHE_DURATION,
            Motion::Appear | Motion::Disappear => Self::APPEAR_DURATION,
            Motion::Replace => Self::REPLACE_DURATION,
        }
    }

    /// Whether `other` is the same effect apart from its trigger's value and options.
    fn same_motion(&self, other: &Self) -> bool {
        self.motion == other.motion && self.trigger.same_kind(other.trigger)
    }
}

/// A one-shot effect waiting for the value that plays it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct DiscreteEffect(Motion);

impl DiscreteEffect {
    /// Plays once each time `value` differs from the last build's; the first build does
    /// not play.
    pub fn on_change(self, value: u64) -> SymbolEffect {
        SymbolEffect::of(self.0, Trigger::Change(value))
    }
}

/// A repeating effect waiting for the flag that runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct IndefiniteEffect(Motion);

impl IndefiniteEffect {
    /// Repeats while `active`; on turning inactive it finishes the cycle in progress.
    pub fn active(self, active: bool) -> SymbolEffect {
        SymbolEffect::of(self.0, Trigger::Active(active))
    }
}

/// A rotation, played once per change or for as long as it is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct RotateEffect(());

impl RotateEffect {
    /// One turn each time `value` differs from the last build's.
    pub fn on_change(self, value: u64) -> SymbolEffect {
        SymbolEffect::of(Motion::Rotate, Trigger::Change(value))
    }

    /// Turns while `active`; on turning inactive it finishes the turn in progress.
    pub fn active(self, active: bool) -> SymbolEffect {
        SymbolEffect::of(Motion::Rotate, Trigger::Active(active))
    }
}

/// Draws a vector icon's paths on or off by trimming them along their length.
///
/// Only [`AnimatedIcon`](super::AnimatedIcon) takes one: a font glyph has no paths to
/// trim, so [`Icon`](super::Icon) has no way to receive it. It plays once when first
/// applied, on mount or when it takes the other's place; under reduced motion, or without
/// a frame clock, the paths land drawn or undrawn at once.
///
/// A glyph icon refuses one at compile time:
///
/// ```compile_fail,E0599
/// # use flui_widgets::{Icon, IconData, PathEffect};
/// let _ = Icon::new(IconData::new(0xe87d)).path_effect(PathEffect::draw_on());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PathEffect {
    pub(super) on: bool,
    speed: Speed,
}

impl PathEffect {
    /// Drawing the paths on or off.
    pub const DRAW_DURATION: Duration = Duration::from_millis(500);

    /// The paths draw from nothing to whole; see [`Self::DRAW_DURATION`].
    #[must_use]
    pub fn draw_on() -> Self {
        Self {
            on: true,
            speed: Speed::NORMAL,
        }
    }

    /// The paths undraw from whole to nothing, and stay undrawn; see [`Self::DRAW_DURATION`].
    #[must_use]
    pub fn draw_off() -> Self {
        Self {
            on: false,
            speed: Speed::NORMAL,
        }
    }

    /// Plays the effect at `speed`; the default is [`Speed::NORMAL`].
    #[must_use]
    pub fn speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    pub(super) fn duration(&self) -> Duration {
        self.speed.scale(Self::DRAW_DURATION)
    }
}

// ============================================================================
// The motions
// ============================================================================

/// How one stop of a motion is reached.
#[derive(Clone, Copy)]
enum Ease {
    Out,
    InOut,
    Linear,
}

/// A value over one cycle as stops of (milliseconds of a one-second cycle, value, ease),
/// from `start`. Every motion's stops fill the cycle.
fn track(start: f64, stops: &[(u64, f64, Ease)]) -> Keyframes<f64> {
    let mut builder = Keyframes::builder(start, Duration::from_secs(1));
    for &(millis, value, ease) in stops {
        let over = Duration::from_millis(millis);
        builder = match ease {
            Ease::Out => builder.to(value, over, curves::EASE_OUT),
            Ease::InOut => builder.to(value, over, curves::EASE_IN_OUT),
            Ease::Linear => builder.to(value, over, Linear),
        };
    }
    builder
        .build()
        .expect("BUG: every motion's stops are finite and fill its one-second cycle")
}

/// The bounce's peak scale, a third of the way through.
const BOUNCE_PEAK: f64 = 1.2;
/// The bounce's dip below rest before it settles.
const BOUNCE_DIP: f64 = 0.95;
/// The wiggle's widest turn either way.
const WIGGLE_TURN: f64 = 0.03;
/// The pulse's lowest opacity, half way through.
const PULSE_LOW: f64 = 0.35;
/// The breath's largest scale, half way through.
const BREATHE_PEAK: f64 = 1.1;
/// The scale an appearing icon grows from and a disappearing one shrinks to.
const APPEAR_SCALE: f64 = 0.5;

/// The turns, scale and opacity a motion moves, each `None` where it stays at rest.
#[derive(Default)]
struct Tracks {
    turns: Option<Keyframes<f64>>,
    scale: Option<Keyframes<f64>>,
    opacity: Option<Keyframes<f64>>,
}

impl Tracks {
    /// `motion`'s tracks; a repeating rotation turns at an even pace so its cycles join.
    fn of(motion: Motion, repeating: bool) -> Self {
        match motion {
            Motion::Bounce => Self {
                scale: Some(track(
                    1.0,
                    &[
                        (300, BOUNCE_PEAK, Ease::Out),
                        (300, BOUNCE_DIP, Ease::InOut),
                        (400, 1.0, Ease::InOut),
                    ],
                )),
                ..Self::default()
            },
            Motion::Wiggle => Self {
                turns: Some(track(
                    0.0,
                    &[
                        (250, WIGGLE_TURN, Ease::InOut),
                        (250, -WIGGLE_TURN, Ease::InOut),
                        (250, WIGGLE_TURN / 2.0, Ease::InOut),
                        (250, 0.0, Ease::InOut),
                    ],
                )),
                ..Self::default()
            },
            Motion::Rotate => Self {
                turns: Some(track(
                    0.0,
                    &[(
                        1000,
                        1.0,
                        if repeating { Ease::Linear } else { Ease::InOut },
                    )],
                )),
                ..Self::default()
            },
            Motion::Pulse => Self {
                opacity: Some(track(
                    1.0,
                    &[(500, PULSE_LOW, Ease::InOut), (500, 1.0, Ease::InOut)],
                )),
                ..Self::default()
            },
            Motion::Breathe => Self {
                scale: Some(track(
                    1.0,
                    &[(500, BREATHE_PEAK, Ease::InOut), (500, 1.0, Ease::InOut)],
                )),
                ..Self::default()
            },
            Motion::Appear => Self {
                scale: Some(track(APPEAR_SCALE, &[(1000, 1.0, Ease::Out)])),
                opacity: Some(track(0.0, &[(1000, 1.0, Ease::Out)])),
                ..Self::default()
            },
            Motion::Disappear => Self {
                scale: Some(track(1.0, &[(1000, APPEAR_SCALE, Ease::InOut)])),
                opacity: Some(track(1.0, &[(1000, 0.0, Ease::InOut)])),
                ..Self::default()
            },
            Motion::Replace => Self {
                opacity: Some(track(0.0, &[(1000, 1.0, Ease::Linear)])),
                ..Self::default()
            },
        }
    }
}

/// `track` observed at `progress`.
fn observe(track: Keyframes<f64>, progress: &Rc<dyn Animation<f64>>) -> Rc<dyn Animation<f64>> {
    Rc::new(track.animate(Rc::clone(progress)))
}

// ============================================================================
// The host
// ============================================================================

/// What an icon is drawing, so a replace knows when it changed.
#[derive(Debug, Clone)]
pub(super) enum ContentKey {
    Glyph(Option<IconData>),
    Vector(AnimatedIconData),
}

impl ContentKey {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Glyph(a), Self::Glyph(b)) => a == b,
            (Self::Vector(a), Self::Vector(b)) => a.is_same(b),
            _ => false,
        }
    }
}

/// Plays a [`SymbolEffect`] on an icon's built content.
#[derive(Clone, StatefulView)]
pub(super) struct SymbolEffectHost {
    effect: SymbolEffect,
    key: ContentKey,
    content: BoxedView,
}

impl fmt::Debug for SymbolEffectHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolEffectHost")
            .field("effect", &self.effect)
            .finish_non_exhaustive()
    }
}

impl SymbolEffectHost {
    pub(super) fn new(effect: SymbolEffect, key: ContentKey, content: BoxedView) -> Self {
        Self {
            effect,
            key,
            content,
        }
    }
}

/// Persistent state for a [`SymbolEffectHost`]: its controller and what it last saw.
pub(super) struct SymbolEffectHostState {
    controller: DrivenController,
    effect: SymbolEffect,
    key: ContentKey,
    /// The content a replace is fading out.
    outgoing: Rc<RefCell<Option<BoxedView>>>,
    /// Set while the first application is waiting for a frame clock.
    pending_first: bool,
}

impl fmt::Debug for SymbolEffectHostState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolEffectHostState")
            .field("effect", &self.effect)
            .field("bound", &self.controller.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for SymbolEffectHost {
    type State = SymbolEffectHostState;

    fn create_state(&self) -> Self::State {
        SymbolEffectHostState {
            controller: AnimationController::builder(self.effect.base_duration())
                .debug_label("SymbolEffect")
                .build_on(None),
            effect: self.effect.clone(),
            key: self.key.clone(),
            outgoing: Rc::new(RefCell::new(None)),
            pending_first: true,
        }
    }
}

impl SymbolEffectHostState {
    fn controller(&self) -> &AnimationController {
        self.controller.controller()
    }

    /// Starts the effect as when it is first applied; `mounting` is true on mount.
    fn apply(&self, mounting: bool) {
        let effect = &self.effect;
        let controller = self.controller();
        let _ = controller.stop();
        controller.set_duration(effect.speed.scale(effect.base_duration()));
        match effect.trigger {
            Trigger::Change(_) | Trigger::Content => {
                let _ = controller.set_value(1.0);
            }
            Trigger::Active(active) => {
                let _ = controller.set_value(0.0);
                if active {
                    self.run_forever();
                }
            }
            Trigger::Applied => {
                if mounting && !effect.animate_on_appear {
                    let _ = controller.set_value(1.0);
                } else {
                    let _ = controller.set_value(0.0);
                    let _ = controller.forward();
                }
            }
        }
    }

    fn run_forever(&self) {
        let _ = self
            .controller()
            .repeat(RepeatSpec::new(RepeatMode::Restart));
    }

    /// Plays a one-shot effect its repeat count of times from rest.
    fn play_once(&self) {
        let controller = self.controller();
        let _ = controller.set_value(0.0);
        let _ = controller.repeat(
            RepeatSpec::new(RepeatMode::Restart)
                .with_iterations(Iterations::Finite(self.effect.repeat.get())),
        );
    }

    /// The replace's cross-fade: the new content fades in over the old, which is out of
    /// the semantics tree, and is drawn alone once the fade ends.
    fn cross_fade(&self, content: BoxedView, live: bool) -> BoxedView {
        let progress: Rc<dyn Animation<f64>> = if live {
            Rc::new(self.controller().clone())
        } else {
            Rc::new(ConstantAnimation::new(1.0))
        };
        let outgoing = Rc::clone(&self.outgoing);
        let fade_in = Tracks::of(Motion::Replace, false)
            .opacity
            .map(|track| observe(track, &progress))
            .expect("BUG: a replace has an opacity track");
        AnimatedBuilder::new(Rc::clone(&progress) as Rc<dyn Listenable>, move || {
            // The new content is heard from the first frame, while it is still unseen.
            let mut layers = vec![
                FadeTransition::new(Rc::clone(&fade_in), content.clone())
                    .always_include_semantics(true)
                    .boxed(),
            ];
            let fading = progress.value() < 1.0;
            if let Some(leaving) = outgoing.borrow().as_ref().filter(|_| fading) {
                let fade_out: Rc<dyn Animation<f64>> =
                    Rc::new(ReverseAnimation::new(Rc::clone(&fade_in)));
                // Positioned, so only the new content sizes the stack; unconstrained, so the
                // old content keeps its own size, centred over the new.
                layers.push(
                    Positioned::fill(
                        OverflowBox::new()
                            .with_min_width(0.0)
                            .with_max_width(f64::INFINITY)
                            .with_min_height(0.0)
                            .with_max_height(f64::INFINITY)
                            .child(
                                ExcludeSemantics::new()
                                    .child(FadeTransition::new(fade_out, leaving.clone())),
                            ),
                    )
                    .boxed(),
                );
            }
            Stack::new(layers)
                .alignment(Alignment::CENTER)
                .fit(StackFit::Passthrough)
        })
        .boxed()
    }
}

impl ViewState<SymbolEffectHost> for SymbolEffectHostState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "a symbol effect lost its frame registry");
        }
        if std::mem::take(&mut self.pending_first) {
            self.apply(true);
        }
    }

    fn did_update_view(&mut self, old: &SymbolEffectHost, view: &SymbolEffectHost) {
        let previous = std::mem::replace(&mut self.effect, view.effect.clone());
        let previous_key = std::mem::replace(&mut self.key, view.key.clone());
        if !previous.same_motion(&self.effect) {
            self.outgoing.borrow_mut().take();
            self.apply(false);
            return;
        }
        self.controller()
            .set_duration(self.effect.speed.scale(self.effect.base_duration()));
        match (previous.trigger, self.effect.trigger) {
            (Trigger::Change(was), Trigger::Change(now)) if was != now => self.play_once(),
            (Trigger::Active(false), Trigger::Active(true)) => self.run_forever(),
            (Trigger::Active(true), Trigger::Active(false)) => {
                // The cycle in progress runs to its end, which is at rest.
                let _ = self.controller().forward();
            }
            (Trigger::Content, Trigger::Content) if !previous_key.same(&self.key) => {
                *self.outgoing.borrow_mut() = Some(old.content.clone());
                let controller = self.controller();
                let _ = controller.set_value(0.0);
                let _ = controller.forward();
            }
            _ => {}
        }
    }

    fn build(&self, view: &SymbolEffectHost, ctx: &dyn BuildContext) -> impl IntoView {
        let motion = self.effect.motion;
        let live = self.controller.is_bound() && TickerMode::is_enabled(ctx);
        let reduced = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        let content = view.content.clone();
        if motion == Motion::Replace {
            return self.cross_fade(content, live);
        }
        let transition = matches!(self.effect.trigger, Trigger::Applied);
        if !live || (reduced && !transition) {
            // Still: at rest, or at a transition's end.
            return if motion == Motion::Disappear {
                FadeTransition::new(Rc::new(ConstantAnimation::new(0.0)), content).boxed()
            } else {
                content
            };
        }
        let progress: Rc<dyn Animation<f64>> = Rc::new(self.controller().clone());
        let tracks = Tracks::of(motion, matches!(self.effect.trigger, Trigger::Active(_)));
        let mut built = content;
        if let Some(turns) = tracks.turns {
            built = RotationTransition::new(observe(turns, &progress), built).boxed();
        }
        if let Some(scale) = tracks.scale {
            built = ScaleTransition::new(observe(scale, &progress), built).boxed();
        }
        if let Some(opacity) = tracks.opacity {
            built = FadeTransition::new(observe(opacity, &progress), built).boxed();
        }
        built
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}
