//! [`ParticleEmitter`] and [`ParticleController`]: the widget that runs a particle system,
//! and the handle that bursts it.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use flui_animation::{Animation, AnimationController, DrivenController, MotionPolicy, Simulation};
use flui_foundation::geometry::{Point, Size};
use flui_foundation::{Listenable, ListenerId};
use flui_painting::Canvas;
use flui_painting::paint::BlendMode;
use flui_view::prelude::*;

use super::atlas::Atlas;
use super::config::ParticleConfig;
use super::system::ParticleSystem;
use crate::animated::VsyncScope;
use crate::{CustomPaint, CustomPainter, MediaQuery, TickerMode};

/// A widget that emits, moves and draws particles: confetti, sparkles, snow, or a
/// [`ParticleConfig`] of the app's own.
///
/// It paints over its [`child`](Self::child), or, without one, takes the size its
/// constraints give it; give it tight constraints (a `Positioned::fill`, a `SizedBox`) when it
/// stands alone. The source's fractions map onto that box.
///
/// Particles move on the frame clock in fixed steps, so the same configuration, seed and
/// frames give the same particles. The clock stops once nothing is alive and the rate has
/// finished; a burst starts it again. Under a disabled `TickerMode` nothing is emitted and
/// bursts are refused; when the ticker comes back, live particles catch up on a pause of up to
/// a quarter second and are removed after a longer one, and the rate carries on without making
/// up the time it missed. Under reduced motion nothing is emitted and live particles are
/// removed. Particles are decoration: they are left out of semantics and do not hit-test.
///
/// # Examples
///
/// ```rust,ignore
/// let confetti = ParticleController::new();
/// ParticleEmitter::new(ParticleConfig::confetti())
///     .controller(confetti.clone())
///     .child(page);
/// // On success:
/// confetti.emit_burst_at(tap_point, 120);
/// ```
#[derive(Clone, StatefulView)]
pub struct ParticleEmitter {
    config: ParticleConfig,
    controller: Option<ParticleController>,
    child: Option<BoxedView>,
}

impl fmt::Debug for ParticleEmitter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParticleEmitter")
            .field("config", &self.config)
            .field("has_controller", &self.controller.is_some())
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl ParticleEmitter {
    /// An emitter of `config`'s particles.
    #[must_use]
    pub fn new(config: ParticleConfig) -> Self {
        Self {
            config,
            controller: None,
            child: None,
        }
    }

    /// The controller that bursts this emitter. A controller drives one mounted emitter at a
    /// time: while another emitter holds it, this one runs without it.
    #[must_use]
    pub fn controller(mut self, controller: ParticleController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// The view the particles are drawn over, which also gives the emitter its size.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view().boxed());
        self
    }
}

/// Bursts the [`ParticleEmitter`] it is attached to.
///
/// Clones are the same controller. It attaches to the first emitter mounted with it and
/// stays with that one until the emitter unmounts or is given another controller; a second
/// emitter built with it meanwhile runs without it.
#[derive(Clone, Default)]
pub struct ParticleController {
    attached: Rc<RefCell<Option<Weak<Core>>>>,
}

impl fmt::Debug for ParticleController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParticleController")
            .field("attached", &self.is_attached())
            .finish()
    }
}

