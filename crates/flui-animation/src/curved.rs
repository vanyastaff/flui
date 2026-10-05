//! `CurvedAnimation` - applies easing curves to animations.

use crate::animation::{
    Animation, ParentSubscription, Retirement, StatusCallback, Terminal, link_parent,
};
use crate::curve::Curve;
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// Captures the run's entering direction while animating (keeping an already
/// captured one) and clears it at rest. Reads only the reported
/// `AnimationStatus` — no separate "is a ticker literally running" check.
/// Shared by the constructor's seed call and the status-listener callback so
/// both apply the identical rule.
fn update_curve_direction(direction: &Mutex<Option<AnimationStatus>>, status: AnimationStatus) {
    let mut direction = direction.lock();
    match status {
        // At rest the lock is released; the next transition re-captures.
        AnimationStatus::Dismissed | AnimationStatus::Completed => *direction = None,
        // First transition into a running status wins; a same-run direction
        // flip (or a directional report from `set_value` that isn't a fresh
        // run) keeps the entry curve.
        AnimationStatus::Forward | AnimationStatus::Reverse => {
            direction.get_or_insert(status);
        }
    }
}

/// An animation that applies a curve to another animation.
///
/// Takes an `Animation<f64>` (typically an `AnimationController`) and applies
/// an easing curve to transform the linear 0.0..1.0 progression into a
/// non-linear progression.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, CurvedAnimation};
/// use flui_animation::Curves;
/// use flui_scheduler::UpdateScheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
///
/// let curved = CurvedAnimation::new(controller, Curves::EaseInOut);
/// ```
#[derive(Clone)]
pub struct CurvedAnimation<C: Curve + Clone + Send + Sync> {
    curve: Terminal<C>,
    reverse_curve: Option<Terminal<C>>,
    links: Terminal<Arc<CurvedLinks>>,
}

struct CurvedLinks {
    parent: Terminal<Arc<dyn Animation<f64>>>,
    notifier: Terminal<Arc<ChangeNotifier>>,
    curve_direction: Arc<Mutex<Option<AnimationStatus>>>,
    parent_sub: Terminal<Arc<ParentSubscription>>,
    status_sub: Terminal<Arc<ParentSubscription>>,
}

impl Drop for CurvedLinks {
    fn drop(&mut self) {
        let parent = self.parent.withdraw();
        let notifier = self.notifier.withdraw();
        let value_sub = self.parent_sub.withdraw();
        let status_sub = self.status_sub.withdraw();
        let mut retirement = Retirement::new();
        value_sub.detach(&mut retirement);
        status_sub.detach(&mut retirement);
        retirement.retire(value_sub);
        retirement.retire(status_sub);
        retirement.retire(parent);
        retirement.retire(notifier);
        retirement.finish();
    }
}

impl<C: Curve + Clone + Send + Sync> Drop for CurvedAnimation<C> {
    fn drop(&mut self) {
        let links = self.links.withdraw();
        let curve = self.curve.withdraw();
        let reverse = self.reverse_curve.take();
        // Curves belong to each value clone; parent subscriptions belong to the
        // shared links allocation and remain installed until its final drop.
        let mut retirement = Retirement::new();
        retirement.run(|| drop(links.into_inner()));
        retirement.retire(curve);
        retirement.retire(reverse);
        retirement.finish();
    }
}

impl<C: Curve + Clone + Send + Sync> CurvedAnimation<C> {
    /// Create a new curved animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    /// * `curve` - The curve to apply
    #[must_use]
    pub fn new(parent: Arc<dyn Animation<f64>>, curve: C) -> Self {
        let parent = Terminal::new(parent);
        let curve = Terminal::new(curve);
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);

        let curve_direction = Arc::new(Mutex::new(None));
        // The seed runs BEFORE the status listener is registered. A
        // `CurvedAnimation` built while the parent is already mid-run (e.g.
        // constructed against a controller some other code already called
        // `forward()` on) captures the run's entering direction immediately;
        // without this seed, `curve_direction` stays `None` until the
        // listener observes its first transition — and if that first
        // transition is a mid-run *flip* (`reverse()` right after
        // construction), the flip itself would be wrongly recorded as the
        // entering direction instead of preserved as a flip.
        update_curve_direction(&curve_direction, parent.status());
        let weak_direction = Arc::downgrade(&curve_direction);
        let status_id = parent.add_status_listener(Arc::new(move |status| {
            if let Some(direction) = weak_direction.upgrade() {
                update_curve_direction(&direction, status);
            }
        }));
        let status_parent = Arc::clone(&parent);
        let status_sub = ParentSubscription::new(move || {
            status_parent.remove_status_listener(status_id);
        });

