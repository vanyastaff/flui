//! Shared machinery for the implicitly-animated widget family — the FLUI port
//! of Flutter's `ImplicitlyAnimatedWidgetState` / `AnimatedWidgetBaseState`.
//!
//! Each implicitly-animated widget (`AnimatedOpacity`, `AnimatedPadding`, …) is
//! a [`StatefulView`](flui_view::StatefulView) whose state owns one
//! [`ImplicitController`] driving one or more tweens. The state's `build`
//! returns an [`AnimatedBuilder`](crate::AnimatedBuilder) over the controller,
//! so only that inner builder rebuilds per frame; the implicit widget itself
//! rebuilds solely when its parent hands it a new configuration, at which point
//! `did_update_view` retargets.
//!
//! - [`ImplicitController`] — the persistent controller + curve + vsync
//!   registration, with no notion of *what* is animated.
//! - [`ImplicitAnimation`] — `ImplicitController` plus one [`Tween<T>`] (the
//!   single-property widgets: opacity, padding, alignment).
//! - [`OptTween`] — one optional property of a multi-property widget
//!   (`AnimatedContainer`), animated only while both old and new values are set.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::curve::ArcCurve;
use flui_animation::{
    Animatable, Animation, AnimationController, AnimationStatus, CurvedAnimation, Scheduler, Tween,
    Vsync, VsyncRegistration,
};
use flui_foundation::Listenable;
use flui_types::geometry::Lerp;

/// The default implicit-animation duration when a widget does not override it.
///
/// 200 ms matches Flutter's common default for implicit transitions — long
/// enough to read as motion, short enough to feel responsive.
pub(crate) const DEFAULT_DURATION: Duration = Duration::from_millis(200);

/// The persistent 0→1 driver behind an implicitly-animated widget: an
/// [`AnimationController`], the curve applied to it, and its `VsyncScope`
/// registration. Holds no tween — `value()` is the curved progress its owner
/// feeds to one or more tweens.
pub(crate) struct ImplicitController {
    controller: AnimationController,
    curved: CurvedAnimation<ArcCurve>,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
}

impl ImplicitController {
    /// A controller at rest (value `0`, `Dismissed`) with `curve` applied.
    pub(crate) fn new(duration: Duration, curve: ArcCurve) -> Self {
        // A fresh scheduler: on a real display its ticker would drive the
        // controller off wall-clock time; under a `VsyncScope` the binding drives
        // it deterministically via `tick_at` instead (this scheduler is never
        // pumped), so the two paths never double-advance the controller.
        let controller = AnimationController::new(duration, Arc::new(Scheduler::new()));
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        let curved = CurvedAnimation::new(parent, curve);
        Self {
            controller,
            curved,
            vsync: None,
            vsync_registration: None,
        }
    }

    /// Register with `vsync` so a binding drives this controller each frame.
    /// Called exactly once, from the owning state's `init_state`.
    pub(crate) fn register(&mut self, vsync: Vsync) {
        let registration = vsync.register(self.controller.clone());
        self.vsync = Some(vsync);
        self.vsync_registration = Some(registration);
    }

    /// The curved progress (`0`→`1`, possibly overshooting) the tweens map.
    pub(crate) fn value(&self) -> f32 {
        self.curved.value()
    }

    /// The listenable an [`AnimatedBuilder`](crate::AnimatedBuilder) subscribes
    /// to: the curved animation, which re-emits the controller's ticks. Its
    /// underlying notifier is stable across the clones each rebuild mints.
    pub(crate) fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::new(self.curved.clone())
    }

    /// A clone of the curved animation for capture in a build closure.
    pub(crate) fn curved(&self) -> CurvedAnimation<ArcCurve> {
        self.curved.clone()
    }

    /// Restart the run from `0` over `duration` — called after the owner's
    /// tween(s) were re-anchored, so the curved progress sweeps `0`→`1` afresh.
    pub(crate) fn restart(&mut self, duration: Duration) {
        self.controller.set_duration(duration);
        // Owned, freshly registered controller: `forward_from` only errors when
        // disposed, which cannot happen before `dispose`.
        let _ = self.controller.forward_from(Some(0.0));
    }

    /// The controller's run status (for diagnostics).
    pub(crate) fn status(&self) -> AnimationStatus {
        self.controller.status()
    }

    /// Unregister from the binding and dispose the controller.
    pub(crate) fn dispose(&mut self) {
        if let (Some(vsync), Some(registration)) = (&self.vsync, self.vsync_registration) {
            vsync.unregister(registration);
        }
        self.controller.dispose();
    }
}