impl ParticleController {
    /// A controller attached to no emitter yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an emitter is mounted with this controller.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.core().is_some()
    }

    /// Emits up to `count` particles from the emitter's source at once, and returns how many
    /// it emitted.
    ///
    /// A burst larger than the free capacity is cut to it: live particles are never pushed
    /// out, and a full emitter emits 0. Without an attached emitter, under a disabled
    /// `TickerMode` and under reduced motion the burst is refused and 0 returned; a refused
    /// burst is not kept for later.
    pub fn emit_burst(&self, count: usize) -> usize {
        self.emit(None, count)
    }

    /// [`emit_burst`](Self::emit_burst) at `point`, in logical pixels of the emitter's box,
    /// instead of from the source. A point outside the box is allowed, so a burst can start
    /// at an edge, but a particle farther than its own size outside the box is removed; a
    /// point that is not finite refuses the burst.
    pub fn emit_burst_at(&self, point: Point<f64>, count: usize) -> usize {
        if !point.x.is_finite() || !point.y.is_finite() {
            return 0;
        }
        self.emit(Some(point), count)
    }

    fn emit(&self, at: Option<Point<f64>>, count: usize) -> usize {
        let Some(core) = self.core() else {
            tracing::debug!("particle burst without an attached emitter emits nothing");
            return 0;
        };
        core.burst(at, count)
    }

    fn core(&self) -> Option<Rc<Core>> {
        self.attached.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// Attaches `core` unless another live emitter holds this controller.
    fn attach(&self, core: &Rc<Core>) -> bool {
        let mut attached = self.attached.borrow_mut();
        match attached.as_ref().and_then(Weak::upgrade) {
            Some(holder) if !Rc::ptr_eq(&holder, core) => false,
            _ => {
                *attached = Some(Rc::downgrade(core));
                true
            }
        }
    }

    /// Lets go of `core`, if it is the one attached.
    fn detach(&self, core: &Rc<Core>) {
        let mut attached = self.attached.borrow_mut();
        if attached
            .as_ref()
            .is_some_and(|weak| std::ptr::eq(weak.as_ptr(), Rc::as_ptr(core)))
        {
            *attached = None;
        }
    }

    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.attached, &other.attached)
    }
}

/// What one mounted emitter shares with its controller, its clock's listener and its painter.
struct Core {
    system: RefCell<ParticleSystem>,
    atlas: Atlas,
    clock: AnimationController,
    /// The clock's value the system last advanced to.
    last: Cell<f64>,
    /// Set by the listener once nothing is alive and the rate has finished: the clock's run
    /// ends on its next tick.
    idle: Rc<Cell<bool>>,
    /// Whether particles may be emitted: the ticker is enabled and motion is not reduced.
    accepting: Cell<bool>,
    /// Whether the next tick ends a pause of the ticker, so the rate skips its time.
    resumed: Cell<bool>,
}

impl Core {
    /// Advances the system to the clock's value.
    fn tick(&self) {
        let now = self.clock.value();
        let dt = now - self.last.replace(now);
        let emit = !self.resumed.replace(false) && self.accepting.get();
        let Ok(mut system) = self.system.try_borrow_mut() else {
            return;
        };
        system.advance(dt, emit);
        self.idle.set(system.idle());
    }

    fn burst(&self, at: Option<Point<f64>>, count: usize) -> usize {
        if !self.accepting.get() {
            return 0;
        }
        let emitted = match self.system.try_borrow_mut() {
            Ok(mut system) => system.spawn(count, at),
            Err(_) => 0,
        };
        if emitted > 0 {
            self.idle.set(false);
            self.run();
        }
        emitted
    }

    /// Starts the clock if the emitter has something to move or emit and it is stopped.
    fn run(&self) {
        if self.clock.is_animating() || self.system.try_borrow().is_ok_and(|s| s.idle()) {
            return;
        }
        self.idle.set(false);
        self.last.set(0.0);
        if let Err(error) = self.clock.animate_with(Elapsed {
            idle: Rc::clone(&self.idle),
        }) {
            tracing::warn!(%error, "particle emitter could not start its clock");
        }
    }

    fn stop(&self) {
        if let Err(error) = self.clock.stop() {
            tracing::warn!(%error, "particle emitter could not stop its clock");
        }
    }
}

/// The emitter's clock: its value is the seconds it has run, until the emitter goes idle.
#[derive(Debug)]
struct Elapsed {
    idle: Rc<Cell<bool>>,
}

impl Simulation for Elapsed {
    fn x(&self, time: f64) -> f64 {
        time
    }

    fn dx(&self, _time: f64) -> f64 {
        1.0
    }

    fn is_done(&self, _time: f64) -> bool {
        self.idle.get()
    }
}

/// Persistent state for [`ParticleEmitter`].
pub struct ParticleEmitterState {
    core: Rc<Core>,
    clock: DrivenController,
    listener: Option<ListenerId>,
    controller: Option<ParticleController>,
    ticking: bool,
}

impl fmt::Debug for ParticleEmitterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParticleEmitterState")
            .field(
                "particles",
                &self.core.system.try_borrow().map(|s| s.len()).ok(),
            )
            .field("running", &self.core.clock.is_animating())
            .finish_non_exhaustive()
    }
}

