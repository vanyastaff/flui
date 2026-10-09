//! Shared machinery for the implicitly-animated widget family.
//!
//! [`ImplicitController`] drives the normalized progress used by the container,
//! and alignment tween paths. Opacity, padding and rotation consume owning
//! component motion directly. Their states reconfigure motion when a parent
//! supplies new values; frame samples update the observed render or build path.
//!
//! - [`ImplicitController`] — the persistent controller + curve + vsync
//!   registration, with no notion of *what* is animated.
//! - [`OptTween`] — one optional property of a multi-property widget
//!   (`AnimatedContainer`), animated only while both old and new values are set.

use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use flui_animation::curve::ArcCurve;
use flui_animation::{
    Animatable, Animation, AnimationController, AnimationStatus, CurvedAnimation, Curves,
    DrivenController, Tween, Vsync,
};
use flui_foundation::Listenable;
use flui_foundation::geometry::Lerp;

/// The default implicit-animation duration when a widget does not override it.
///
/// 200 ms is long
/// enough to read as motion, short enough to feel responsive.
pub(crate) const DEFAULT_DURATION: Duration = Duration::from_millis(200);

/// The default implicit-animation curve (`Curves::EaseInOut`), cached behind
/// one process-wide handle so every widget built *without* an explicit
/// `.curve(...)` override compares curve-**unchanged**
/// (see [`ArcCurve`]'s `PartialEq`) across an unrelated rebuild.
///
/// Without this cache, `ArcCurve::new(Curves::EaseInOut)` would heap-allocate
/// a fresh, distinct handle every time a widget default-constructs its curve
/// — which, under `ArcCurve`'s reference-equality comparison, would make
/// every single reconfigure look like a curve change, defeating
/// [`ImplicitController::set_curve`]'s no-op gate. This is the Rust-native
/// equivalent of canonicalizing a `const` curve — see `ArcCurve`'s doc.
pub(crate) fn default_curve() -> ArcCurve {
    static DEFAULT: OnceLock<ArcCurve> = OnceLock::new();
    DEFAULT
        .get_or_init(|| ArcCurve::new(Curves::EaseInOut))
        .clone()
}

/// The persistent 0→1 driver behind an implicitly-animated widget: an
/// [`AnimationController`], the curve applied to it, and its `VsyncScope`
/// registration. Holds no tween — `value()` is the curved progress its owner
/// feeds to one or more tweens.
pub(crate) struct ImplicitController {
    controller: DrivenController,
    /// The curve currently baked into `curved`, kept alongside it so
    /// [`set_curve`](Self::set_curve) can detect a no-op reconfigure
    /// (`ArcCurve`'s `PartialEq` is reference equality — see its doc) without
    /// re-deriving it from the `CurvedAnimation`, which exposes no getter.
    curve: ArcCurve,
    curved: CurvedAnimation<ArcCurve>,
}

impl ImplicitController {
    /// A controller at rest (value `0`, `Dismissed`) with `curve` applied.
    pub(crate) fn new(duration: Duration, curve: ArcCurve) -> Self {
        // No ticker: `VsyncScope` drives this controller deterministically
        // via `tick_at` instead.
        let controller = AnimationController::builder(duration).build_on(None);
        let parent: std::rc::Rc<dyn Animation<f64>> =
            std::rc::Rc::new(controller.controller().clone());
        let curved = CurvedAnimation::new(parent, curve.clone());
        Self {
            controller,
            curve,
            curved,
        }
    }

    /// Register with `vsync` so a binding drives this controller each frame.
    /// Called exactly once, from the owning state's `init_state`.
    pub(crate) fn rebind(&mut self, vsync: Option<&Vsync>) {
        if let Err(error) = self.controller.rebind(vsync) {
            tracing::error!(%error, "implicit animation has no clock");
        }
    }

    /// The curved progress (`0`→`1`, possibly overshooting) the tweens map.
    pub(crate) fn value(&self) -> f64 {
        self.curved.value()
    }

    /// The listenable an [`AnimatedBuilder`](crate::AnimatedBuilder) subscribes
    /// to: the curved animation, which re-emits the controller's ticks. Its
    /// underlying notifier is stable across the clones each rebuild mints.
    pub(crate) fn listenable(&self) -> std::rc::Rc<dyn Listenable> {
        Rc::new(self.curved.clone())
    }

    /// A clone of the curved animation for capture in a build closure.
    pub(crate) fn curved(&self) -> CurvedAnimation<ArcCurve> {
        self.curved.clone()
    }

