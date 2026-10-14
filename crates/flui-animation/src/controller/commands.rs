//! Run commands: every public way to start, retarget or halt a run.

use std::cell::RefMut;
use std::rc::Rc;
use std::time::Duration;

use super::run::{RepeatRun, RunMode, Settle, SimulationRun};
use super::{
    AnimationController, AnimationControllerInner, FLING_TOLERANCE, Opaque, RetiredSources,
    ValueChange, default_fling_spring,
};
use crate::AnimationRunFuture;
use crate::animation::AnimationDirection;
use crate::curve::Curve;
use crate::error::{AnimationError, AnimationInput, RangeDefect};
use crate::repeat::{Iterations, RepeatMode, RepeatSpec};
use crate::run_future::RunCompleter;
use crate::simulation::{Simulation, SpringDescription, SpringSimulation, SpringType, Tolerance};
use crate::status::AnimationStatus;

impl AnimationController {
    /// Set the duration for reverse animation.
    pub fn set_reverse_duration(&self, duration: Duration) {
        let mut inner = self.inner.borrow_mut();
        inner.reverse_duration = Some(duration);
    }

    /// Set the base forward duration.
    ///
    /// An implicitly animated widget assigns this on every update so a
    /// duration change takes effect on the *next* run. It does not retime an in-flight run (the active run keeps
    /// the duration it started with, since `tick_at` scales against the
    /// already-captured run duration); the new value is read when
    /// the next `forward`/`reverse` begins the run at elapsed zero.
    pub fn set_duration(&self, duration: Duration) {
        let mut inner = self.inner.borrow_mut();
        inner.duration = duration;
    }

