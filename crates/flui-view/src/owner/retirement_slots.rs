use std::sync::Arc;

// These concrete slots keep their ordinary field position and destruction order.
// They retain only their own envelope when a prior failure is already unwinding.
pub(super) struct OwnerReactive(pub(super) Option<crate::reactive::Reactive>);
impl std::ops::Deref for OwnerReactive {
    type Target = crate::reactive::Reactive;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("BUG: live owner graph is occupied")
    }
}
impl Drop for OwnerReactive {
    fn drop(&mut self) {
        let graph = self.0.take();
        if std::thread::panicking() {
            std::mem::forget(graph);
        } else {
            drop(graph);
        }
    }
}

pub(crate) struct OwnerTreeObserverSlot(
    pub(super) Option<Arc<dyn flui_foundation::observe::TreeObserver>>,
);
impl std::ops::Deref for OwnerTreeObserverSlot {
    type Target = Option<Arc<dyn flui_foundation::observe::TreeObserver>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnerTreeObserverSlot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OwnerTreeObserverSlot {
    fn drop(&mut self) {
        let observer = self.0.take();
        if std::thread::panicking() {
            std::mem::forget(observer);
        } else {
            drop(observer);
        }
    }
}

pub(crate) struct OwnerBuildScheduledSlot(pub(super) Option<Arc<dyn Fn() + Send + Sync>>);
impl std::ops::Deref for OwnerBuildScheduledSlot {
    type Target = Option<Arc<dyn Fn() + Send + Sync>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnerBuildScheduledSlot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OwnerBuildScheduledSlot {
    fn drop(&mut self) {
        let callback = self.0.take();
        if std::thread::panicking() {
            std::mem::forget(callback);
        } else {
            drop(callback);
        }
    }
}