    /// Update the controller's base forward duration.
    ///
    /// This is unconditional on every reconfigure, independent of whether a
    /// target or curve actually changed.
    /// Never retimes a run already in flight — see
    /// `AnimationController::set_duration`'s own doc.
    pub(crate) fn set_duration(&mut self, duration: Duration) {
        self.controller.controller().set_duration(duration);
    }

    /// Swap in `curve`, rebuilding the `CurvedAnimation` over the SAME
    /// controller — the run in flight, if any, is untouched; only the easing
    /// function applied to its current position changes. A no-op (`curve`
    /// reference-equal to the one already installed) skips the rebuild
    /// entirely, so an unrelated widget reconfigure does not drop and re-add
    /// the controller's value subscription. Returns whether the curve
    /// actually changed.
    ///
    /// A curve-only change disposes the old `CurvedAnimation` and builds a
    /// fresh one over the same `controller`. The controller is never
    /// restarted for a curve-only change; see
    /// [`restart_from_zero`](Self::restart_from_zero)'s doc for what is.
    pub(crate) fn set_curve(&mut self, curve: ArcCurve) -> bool {
        if self.curve == curve {
            return false;
        }
        let parent: std::rc::Rc<dyn Animation<f64>> =
            std::rc::Rc::new(self.controller.controller().clone());
        self.curved = CurvedAnimation::new(parent, curve.clone());
        self.curve = curve;
        true
    }

    /// Restart the run from `0` over the currently-set duration — called
    /// after the owner's tween(s) were re-anchored (a genuine target
    /// change), so the curved progress sweeps `0`→`1` afresh.
    ///
    /// Gated on a genuine target change — a curve-only change never reaches
    /// this restart.
    pub(crate) fn restart_from_zero(&mut self) {
        // Owned, freshly registered controller: `forward_from` only errors when
        // disposed, which cannot happen before `dispose`.
        let _ = self.controller.controller().forward_from(Some(0.0));
    }

    /// The controller's run status (for diagnostics).
    pub(crate) fn status(&self) -> AnimationStatus {
        self.controller.controller().status()
    }

    /// Unregister from the binding and dispose the controller.
    pub(crate) fn dispose(&mut self) {
        self.controller.dispose();
    }
}

impl std::fmt::Debug for ImplicitController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImplicitController")
            .field("status", &self.status())
            .field("registered", &self.controller.is_bound())
            .finish_non_exhaustive()
    }
}

/// One optional property of a multi-property implicitly-animated widget
/// (`AnimatedContainer`). The tween exists only while the property is set; the
/// property animates only across a Some→Some change, and snaps on a Some↔None
/// transition (a value appearing or disappearing has no "from"/"to" to lerp,
/// the pragmatic edge for optional geometry tweens).
#[derive(Debug, Clone)]
pub(crate) struct OptTween<T: Lerp + Clone + PartialEq> {
    tween: Option<Tween<T>>,
}

impl<T: Lerp + Clone + PartialEq> OptTween<T> {
    /// At-rest tween for an initial `target` (both endpoints the target, or no
    /// tween when the property is unset).
    pub(crate) fn at_rest(target: Option<T>) -> Self {
        Self {
            tween: target.map(|value| Tween::new(value.clone(), value)),
        }
    }

    /// The current value at curved progress `t`, or `None` when unset.
    pub(crate) fn current(&self, t: f64) -> Option<T> {
        self.tween.as_ref().map(|tween| tween.transform(t))
    }

    /// Whether moving to `new_target` is a continuous (animatable) Some→Some change;
    /// the owner restarts the shared controller if any property's is.
    pub(crate) fn animates_toward(&self, new_target: Option<&T>) -> bool {
        matches!((new_target, &self.tween), (Some(target), Some(existing)) if existing.end != *target)
    }

    /// Move toward `new_target`. When the owner supplies a restart sample for the shared
    /// controller, every Some→Some tween — changed or not — re-anchors at its value
    /// for that progress, so an unchanged property continues from where
    /// it is instead of replaying its old run from the start.
    pub(crate) fn retarget(&mut self, new_target: Option<T>, restart_at: Option<f64>) {
        match (new_target, self.tween.as_ref()) {
            (Some(target), Some(existing)) => {
                if let Some(t) = restart_at {
                    let from = existing.transform(t);
                    self.tween = Some(Tween::new(from, target));
                }
            }
            (Some(target), None) => {
                // Appearing: snap in (no "from" to animate from).
                self.tween = Some(Tween::new(target.clone(), target));
            }
            (None, _) => {
                // Disappearing: drop the tween, snap out.
                self.tween = None;
            }
        }
    }
}
