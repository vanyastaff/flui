//! Poll-scoped initializer ancestry shared by clones of one cache.

use std::sync::Arc;
use std::thread::ThreadId;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

struct Active<K> {
    thread: ThreadId,
    key: Arc<K>,
}

pub(super) struct Initializers<K> {
    active: parking_lot::Mutex<Vec<Arc<Active<K>>>>,
}

impl<K: Eq> Initializers<K> {
    pub(super) fn new() -> Self {
        Self {
            active: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub(super) fn is_reentrant(&self, key: &K) -> bool {
        let thread = std::thread::current().id();
        let ancestors: Vec<_> = self
            .active
            .lock()
            .iter()
            .filter(|entry| entry.thread == thread)
            .map(|entry| Arc::clone(&entry.key))
            .collect();
        // Generic equality and key destruction can call user code.
        ancestors.iter().any(|ancestor| **ancestor == *key)
    }

    pub(super) fn run<'a, F: Future>(
        &'a self,
        key: &'a Arc<K>,
        future: F,
    ) -> ScopedInitializer<'a, K, F> {
        ScopedInitializer {
            initializers: self,
            key,
            future: Some(Box::pin(future)),
        }
    }

    fn enter(&self, key: &Arc<K>) -> Scope<'_, K> {
        let entry = Arc::new(Active {
            thread: std::thread::current().id(),
            key: Arc::clone(key),
        });
        self.active.lock().push(Arc::clone(&entry));
        Scope {
            initializers: self,
            entry,
        }
    }
}

pub(super) struct ScopedInitializer<'a, K: Eq, F: Future> {
    initializers: &'a Initializers<K>,
    // The owner outside Moka's future keeps this key alive until its waiter
    // guard and initializer have both retired, including during cancellation.
    key: &'a Arc<K>,
    future: Option<Pin<Box<F>>>,
}

impl<K: Eq, F: Future> Future for ScopedInitializer<'_, K, F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let _scope = this.initializers.enter(this.key);
        let result = this
            .future
            .as_mut()
            .expect("BUG: completed initializer is not polled again")
            .as_mut()
            .poll(cx);
        if result.is_ready() {
            // Future captures may reenter before Moka releases its waiter.
            drop(this.future.take());
        }
        result
    }
}

impl<K: Eq, F: Future> Drop for ScopedInitializer<'_, K, F> {
    fn drop(&mut self) {
        if let Some(future) = self.future.take() {
            let _scope = self.initializers.enter(self.key);
            drop(future);
        }
    }
}

struct Scope<'a, K> {
    initializers: &'a Initializers<K>,
    entry: Arc<Active<K>>,
}

impl<K> Drop for Scope<'_, K> {
    fn drop(&mut self) {
        let retired = {
            let mut active = self.initializers.active.lock();
            let index = active
                .iter()
                .position(|entry| Arc::ptr_eq(entry, &self.entry))
                .expect("BUG: active initializer scope is registered");
            active.swap_remove(index)
        };
        drop(retired);
    }
}