    /// Start animation forward from current value to upper bound.
    ///
    /// The returned [`AnimationRunFuture`] resolves `Ok(())` when this run finishes
    /// and `Err(RunCanceled)` if it is superseded (a later run starts
    /// before this one ends) or torn down ([`stop`](Self::stop)/
    /// [`set_value`](Self::set_value)/[`reset`](Self::reset)/
    /// [owning disposal](crate::DrivenController::dispose)). See
    /// [`AnimationRunFuture::when_complete_or_cancel`] for the idiom to react to
    /// either outcome without matching on it, and this method's own
    /// `# Awaiting a run` section below for the `async`/`await` route.
    ///
    /// # Awaiting a run
    ///
    /// ```rust
    /// use flui_animation::AnimationController;
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use std::task::{Context, Poll, Waker};
    /// use std::time::Duration;
    ///
    /// let controller = AnimationController::builder(Duration::from_millis(100)).build();
    /// let mut run = controller.forward().unwrap();
    ///
    /// // A caller that only cares "how did it end", not "did it end yet",
    /// // reacts to either outcome the same way:
    /// run.when_complete_or_cancel(|end| {
    ///     println!("run ended: {end:?}");
    /// });
    ///
    /// // Or await it directly. `tick_at` is normally driven by a real
    /// // Vsync; this drives it to completion by hand for the example.
    /// controller.tick_at(Duration::from_millis(100));
    /// let waker = Waker::noop();
    /// let mut cx = Context::from_waker(waker);
    /// assert_eq!(Pin::new(&mut run).poll(&mut cx), Poll::Ready(Ok(())));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward(&self) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::forward",
            self.drive_to_bound(AnimationDirection::Forward, None),
        )
    }

    /// Start animation forward from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the forward duration scaled by
    /// `(upper_bound - value) / (upper_bound - lower_bound)`. A zero DISTANCE
    /// (starting at the upper bound) or a zero DURATION (the scaled run
    /// duration is `Duration::ZERO`) settles SYNCHRONOUSLY, before this
    /// call returns, with [`AnimationStatus::ForwardSettled`] and an
    /// already-complete [`AnimationRunFuture`]. See
    /// [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] if `from` is `NaN`.
    /// Returns [`AnimationError::UnboundedTarget`] if `from` is infinite
    /// toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](crate::AnimationControllerBuilder::unbounded) (`forward`
    /// always targets `upper_bound`, which has no finite value to run to).
    pub fn forward_from(&self, from: Option<f64>) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::forward_from",
            self.drive_to_bound(AnimationDirection::Forward, from),
        )
    }

    /// Start animation in reverse from current value to lower bound.
    ///
    /// See [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse(&self) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::reverse",
            self.drive_to_bound(AnimationDirection::Reverse, None),
        )
    }

    /// Start animation in reverse from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the reverse duration (falling back to the forward
    /// duration) scaled by `(value - lower_bound) / (upper_bound -
    /// lower_bound)`. A zero DISTANCE (starting at the lower bound) or a zero DURATION
    /// settles SYNCHRONOUSLY, before this call returns, with
    /// [`AnimationStatus::ReverseSettled`] — see [`forward_from`](Self::forward_from).
    /// See [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] if `from` is `NaN`.
    /// Returns [`AnimationError::UnboundedTarget`] if `from` is infinite
    /// toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](crate::AnimationControllerBuilder::unbounded) (`reverse`
    /// always targets `lower_bound`, which has no finite value to run to).
    pub fn reverse_from(&self, from: Option<f64>) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::reverse_from",
            self.drive_to_bound(AnimationDirection::Reverse, from),
        )
    }

    /// Run from `from` (or the current value) to the bound `direction`
    /// faces, over that direction's duration scaled by the remaining
    /// distance. Shared by [`forward_from`](Self::forward_from) and
    /// [`reverse_from`](Self::reverse_from).
    fn drive_to_bound(
        &self,
        direction: AnimationDirection,
        from: Option<f64>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;

        let (bound, base) = match direction {
            AnimationDirection::Forward => (inner.upper_bound, inner.duration),
            AnimationDirection::Reverse => (
                inner.lower_bound,
                inner.reverse_duration.unwrap_or(inner.duration),
            ),
        };
        // The run always targets the bound: refuse an unbounded side before
        // touching `from`, so `forward()` and `reverse()` (no `from` at all)
        // are refused on an unbounded controller too.
        if !bound.is_finite() {
            return Err(AnimationError::UnboundedTarget {
                input: AnimationInput::Target,
                value: bound,
            });
        }
        let entry_value = inner.value;
        if let Some(raw) = from {
            inner.value = Self::canonicalize_value_target(
                AnimationInput::From,
                raw,
                inner.lower_bound,
                inner.upper_bound,
            )?;
        }

        inner.direction = direction;
        inner.start_value = inner.value;
        inner.target_value = bound;
        let run_duration = inner.scaled_run_duration(base);
        if inner.value == inner.target_value || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, None, retired, inner));
        }

        let status = direction.running_status();
        inner.status = status;
        // `from` may have jumped `value` above without a settle (a real run
        // still starts). Notification fires "iff it actually moved" (the
        // same entry-value rule `settle_at_target` uses), so
        // `forward_from(Some(x))` from `x` itself does not fire a spurious
        // notification for a value that never changed.
        let value_change = if inner.value == entry_value {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        // Queue the restart; finish commits the run before invoking the wake hook.
        let (future, displaced_delivery) = Self::install_run(
            &mut inner,
            RunMode::Timed {
                curve: None,
                duration: run_duration,
            },
            Settle::Sampled,
            &mut retired,
        );
        self.deliver_transition(status, value_change, displaced_delivery, retired, inner);
        Ok(future)
    }

    /// Stop the animation at its current value.
    ///
    /// The status transitions to a **non-running** settled status:
    /// - [`AnimationStatus::ForwardSettled`] at (or above) the upper bound, or when
    ///   the active run direction was forward
    /// - [`AnimationStatus::ReverseSettled`] at (or below) the lower bound, or when
    ///   the active run direction was reverse
    ///
    /// This phase does not prove endpoint arrival or successful completion.
    /// Frame drivers query [`Animation::is_animating`](crate::Animation::is_animating) for run ownership.
    ///
    /// Cancels the active run's [`AnimationRunFuture`] with
    /// [`RunCanceled`](crate::RunCanceled),
    /// delivered with no controller borrow held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        if let Err(disposed) = Self::check_disposed(&inner) {
            drop(inner);
            return self.reported_teardown("AnimationController::stop", Err(disposed));
        }

        let delivery = inner.stop_running(&mut retired);

        let status = inner.settled_status_directed();
        inner.status = status;
        self.deliver_transition(status, ValueChange::Unchanged, delivery, retired, inner);
        Ok(())
    }

    /// Reset to the beginning (lower bound).
    ///
    /// Sets the value to the beginning — `lower_bound` on a bounded
    /// controller, `0.0` on an [`unbounded`](crate::AnimationControllerBuilder::unbounded) one (there is
    /// no `lower_bound` to return to) — and the status to
    /// [`AnimationStatus::ReverseSettled`]. Never fails for non-finiteness: a
    /// reset always has a value to land on. Cancels the active run's
    /// [`AnimationRunFuture`] with
    /// [`RunCanceled`](crate::RunCanceled), delivered
    /// with no controller borrow held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        if let Err(disposed) = Self::check_disposed(&inner) {
            drop(inner);
            return self.reported_teardown("AnimationController::reset", Err(disposed));
        }

        let delivery = inner.stop_running(&mut retired);
        inner.value = if inner.is_unbounded() {
            0.0
        } else {
            inner.lower_bound
        };
        inner.status = AnimationStatus::ReverseSettled;
        self.deliver_transition(
            AnimationStatus::ReverseSettled,
            ValueChange::Notify,
            delivery,
            retired,
            inner,
        );
        Ok(())
    }

    /// Animate to a specific value over `duration` (or the controller's forward
    /// duration when `None`, scaled by the remaining fraction of the range —
    /// see [`forward_from`](Self::forward_from)).
    ///
    /// The per-run `duration` override applies to **this run only** and does not
    /// modify the controller's base duration. An explicit `duration` is used
    /// as-is, without remaining-fraction scaling. See [`forward`](Self::forward)
    /// for the returned future's contract and the `when_complete_or_cancel`
    /// idiom.
    ///
    /// [`status`](crate::Animation::status) is reported as
    /// [`AnimationStatus::Forward`] **regardless of whether `target` is above
    /// or below the current value**, ending [`AnimationStatus::ForwardSettled`].
    /// If `target` is
    /// already the current value (or `duration` resolves to
    /// `Duration::ZERO`), this settles synchronously; see
    /// [`forward_from`](Self::forward_from)'s doc.
    ///
    /// # Arguments
    ///
    /// * `target` - The target value (clamped to bounds)
    /// * `duration` - Optional per-run duration override
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] if `target` is `NaN`,
    /// [`AnimationError::UnboundedTarget`] if an infinite target has no finite
    /// bound to clamp to, or [`AnimationError::SpanOverflow`] when
    /// `target - value` overflows `f64`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_animation::AnimationController;
    /// use std::time::Duration;
    ///
    /// let controller = AnimationController::builder(Duration::from_millis(100)).build();
    /// let run = controller.animate_to(1.0, None).unwrap();
    /// run.when_complete_or_cancel(|end| {
    ///     assert!(end.is_ok(), "a run nothing superseded or stopped must complete");
    /// });
    /// controller.tick_at(Duration::from_millis(100));
    /// ```
    pub fn animate_to(
        &self,
        target: f64,
        duration: Option<Duration>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_to",
            self.drive_to(target, duration, AnimationDirection::Forward, None),
        )
    }

    /// Animate back to a specific value, defaulting to the reverse duration.
    ///
    /// Like [`animate_to`](Self::animate_to) but, when `duration` is `None`,
    /// defaults to the configured reverse duration (then the base duration),
    /// scaled by the remaining fraction of the range.
    ///
    /// [`status`](crate::Animation::status) is reported as
    /// [`AnimationStatus::Reverse`] regardless of whether `target` is above
    /// or below the current value, ending [`AnimationStatus::ReverseSettled`] —
    /// the mirror of [`animate_to`](Self::animate_to)'s own contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`], [`AnimationError::UnboundedTarget`]
    /// or [`AnimationError::SpanOverflow`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back(
        &self,
        target: f64,
        duration: Option<Duration>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_back",
            self.drive_to(target, duration, AnimationDirection::Reverse, None),
        )
    }

    /// Like [`animate_to`](Self::animate_to), but eases the run through
    /// `curve` instead of running linearly.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`], [`AnimationError::UnboundedTarget`]
    /// or [`AnimationError::SpanOverflow`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_to_curved(
        &self,
        target: f64,
        duration: Option<Duration>,
        curve: impl Curve + 'static,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_to_curved",
            self.drive_to(
                target,
                duration,
                AnimationDirection::Forward,
                Some(Rc::new(curve)),
            ),
        )
    }

    /// Like [`animate_back`](Self::animate_back), but eases the run through
    /// `curve` instead of running linearly.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`], [`AnimationError::UnboundedTarget`]
    /// or [`AnimationError::SpanOverflow`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back_curved(
        &self,
        target: f64,
        duration: Option<Duration>,
        curve: impl Curve + 'static,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_back_curved",
            self.drive_to(
                target,
                duration,
                AnimationDirection::Reverse,
                Some(Rc::new(curve)),
            ),
        )
    }

    /// Shared driver for [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back)
    /// and their `_curved` variants: interpolate from the current value to
    /// `target`, easing through `curve` (`None` = linear).
    ///
    /// `direction` is the METHOD's, not derived from `target`'s relation to
    /// the current value: it is fixed by which method was called, before
    /// `target` is even looked at. It drives both the run's status (Forward/Reverse while
    /// running, ForwardSettled/ReverseSettled at the end —
    /// [`AnimationDirection::settled_status`]) and, when `duration` is
    /// `None`, which base duration (`self.duration`/`self.reverse_duration`)
    /// the remaining-fraction scaling starts from — again the method's
    /// choice, not a travel comparison.
    fn drive_to(
        &self,
        target: f64,
        duration: Option<Duration>,
        direction: AnimationDirection,
        curve: Option<Rc<dyn Curve>>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut curve = Opaque::new(curve);
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;
        let entry_value = inner.value;

        let target = Self::canonicalize_value_target(
            AnimationInput::Target,
            target,
            inner.lower_bound,
            inner.upper_bound,
        )?;
        // `target - value` overflowing f64 (e.g. `set_value(-f64::MAX)` then
        // `animate_to(f64::MAX)`) would make `tick_time_based`'s
        // `start_value + range * eased_t` interior lerp compute `inf * t`,
        // finite but wrong, or — at an already-non-finite `start_value` —
        // `inf * 0.0 = NaN`. Refuse before any mutation rather than let a
        // run install with a span nothing downstream can interpolate.
        let span = target - entry_value;
        if !span.is_finite() {
            return Err(AnimationError::SpanOverflow {
                from: entry_value,
                to: target,
            });
        }
        inner.start_value = inner.value;
        inner.target_value = target;
        inner.direction = direction;

        // Per-run override only — never clobber `inner.duration`. Without an
        // explicit duration, the direction's base duration is scaled by the
        // remaining fraction so partial runs keep the full-range velocity.
        let run_duration = duration.unwrap_or_else(|| {
            let base = match inner.direction {
                AnimationDirection::Forward => inner.duration,
                AnimationDirection::Reverse => inner.reverse_duration.unwrap_or(inner.duration),
            };
            inner.scaled_run_duration(base)
        });
        // No-op fast path: already at the target, or the run duration is
        // zero. Starting a timed run would wait for the full duration,
        // re-notifying value listeners every frame while the value never
        // changes, so settle immediately with a single notification instead
        // (see `forward_from`).
        if target == inner.value || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, curve.take(), retired, inner));
        }

        inner.status = inner.direction.running_status();
        // Queue the restart; finish commits the run before invoking the wake hook.
        let (future, displaced_delivery) = Self::install_run(
            &mut inner,
            RunMode::Timed {
                curve: curve.take(),
                duration: run_duration,
            },
            Settle::Sampled,
            &mut retired,
        );

        let status = inner.status;
        self.deliver_transition(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Settle a run whose distance or duration is trivial: snap the value,
    /// cancel whatever run this displaced, and report the
    /// run's directed settled status — no transient running status, no
    /// full-duration no-op run. Returns an already-complete [`AnimationRunFuture`]
    /// for the (trivial) run this call represents. The single settle
    /// chokepoint for zero-DISTANCE (`forward()` already at the upper bound)
    /// and zero-DURATION (`forward(..., Some(Duration::ZERO))`) runs alike
    /// (issue #1171).
    ///
    /// `entry_value` is the value at the METHOD's entry, before
    /// `forward_from(Some(x))`/`reverse_from(Some(x))` apply `from` —
    /// comparing against the post-`from` value here would miss a jump
    /// (`forward_from(Some(1.0))` from `0.3` would report no value change).
    /// Value listeners fire only when `entry_value` actually differs from
    /// where this settle lands.
    ///
    /// `curve` is the trivial run's own: it retires with that run, never
    /// before the run's outcome commits, so its destructor cannot displace it.
    fn settle_at_target(
        &self,
        entry_value: f64,
        curve: Option<Rc<dyn Curve>>,
        mut retired: RetiredSources,
        mut inner: RefMut<'_, AnimationControllerInner>,
    ) -> AnimationRunFuture {
        if inner.delivering {
            inner.start_value = inner.value;
            inner.status = inner.direction.running_status();
            let (future, displaced) = Self::install_run(
                &mut inner,
                RunMode::Timed {
                    curve,
                    duration: Duration::ZERO,
                },
                Settle::Immediate,
                &mut retired,
            );
            let status = inner.status;
            let change = if inner.value == entry_value {
                ValueChange::Unchanged
            } else {
                ValueChange::Notify
            };
            self.deliver_transition(status, change, displaced, retired, inner);
            return future;
        }
        // Settled synchronously below: the curve outlives the delivery.
        let _curve = Opaque::new(curve);
        inner.value = inner.target_value;
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.take_run(&mut retired).map(RunCompleter::cancel);
        let value_change = if inner.target_value == entry_value {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        self.deliver_transition(status, value_change, delivery, retired, inner);
        AnimationRunFuture::complete()
    }

    /// Repeat between two endpoints as `spec` describes: a [`RepeatMode`]
    /// alone repeats forever over the controller's bounds.
    ///
    /// The run starts from the CURRENT value, clamped into the range — not
    /// from its lower endpoint — so a repeat issued every build (a common
    /// pattern for a looping indicator) progresses instead of freezing at
    /// the start each time; to start at the lower endpoint, call
    /// [`set_value`](Self::set_value) first. `value`/`status`/`direction` at
    /// any later [`tick_at`](Self::tick_at) are a pure function of the
    /// elapsed time since this call and the spec — the frame partition
    /// never changes the answer. Iteration boundaries are measured from that
    /// phase origin, not from a fresh iteration 0: a run started mid-way
    /// ends `n` boundaries later, not `n` full periods (Compose:
    /// `iterations*duration - initialOffset`), and a finite run lands on the
    /// END of its last iteration.
    ///
    /// A zero effective period (an explicit one, or a zero forward duration)
    /// settles SYNCHRONOUSLY at the call, on the end of the last iteration
    /// (the first, for an infinite repeat), instead of installing a run that
    /// could never advance. [`Iterations::Finite(0)`](crate::Iterations::Finite)
    /// also settles at the call, at the current clamped value. Both return an
    /// already-complete future; otherwise an infinite repeat's future
    /// resolves only by cancellation. See [`forward`](Self::forward) for the
    /// returned future's general contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::InvalidRepeatRange`] with the effective
    /// endpoints and a [`RangeDefect`] when the range, clamped into the
    /// controller's bounds, is empty, or when it defaults to an
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded)
    /// controller's infinite bounds: give such a controller a range.
    pub fn repeat(
        &self,
        spec: impl Into<RepeatSpec>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::repeat",
            self.admit_repeat(spec.into()),
        )
    }

    fn admit_repeat(&self, spec: RepeatSpec) -> Result<AnimationRunFuture, AnimationError> {
        let RepeatSpec {
            range,
            mode,
            period,
            iterations,
        } = spec;
        let reverse = mode == RepeatMode::Alternate;
        let count = match iterations {
            Iterations::Finite(count) => Some(count),
            Iterations::Infinite => None,
        };
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;
        let entry_value = inner.value;

        // Clamp the repeat range into the controller's bounds and reject an
        // empty one, so a repeat run can never start `value` (or its ticks)
        // outside `[lower_bound, upper_bound]`. A `ValueRange` is finite and
        // ordered, so only clamping can empty it: a repeat that can never
        // change value would hold the frame loop open doing nothing.
        let (lo, hi) = range.map_or((inner.lower_bound, inner.upper_bound), |range| {
            (
                range.lower().clamp(inner.lower_bound, inner.upper_bound),
                range.upper().clamp(inner.lower_bound, inner.upper_bound),
            )
        });
        if lo >= hi {
            return Err(AnimationError::InvalidRepeatRange {
                min: lo,
                max: hi,
                lower: inner.lower_bound,
                upper: inner.upper_bound,
                defect: RangeDefect::Inverted,
            });
        }
        // An unbounded controller's infinite bounds, defaulted in for a
        // missing range: there is no finite range to repeat inside. A given
        // range, and a bounded controller's bounds, are `ValueRange`s, finite
        // with a finite span, so neither reaches here.
        if !lo.is_finite() || !hi.is_finite() {
            return Err(AnimationError::InvalidRepeatRange {
                min: lo,
                max: hi,
                lower: inner.lower_bound,
                upper: inner.upper_bound,
                defect: RangeDefect::NonFinite,
            });
        }

        // The value at the call is the pure function sampled at elapsed
        // time zero — NOT a bare `lo`: from `value == max` in restart mode
        // that is `lo` (the phase wraps); in bounce mode a
        // value starting at `max` reports the reverse leg. Widen to f64
        // before subtracting — near `max` the f64 difference loses bits,
        // ~60ns of quantization at a 1s period, harmless to the phase this
        // computes.
        let v = inner.value.clamp(lo, hi);

        // `count: Some(0)` is a degenerate case distinct from a zero
        // period: zero cycles run AT ALL, regardless of period, so the
        // value at the call is the plain clamp with no landing jump —
        // there is no cycle to land on. Checked before `RepeatRun` even
        // exists; the run never reaches `tick_repeat`.
        if count == Some(0) {
            inner.direction = AnimationDirection::Forward;
            inner.target_value = v;
            return Ok(self.settle_at_target(entry_value, None, retired, inner));
        }

        // Resolved ONCE, not read live on every tick: a later `set_duration`
        // must not retime an active repeat (the period is captured at the
        // call), and one period
        // for both legs of a bounce keeps the modular-nanosecond arithmetic
        // in `tick_repeat` exact.
        let period = period.unwrap_or(inner.duration);
        let period_ns = period.as_nanos();

        if period_ns == 0 {
            // ZERO EFFECTIVE PERIOD, any count: settle SYNCHRONOUSLY at the
            // call instead of installing a run that can never advance —
            // Android's rule ("0 duration animator, ignore the repeat count
            // and skip to the end", `ValueAnimator.animateBasedOnTime`);
            // Compose rejects it. An infinite zero-period
            // repeat ticking once per frame would hold the frame loop open
            // forever doing nothing, so it settles instead — a documented
            // exception to "an infinite repeat's future resolves only by
            // cancellation", landing where its first iteration ends.
            let (value, direction) =
                AnimationControllerInner::repeat_landing(reverse, lo, hi, count.unwrap_or(1));
            inner.direction = direction;
            inner.target_value = value;
            return Ok(self.settle_at_target(entry_value, None, retired, inner));
        }

        // Every degenerate case has returned: a `RepeatRun` is constructed
        // only here, so `period_ns > 0` holds by construction for every live
        // run and nothing downstream needs to guard it again.
        let ratio = (v - lo) / (hi - lo);
        let initial_ns = (ratio * period_ns as f64).round() as u128;
        let run = RepeatRun {
            reverse,
            min: lo,
            max: hi,
            period,
            period_ns,
            count,
            initial_ns,
        };
        let sample = AnimationControllerInner::repeat_sample(&run, initial_ns);
        inner.direction = sample.direction;
        inner.status = sample.direction.running_status();
        inner.start_value = sample.start;
        inner.target_value = sample.target;
        inner.value = sample.value;
        // Queue the restart; finish commits the run before invoking the wake hook.
        let (future, displaced_delivery) = Self::install_run(
            &mut inner,
            RunMode::Repeat(run),
            Settle::Sampled,
            &mut retired,
        );

        // The value is set directly, without notifying listeners — the
        // value-at-the-call jump is real but reported on the run's first
        // tick, not synchronously here.
        let status = inner.status;
        self.deliver_transition(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Drive the animation with a spring (fling) and initial velocity.
    ///
    /// Positive velocity drives toward the upper bound; negative toward the lower.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] if `velocity` is not finite,
    /// or [`AnimationError::UnboundedTarget`] if this controller is
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded) in
    /// the direction `velocity` drives toward (a fling has no finite end to
    /// spring at).
    /// Returns [`AnimationError::UnderdampedFlingSpring`] if the spring is underdamped
    /// (would oscillate).
    ///
    /// # Example
    ///
    /// ```
    /// # use flui_animation::AnimationController;
    /// # use std::time::Duration;
    /// let controller = AnimationController::builder(Duration::from_millis(300)).build();
    /// controller.fling(1.0).unwrap(); // Fling forward
    /// ```
    pub fn fling(&self, velocity: f64) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::fling",
            self.admit_fling(velocity, None),
        )
    }

    /// Drive toward a bound with a gesture velocity in pixels per second.
    /// `extent` is the positive pixel length corresponding to the controller's range.
    ///
    /// # Errors
    /// Returns [`AnimationError::InvalidExtent`] for non-finite or non-positive
    /// extents, [`AnimationError::NonFiniteInput`] for non-finite velocity,
    /// [`AnimationError::UnboundedTarget`] for an infinite controller span, or
    /// [`AnimationError::VelocityOverflow`] for an unrepresentable converted
    /// velocity. Other refusals match [`Self::fling`].
    pub fn fling_across(
        &self,
        velocity: f64,
        extent: f64,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let admitted = self
            .gesture_velocity(velocity, extent)
            .and_then(|velocity| self.admit_fling(velocity, None));
        self.reported("AnimationController::fling_across", admitted)
    }

    /// `velocity` in gesture units per second across `extent`, converted to
    /// controller units per second.
    fn gesture_velocity(&self, velocity: f64, extent: f64) -> Result<f64, AnimationError> {
        let inner = self.inner.borrow();
        Self::check_run_admission(&inner)?;
        if !extent.is_finite() || extent <= 0.0 {
            return Err(AnimationError::InvalidExtent);
        }
        let span = inner.upper_bound - inner.lower_bound;
        let converted = crate::retarget::rate(velocity, span, extent);
        let refusal = if !velocity.is_finite() {
            Some(AnimationError::NonFiniteInput {
                input: AnimationInput::Velocity,
                value: velocity,
            })
        } else if !span.is_finite() {
            Some(AnimationError::UnboundedTarget {
                input: AnimationInput::Velocity,
                value: velocity,
            })
        } else if !converted.is_finite() {
            Some(AnimationError::VelocityOverflow {
                velocity,
                extent,
                span,
            })
        } else {
            None
        };
        refusal.map_or(Ok(converted), Err)
    }

    /// Drive the animation with a custom spring and initial velocity.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] or [`AnimationError::UnboundedTarget`]
    /// under the same conditions
    /// as [`fling`](Self::fling).
    /// Returns [`AnimationError::UnderdampedFlingSpring`] if the spring is underdamped.
    /// Returns [`AnimationError::Simulation`] if its coefficients cannot be
    /// represented. Every refusal preserves the preceding run and its future.
    pub fn fling_with(
        &self,
        velocity: f64,
        spring: Option<SpringDescription>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::fling_with",
            self.admit_fling(velocity, spring),
        )
    }

    fn admit_fling(
        &self,
        velocity: f64,
        spring: Option<SpringDescription>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;

        if !velocity.is_finite() {
            return Err(AnimationError::NonFiniteInput {
                input: AnimationInput::Velocity,
                value: velocity,
            });
        }

        // Compute into locals; every refusal below must leave `inner`
        // untouched, so `direction` is assigned to the controller only
        // after the underdamped-spring check too -- assigning it any earlier
        // would corrupt `direction` on a refused fling, observable only via
        // a later `stop()`.
        let direction = if velocity < 0.0 {
            AnimationDirection::Reverse
        } else {
            AnimationDirection::Forward
        };
        let target = if velocity < 0.0 {
            inner.lower_bound - FLING_TOLERANCE.distance
        } else {
            inner.upper_bound + FLING_TOLERANCE.distance
        };
        if !target.is_finite() {
            return Err(AnimationError::UnboundedTarget {
                input: AnimationInput::Velocity,
                value: velocity,
            });
        }

        let spring = spring.unwrap_or_else(default_fling_spring);
        if spring.spring_type() == SpringType::Underdamped {
            return Err(AnimationError::UnderdampedFlingSpring {
                damping_ratio: spring.damping_ratio(),
            });
        }

        let sim = SpringSimulation::new(spring, inner.value, target, velocity, Tolerance::DEFAULT)?;

        inner.direction = direction;
        let fling = SimulationRun::Fling {
            source: Rc::new(sim),
            bound: match direction {
                AnimationDirection::Forward => inner.upper_bound,
                AnimationDirection::Reverse => inner.lower_bound,
            },
            direction,
        };
        inner.status = inner.direction.running_status();
        // Queue the restart; finish commits the run before invoking the wake hook.
        let (future, displaced_delivery) = Self::install_run(
            &mut inner,
            RunMode::Simulation(fling),
            Settle::Sampled,
            &mut retired,
        );

        let status = inner.status;
        self.deliver_transition(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Drive the animation according to a custom simulation. Works
    /// unmodified on an [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller — a
    /// simulation is not refused the way a bound-targeting run is, since it
    /// carries its own `is_done` termination rather than running toward
    /// `lower_bound`/`upper_bound`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] if `simulation.x(0.0)` is
    /// not finite.
    ///
    /// # Example
    ///
    /// ```
    /// # use flui_animation::{AnimationController, simulation::SpringSimulation};
    /// # use flui_animation::simulation::{SpringDescription, Tolerance};
    /// # use std::time::Duration;
    /// let controller = AnimationController::builder(Duration::from_millis(300)).build();
    /// let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 0.5).unwrap();
    /// let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT).unwrap();
    /// controller.animate_with(sim).unwrap();
    /// ```
    pub fn animate_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_with",
            self.drive_simulation(Box::new(simulation), AnimationDirection::Forward),
        )
    }

    /// Drive the animation according to a custom simulation, reporting reverse.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteInput`] under the same condition
    /// as [`animate_with`](Self::animate_with).
    pub fn animate_back_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.reported(
            "AnimationController::animate_back_with",
            self.drive_simulation(Box::new(simulation), AnimationDirection::Reverse),
        )
    }

    fn drive_simulation(
        &self,
        simulation: Box<dyn Simulation>,
        direction: AnimationDirection,
    ) -> Result<AnimationRunFuture, AnimationError> {
        // `Simulation::x` is arbitrary caller code, evaluated BEFORE taking
        // a borrow: running it under `inner`'s guard (as a `borrow, evaluate,
        // proceed` shape would) lets a caller whose `x` re-enters this same
        // controller — directly, or through anything reachable from it —
        // panic on a conflicting `RefCell` borrow. A simulation
        // that starts non-finite would otherwise also install a run whose
        // `is_done` may never fire — refused below, before any mutation,
        // exactly like every other non-finite entry point.
        let mut simulation = Opaque::new(Rc::<dyn Simulation>::from(simulation));
        let initial = simulation.get().x(0.0);

        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        // Initial sampling has already run and may have panicked or changed
        // the controller. Of the returned validation errors, Disposed wins
        // over a non-finite initial value; a sampling panic propagates first.
        Self::check_run_admission(&inner)?;

        if !initial.is_finite() {
            return Err(AnimationError::NonFiniteInput {
                input: AnimationInput::SimulationStart,
                value: initial,
            });
        }

        inner.direction = direction;
        inner.status = direction.running_status();
        inner.value = initial.clamp(inner.lower_bound, inner.upper_bound);
        // Queue the restart; finish commits the run before invoking the wake hook.
        let (future, displaced_delivery) = Self::install_run(
            &mut inner,
            RunMode::Simulation(SimulationRun::Custom(simulation.take())),
            Settle::Sampled,
            &mut retired,
        );

        let status = inner.status;
        self.deliver_transition(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }
}
