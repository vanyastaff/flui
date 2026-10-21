//! Main-thread native ownership with nonblocking final release from workers.
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::MainThreadMarker;

pub(super) struct NativeOwner<T: 'static>(Option<MainThreadBound<T>>);

impl<T: 'static> NativeOwner<T> {
    pub(super) fn new(value: T, marker: MainThreadMarker) -> Self {
        Self(Some(MainThreadBound::new(value, marker)))
    }

    pub(super) fn get(&self, marker: MainThreadMarker) -> &T {
        self.0
            .as_ref()
            .expect("BUG: live native owner has its value")
            .get(marker)
    }
}

impl<T: 'static> Drop for NativeOwner<T> {
    fn drop(&mut self) {
        let value = self.0.take();
        if MainThreadMarker::new().is_some() {
            crate::shared::panic_boundary::contain_owner_callback(|| drop(value));
        } else {
            // MainThreadBound's default Drop synchronously waits on main. A
            // joining owner must not deadlock against this worker's final drop.
            DispatchQueue::main().exec_async(move || {
                crate::shared::panic_boundary::contain_owner_callback(|| drop(value));
            });
        }
    }
}
