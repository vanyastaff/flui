//! ProxyAnimation - wraps another animation, allowing hot-swapping.

use crate::animation::{Animation, StatusCallback};
use flui_core::foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_types::animation::AnimationStatus;
use parking_lot::RwLock;
use std::fmt;
use std::sync::Arc;

/// An animation that can be hot-swapped for another animation.
///
/// ProxyAnimation forwards all calls to its parent animation, but allows
/// the parent to be changed dynamically. This is useful when you need to
/// change the animation being used without recreating the entire widget tree.
///
/// # Examples
///
/// ```
/// use flui_animation::{ProxyAnimation, AnimationController};
/// use flui_core::foundation::SimpleTickerProvider;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let ticker_provider = Arc::new(SimpleTickerProvider);
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     ticker_provider.clone(),
/// ));
///
/// let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);
///
/// // Later, swap to a different animation
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(500),
///     ticker_provider,
/// ));
/// proxy.set_parent(controller2 as Arc<dyn Animation<f32>>);
/// ```
#[derive(Clone)]
pub struct ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    parent: Arc<RwLock<Arc<dyn Animation<T>>>>,
    notifier: Arc<ChangeNotifier>,
}

impl<T> ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    /// Create a new proxy animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The initial parent animation
    pub fn new(parent: Arc<dyn Animation<T>>) -> Self {
        Self {
            parent: Arc::new(RwLock::new(parent)),
            notifier: Arc::new(ChangeNotifier::new()),
        }
    }

    /// Get the current parent animation.
    #[must_use]
    pub fn parent(&self) -> Arc<dyn Animation<T>> {
        self.parent.read().clone()
    }

    /// Set a new parent animation.
    ///
    /// This will cause all listeners to be notified of the change.
    pub fn set_parent(&self, new_parent: Arc<dyn Animation<T>>) {
        *self.parent.write() = new_parent;
        self.notifier.notify_listeners();
    }
}

impl<T> Animation<T> for ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    fn value(&self) -> T {
        self.parent.read().value()
    }

    fn status(&self) -> AnimationStatus {
        self.parent.read().status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.parent.read().add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.read().remove_status_listener(id)
    }
}

impl<T> Listenable for ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn add_listener(&mut self, callback: ListenerCallback) -> ListenerId {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .add_listener(callback)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_listener(id)
    }

    fn remove_all_listeners(&mut self) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_all_listeners()
    }
}

impl<T> fmt::Debug for ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_core::foundation::SimpleTickerProvider;
    use std::time::Duration;

    #[test]
    fn test_proxy_animation() {
        let ticker_provider = Arc::new(SimpleTickerProvider);
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            ticker_provider.clone(),
        ));

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);

        controller1.set_value(0.5);
        assert_eq!(proxy.value(), 0.5);

        // Swap to a different animation
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(200),
            ticker_provider,
        ));
        controller2.set_value(0.75);
        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f32>>);

        assert_eq!(proxy.value(), 0.75);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_proxy_animation_status() {
        let ticker_provider = Arc::new(SimpleTickerProvider);
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            ticker_provider,
        ));

        let proxy = ProxyAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);

        assert_eq!(proxy.status(), AnimationStatus::Dismissed);

        controller.forward().unwrap();
        assert_eq!(proxy.status(), AnimationStatus::Forward);

        controller.dispose();
    }
}