        Self {
            curve,
            reverse_curve: None,
            links: Terminal::new(Arc::new(CurvedLinks {
                parent,
                notifier: Terminal::new(notifier),
                curve_direction,
                parent_sub: Terminal::new(parent_sub),
                status_sub: Terminal::new(status_sub),
            })),
        }
    }

    /// Set a different curve for reverse animation.
    #[must_use]
    pub fn with_reverse_curve(mut self, reverse_curve: C) -> Self {
        let old = self.reverse_curve.replace(Terminal::new(reverse_curve));
        drop(old);
        self
    }

    /// Get the current curve being used (respects reverse).
    ///
    /// Uses the direction captured at run start when running (so a mid-run
    /// direction flip keeps the entry curve), falling back to the parent's
    /// instantaneous status at rest.
    #[inline]
    fn current_curve(&self) -> &C {
        let captured: Option<AnimationStatus> = *self.links.curve_direction.lock();
        let effective = captured.unwrap_or_else(|| self.links.parent.status());
        match effective {
            AnimationStatus::Reverse => self
                .reverse_curve
                .as_ref()
                .map_or(self.curve.get(), Terminal::get),
            _ => &self.curve,
        }
    }
}

impl<C: Curve + Clone + Send + Sync + fmt::Debug + 'static> Animation<f64> for CurvedAnimation<C> {
    #[inline]
    fn value(&self) -> f64 {
        let t = self.links.parent.value();
        let curve = self.current_curve();
        curve.transform(t)
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.links.parent.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.links.parent.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.links.parent.remove_status_listener(id);
    }
}

impl<C: Curve + Clone + Send + Sync> Listenable for CurvedAnimation<C> {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.links.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.links.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.links.notifier.remove_all_listeners();
    }
}

impl<C: Curve + Clone + Send + Sync + fmt::Debug + 'static> fmt::Debug for CurvedAnimation<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value_str = format!("{:.3}", self.value());
        f.debug_struct("CurvedAnimation")
            .field("value", &value_str)
            .field("status", &self.status())
            .field("curve", &self.curve)
            .field("has_reverse_curve", &self.reverse_curve.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use crate::curve::{Cubic, Curves};
    use flui_scheduler::UpdateScheduler;
    use std::time::Duration;

    #[test]
    fn reverse_curve_locked_to_run_entry_direction() {
        // A run that entered Forward keeps
        // the forward curve even if the parent's status flips to Reverse
        // mid-run; the reverse curve only applies to a run entered in
        // Reverse. Without the lock, a mid-run `reverse()` would swap curves
        // underneath the value and cause a visual jump.
        let scheduler = UpdateScheduler::new();
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            &scheduler,
        ));
        // Forward curve is the identity cubic; the reverse curve is strongly
        // sub-linear at t=0.5, so any curve swap is observable there.
        let curved = CurvedAnimation::new(
            controller.clone() as Arc<dyn Animation<f64>>,
            Cubic::new(0.0, 0.0, 1.0, 1.0), // y(x) = x
        )
        .with_reverse_curve(Curves::EaseInQuint);

        controller.set_value(0.5);
        let _ = controller.forward();
        let during_forward = curved.value();

        // Flip direction mid-run: the captured Forward direction must keep
        // the (≈linear) forward curve active.
        let _ = controller.reverse();
        let during_flip = curved.value();
        assert!(
            (during_forward - during_flip).abs() < 1e-3,
            "mid-run direction flip must not swap curves (forward {during_forward} vs flipped {during_flip})"
        );

        // Settle the run, then start a fresh run in Reverse: now the reverse
        // curve applies from the start.
        controller.set_value(1.0); // Completed -> direction lock cleared
        let _ = controller.reverse();
        controller.set_value(0.5);
        let reverse_run = curved.value();
        let expected = Curves::EaseInQuint.transform(0.5);
        assert!(
            (reverse_run - expected).abs() < 1e-3,
            "a run entered in Reverse must use the reverse curve ({reverse_run} vs {expected})"
        );

        controller.dispose();
    }
}