impl std::fmt::Debug for ImplicitController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImplicitController")
            .field("status", &self.status())
            .field("registered", &self.vsync_registration.is_some())
            .finish_non_exhaustive()
    }
}

/// One animated property: an [`ImplicitController`] plus a [`Tween<T>`] whose
/// `begin`/`end` are re-anchored on every retarget. The single-property
/// implicitly-animated widgets (`AnimatedOpacity`, `AnimatedPadding`,
/// `AnimatedAlign`) hold exactly one of these.
///
/// `T` must be [`Lerp`] so the tween can interpolate it and [`PartialEq`] so a
/// retarget can detect "the target actually changed".
#[derive(Debug)]
pub(crate) struct ImplicitAnimation<T: Lerp + Clone + PartialEq + Send + Sync + 'static> {
    controller: ImplicitController,
    /// `begin` = the value shown when the current run started; `end` = the
    /// target. At rest both equal the target, so the widget sits at its target
    /// with no motion until a configuration change retargets it.
    tween: Tween<T>,
}

impl<T: Lerp + Clone + PartialEq + Send + Sync + 'static> ImplicitAnimation<T> {
    /// Build an animation sitting at `target` (no motion yet).
    pub(crate) fn new(target: T, duration: Duration, curve: ArcCurve) -> Self {
        Self {
            controller: ImplicitController::new(duration, curve),
            tween: Tween::new(target.clone(), target),
        }
    }

    /// Register with `vsync` so a binding drives this controller each frame.
    pub(crate) fn register(&mut self, vsync: Vsync) {
        self.controller.register(vsync);
    }

    /// The current displayed value — the tween evaluated at the curved progress.
    pub(crate) fn current_value(&self) -> T {
        self.tween.transform(self.controller.value())
    }

    /// The listenable an `AnimatedBuilder` subscribes to.
    pub(crate) fn listenable(&self) -> Arc<dyn Listenable> {
        self.controller.listenable()
    }

    /// A clone of the curved animation for capture in a build closure.
    pub(crate) fn curved(&self) -> CurvedAnimation<ArcCurve> {
        self.controller.curved()
    }

    /// A clone of the current tween for capture in a build closure.
    pub(crate) fn tween(&self) -> Tween<T> {
        self.tween.clone()
    }

    /// Retarget to `new_target` over `duration`: if the target changed, set
    /// `begin` to the currently displayed value and `end` to the new target,
    /// then restart the controller from `0`. A no-op when the target is
    /// unchanged, so an unrelated rebuild does not restart the animation.
    ///
    /// Flutter's `_constructTweens` + `forEachTween` + `forward(from: 0)`,
    /// collapsed for one property.
    pub(crate) fn retarget(&mut self, new_target: T, duration: Duration) {
        if self.tween.end == new_target {
            return;
        }
        let from = self.current_value();
        self.tween = Tween::new(from, new_target);
        self.controller.restart(duration);
    }

    /// Unregister from the binding and dispose the controller.
    pub(crate) fn dispose(&mut self) {
        self.controller.dispose();
    }
}

/// One optional property of a multi-property implicitly-animated widget
/// (`AnimatedContainer`). The tween exists only while the property is set; the
/// property animates only across a Some→Some change, and snaps on a Some↔None
/// transition (a value appearing or disappearing has no "from"/"to" to lerp,
/// matching the pragmatic edge of Flutter's nullable geometry tweens).
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
    pub(crate) fn current(&self, t: f32) -> Option<T> {
        self.tween.as_ref().map(|tween| tween.transform(t))
    }

    /// Re-anchor toward `new_target`, evaluating the current value at `t` for a
    /// Some→Some change. Returns `true` when a continuous (animatable) change
    /// occurred — the owner restarts the shared controller if any property does.
    pub(crate) fn retarget(&mut self, new_target: Option<T>, t: f32) -> bool {
        match (new_target, self.tween.as_ref()) {
            (Some(target), Some(existing)) if existing.end != target => {
                let from = existing.transform(t);
                self.tween = Some(Tween::new(from, target));
                true
            }
            (Some(_), Some(_)) => false, // unchanged target
            (Some(target), None) => {
                // Appearing: snap in (no "from" to animate from).
                self.tween = Some(Tween::new(target.clone(), target));
                false
            }
            (None, _) => {
                // Disappearing: drop the tween, snap out.
                self.tween = None;
                false
            }
        }
    }
}
