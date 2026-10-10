//! `CurvedAnimation` - applies easing curves to animations.

use crate::animation::{Animation, ParentLinks, Retirement, StatusCallback, Terminal};
use crate::curve::Curve;
use crate::status::AnimationStatus;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// Captures the run's entering direction while animating (keeping an already
/// captured one) and clears it at rest. Reads only the reported
/// `AnimationStatus` — no separate "is a ticker literally running" check.
/// Shared by the constructor's seed call and the status-listener callback so
/// both apply the identical rule.
fn update_curve_direction(direction: &RefCell<Option<AnimationStatus>>, status: AnimationStatus) {
    let mut direction = direction.borrow_mut();
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
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let controller = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
///
/// let curved = CurvedAnimation::new(controller, Curves::EaseInOut);
/// ```
#[derive(Clone)]
pub struct CurvedAnimation<C: Curve + Clone> {
    curve: Terminal<C>,
    reverse_curve: Option<Terminal<C>>,
    links: Terminal<Rc<ParentLinks>>,
    curve_direction: Rc<RefCell<Option<AnimationStatus>>>,
}

impl<C: Curve + Clone> Drop for CurvedAnimation<C> {
    fn drop(&mut self) {
        let mut retirement = Retirement::new();
        self.links.inherit_failure(&mut retirement.scope());
        let links = self.links.withdraw();
        let curve = self.curve.withdraw();
        let reverse = self.reverse_curve.take();
        // Curves belong to each value clone; parent subscriptions belong to the
        // shared links allocation and remain installed until its final drop.
        retirement.run(|| drop(links.into_inner()));
        retirement.retire(curve);
        retirement.retire(reverse);
        retirement.finish();
    }
}

impl<C: Curve + Clone> CurvedAnimation<C> {
    /// Create a new curved animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    /// * `curve` - The curve to apply
    #[must_use]
    pub fn new(parent: Rc<dyn Animation<f64>>, curve: C) -> Self {
        let parent = Terminal::new(parent);
        let curve = Terminal::new(curve);
        let curve_direction = Rc::new(RefCell::new(None));
        update_curve_direction(&curve_direction, parent.status());
        let weak_direction = Rc::downgrade(&curve_direction);
        let links = ParentLinks::new(parent.into_inner(), move |status| {
            if let Some(direction) = weak_direction.upgrade() {
                update_curve_direction(&direction, status);
            }
            status
        });
        Self {
            curve,
            reverse_curve: None,
            links: Terminal::new(links),
            curve_direction,
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
        let captured: Option<AnimationStatus> = *self.curve_direction.borrow_mut();
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

impl<C: Curve + Clone + fmt::Debug + 'static> Animation<f64> for CurvedAnimation<C> {
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

    fn is_animating(&self) -> bool {
        self.links.parent.is_animating()
    }

    fn subscribe_status(&self, callback: StatusCallback) -> crate::StatusSubscription {
        self.links.subscribe_status(callback)
    }

    fn subscribe_status_observer(
        &self,
        observer: crate::animation::StatusObserver,
    ) -> crate::StatusSubscription {
        self.links.subscribe_status_observer(observer)
    }
}

impl<C: Curve + Clone> Listenable for CurvedAnimation<C> {
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.links.notifier.add_observer(observer)
    }
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

impl<C: Curve + Clone + fmt::Debug + 'static> fmt::Debug for CurvedAnimation<C> {
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