impl StatefulView for ParticleEmitter {
    type State = ParticleEmitterState;

    fn create_state(&self) -> Self::State {
        let clock = AnimationController::builder(std::time::Duration::from_secs(1))
            .unbounded()
            .debug_label("ParticleEmitter.clock")
            .build_on(None);
        let atlas = Atlas::new(&self.config.sprite);
        let core = Rc::new(Core {
            system: RefCell::new(ParticleSystem::new(self.config.clone(), atlas.side)),
            atlas,
            clock: clock.controller().clone(),
            last: Cell::new(0.0),
            idle: Rc::new(Cell::new(false)),
            accepting: Cell::new(false),
            resumed: Cell::new(false),
        });
        let weak = Rc::downgrade(&core);
        let listener = clock.controller().add_listener(Rc::new(move || {
            if let Some(core) = weak.upgrade() {
                core.tick();
            }
        }));
        let mut state = ParticleEmitterState {
            core,
            clock,
            listener: Some(listener),
            controller: None,
            ticking: true,
        };
        state.adopt(self.controller.as_ref());
        state
    }
}

impl ParticleEmitterState {
    fn adopt(&mut self, controller: Option<&ParticleController>) {
        if let (Some(old), Some(new)) = (&self.controller, controller)
            && old.same(new)
        {
            return;
        }
        if let Some(old) = self.controller.take() {
            old.detach(&self.core);
        }
        if let Some(new) = controller {
            if new.attach(&self.core) {
                self.controller = Some(new.clone());
            } else {
                tracing::warn!(
                    "a ParticleController already drives a mounted emitter; this emitter runs without it"
                );
            }
        }
    }
}

impl ViewState<ParticleEmitter> for ParticleEmitterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.clock.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "particle emitter lost its frame registry");
        }
        let ticking = TickerMode::is_enabled(ctx);
        let reduced = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        if ticking && !self.ticking {
            self.core.resumed.set(true);
        }
        self.ticking = ticking;
        self.core.accepting.set(ticking && !reduced);
        if reduced {
            if let Ok(mut system) = self.core.system.try_borrow_mut() {
                system.clear();
            }
            self.core.stop();
        } else {
            self.core.run();
        }
    }

    fn did_update_view(&mut self, _old: &ParticleEmitter, new: &ParticleEmitter) {
        self.adopt(new.controller.as_ref());
    }

    fn build(&self, view: &ParticleEmitter, _ctx: &dyn BuildContext) -> impl IntoView {
        let painter = ParticlePainter {
            core: Rc::clone(&self.core),
        };
        let paint = CustomPaint::new().foreground_painter(painter);
        let paint = match &view.child {
            Some(child) => paint.child(child.clone()),
            None => paint,
        };
        // The painter publishes no semantics: particles are decoration, and the child keeps its
        // own.
        paint
    }

    fn dispose(&mut self) {
        if let Some(controller) = self.controller.take() {
            controller.detach(&self.core);
        }
        if let Some(listener) = self.listener.take() {
            self.core.clock.remove_listener(listener);
        }
        self.core.stop();
        let _ = self.clock.rebind(None);
        self.clock.dispose();
    }
}

/// Draws an emitter's particles in one atlas call.
struct ParticlePainter {
    core: Rc<Core>,
}

impl fmt::Debug for ParticlePainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParticlePainter").finish_non_exhaustive()
    }
}

impl CustomPainter for ParticlePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let Ok(mut system) = self.core.system.try_borrow_mut() else {
            return;
        };
        system.set_bounds(size);
        let placements = system.placements();
        drop(system);
        if placements.is_empty() {
            return;
        }
        let atlas = &self.core.atlas;
        let (transforms, colors) = placements
            .into_iter()
            .map(|placement| (placement.transform, placement.color))
            .unzip::<_, _, Vec<_>, Vec<_>>();
        canvas.draw_atlas(
            atlas.image.clone(),
            vec![atlas.sprite; transforms.len()],
            transforms,
            Some(colors),
            BlendMode::SrcOver,
            None,
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::new(self.core.clock.clone()) as Rc<dyn Listenable>)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
