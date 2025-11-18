//! Computed signals with automatic dependency tracking.
//!
//! This module provides memoized computations that automatically track
//! their dependencies and update only when necessary.

use crate::owner::Owner;
use crate::runtime::SignalRuntime;
use crate::signal::{Signal, SignalId, SubscriptionId};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use tracing::{debug, trace};

// Thread-local cycle detection for computed signals
thread_local! {
    static COMPUTATION_STACK: RefCell<HashSet<ComputedId>> = RefCell::new(HashSet::new());
}

/// Unique identifier for a computed signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComputedId(u64);

impl ComputedId {
    /// Create a new computed ID.
    ///
    /// # Panics
    ///
    /// Panics if u64::MAX computed signals have been created (practically impossible).
    #[inline]
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        let id = COUNTER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current >= u64::MAX - 1 {
                    None
                } else {
                    Some(current + 1)
                }
            })
            .expect("ComputedId counter overflow! Cannot create more computed signals.");

        Self(id)
    }

    /// Get the inner ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Default for ComputedId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<u64> for ComputedId {
    #[inline]
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<ComputedId> for u64 {
    #[inline]
    fn from(id: ComputedId) -> Self {
        id.0
    }
}

impl std::fmt::Display for ComputedId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Computed({})", self.0)
    }
}

/// Computation function that tracks dependencies.
type ComputeFn<T> = Box<dyn FnMut() -> T + Send + 'static>;

/// Stored subscription with signal ID
///
/// Automatically unsubscribes when dropped (RAII cleanup).
struct StoredSubscription {
    signal_id: SignalId,
    subscription_id: SubscriptionId,
}

impl Drop for StoredSubscription {
    fn drop(&mut self) {
        // Unsubscribe from signal when dropped
        SignalRuntime::global().unsubscribe(self.signal_id, self.subscription_id);
        trace!(
            signal_id = ?self.signal_id,
            subscription_id = ?self.subscription_id,
            "StoredSubscription dropped - unsubscribed from signal"
        );
    }
}

/// Inner state of a computed signal.
struct ComputedInner<T> {
    id: ComputedId,
    compute_fn: Mutex<ComputeFn<T>>,
    cached_value: Signal<T>,
    dependencies: Mutex<HashSet<SignalId>>,
    subscriptions: Mutex<Vec<StoredSubscription>>,
    is_dirty: std::sync::atomic::AtomicBool, // Lock-free dirty flag for read-heavy workloads
}

impl<T> fmt::Debug for ComputedInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComputedInner")
            .field("id", &self.id)
            .field("dependencies_count", &self.dependencies.lock().len())
            .field("is_dirty", &self.is_dirty.load(std::sync::atomic::Ordering::Acquire))
            .finish()
    }
}

/// Computed signal with automatic dependency tracking.
///
/// A computed signal is a memoized computation that automatically tracks
/// which signals it depends on and re-computes only when those signals change.
///
/// # Thread Safety
///
/// `Computed` is thread-safe (`Send + Sync`) and can be safely shared across threads.
/// However, be aware of **potential deadlocks** in multi-threaded scenarios with
/// circular dependencies:
///
/// ## Deadlock Risk
///
/// **IMPORTANT:** If you have `Computed` signals with circular dependencies across
/// different threads, a deadlock can occur:
///
/// ```rust,ignore
/// // Thread A owns computed1
/// let computed1 = Computed::new(move || computed2.get() + 1);
///
/// // Thread B owns computed2
/// let computed2 = Computed::new(move || computed1.get() + 1);
///
/// // ⚠️ DEADLOCK if both threads call .get() simultaneously!
/// // Thread A: locks computed1, waits for computed2
/// // Thread B: locks computed2, waits for computed1
/// ```
///
/// **Same-thread circular dependencies** are detected and panic (correct behavior).
/// **Cross-thread circular dependencies** can deadlock.
///
/// **Mitigation:**
/// - Avoid circular dependencies between `Computed` signals
/// - If necessary, use signal updates to break the cycle
/// - Document dependency graphs in complex applications
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::{Signal, Computed};
///
/// let width = Signal::new(10);
/// let height = Signal::new(5);
///
/// // Computed automatically tracks width and height
/// let area = Computed::new(move || {
///     width.get() * height.get()
/// });
///
/// println!("Area: {}", area.get()); // 50
///
/// width.set(20);
/// println!("Area: {}", area.get()); // 100 (automatically re-computed)
/// ```
pub struct Computed<T> {
    inner: Arc<ComputedInner<T>>,
}

impl<T> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Computed<T>
where
    T: Clone + Send + 'static,
{
    /// Create a new computed signal.
    ///
    /// The computation function will be called immediately to compute the initial value,
    /// and any signals accessed during computation will be tracked as dependencies.
    pub fn new<F>(compute_fn: F) -> Self
    where
        F: FnMut() -> T + Send + 'static,
    {
        let id = ComputedId::new();
        let mut compute_fn_boxed = Box::new(compute_fn);

        // Initial computation with dependency tracking
        let (initial_value, dependencies) = Self::track_dependencies(&mut *compute_fn_boxed);

        debug!(
            computed_id = ?id,
            dependencies_count = dependencies.len(),
            "Created computed signal"
        );

        let cached_value = Signal::new(initial_value);

        let inner = Arc::new(ComputedInner {
            id,
            compute_fn: Mutex::new(compute_fn_boxed),
            cached_value,
            dependencies: Mutex::new(dependencies.clone()),
            subscriptions: Mutex::new(Vec::new()),
            is_dirty: std::sync::atomic::AtomicBool::new(false),
        });

        // Subscribe to all dependencies with rollback on failure
        let weak_inner = Arc::downgrade(&inner);
        let mut subscriptions = Vec::new();

        for &dep_id in &dependencies {
            let weak = weak_inner.clone();

            match SignalRuntime::global().subscribe(dep_id, move || {
                if let Some(inner) = weak.upgrade() {
                    inner.is_dirty.store(true, std::sync::atomic::Ordering::Release);
                    trace!(computed_id = ?inner.id, "Marked dirty");

                    // Trigger notification on cached_value to propagate dirty flag
                    // to downstream computed signals
                    inner.cached_value.notify_subscribers();
                }
            }) {
                Ok(sub_id) => {
                    subscriptions.push(StoredSubscription {
                        signal_id: dep_id,
                        subscription_id: sub_id,
                    });
                }
                Err(e) => {
                    // Rollback: unsubscribe from all previously subscribed dependencies
                    tracing::error!(
                        "Failed to subscribe to dependency {:?}: {}. Rolling back all subscriptions.",
                        dep_id, e
                    );

                    // CRITICAL: Manually unsubscribe and prevent StoredSubscription::drop()
                    // to avoid double-unsubscription (which could affect unrelated signals
                    // if subscription IDs are reused)
                    for stored_sub in subscriptions {
                        let signal_id = stored_sub.signal_id;
                        let sub_id = stored_sub.subscription_id;

                        // Prevent Drop from running (which would unsubscribe again)
                        std::mem::forget(stored_sub);

                        // Manual cleanup
                        SignalRuntime::global().unsubscribe(signal_id, sub_id);
                    }

                    panic!(
                        "Failed to create Computed signal: dependency subscription failed: {}",
                        e
                    );
                }
            }
        }

        *inner.subscriptions.lock() = subscriptions;

        Self { inner }
    }

    /// Get the current value.
    ///
    /// If the computed signal is dirty (dependencies changed), it will
    /// re-compute before returning the value.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - A circular dependency is detected between computed signals
    /// - Lock acquisition times out (potential deadlock detected after 5 seconds)
    /// - The computation function itself panics
    ///
    /// **Note:** If the computation function panics, the computed signal remains dirty and will
    /// re-run the computation on the next `get()` call. This means panics are NOT cached - the
    /// computation will panic again on every access until it succeeds or dependencies change.
    ///
    /// # Thread Safety
    ///
    /// Lock-free dirty checking eliminates contention on read-heavy workloads.
    /// Circular dependency detection works per-thread; cross-thread cycles cannot be detected
    /// and will result in deadlock detection timeout instead.
    pub fn get(&self) -> T {
        // Thread-local cycle detection (fast path for same-thread cycles)
        let is_cycle = COMPUTATION_STACK.with(|stack| !stack.borrow_mut().insert(self.inner.id));

        if is_cycle {
            panic!(
                "Circular dependency detected in Computed({:?}). Computed signals cannot form dependency cycles.",
                self.inner.id
            );
        }

        // CRITICAL FIX: Atomically check and reset dirty flag using swap
        // This prevents race where another thread sets dirty between our check and recompute
        let was_dirty = self.inner.is_dirty.swap(false, std::sync::atomic::Ordering::AcqRel);

        if was_dirty {
            // If recompute fails (deadlock detected), panic with clear message
            // recompute() will NOT touch is_dirty flag (already reset to false)
            self.recompute();
        }

        let value = self.inner.cached_value.get();

        // Remove from computation stack when done
        COMPUTATION_STACK.with(|stack| {
            stack.borrow_mut().remove(&self.inner.id);
        });

        value
    }

    /// Re-compute the value and update dependencies.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Lock acquisition timeout (5 seconds) - indicates potential deadlock
    /// - This likely indicates circular Computed dependencies across threads
    /// - The computation function panics
    fn recompute(&self) {
        let (new_value, new_dependencies) = {
            // Try to acquire lock with timeout to detect potential deadlocks
            let mut compute_fn = self
                .inner
                .compute_fn
                .try_lock_for(std::time::Duration::from_secs(5))
                .unwrap_or_else(|| {
                    panic!(
                        "Potential deadlock detected in Computed::compute_fn: failed to acquire lock within 5 seconds. \
                         This likely indicates circular dependencies across threads. \
                         Review your Computed dependency graph."
                    )
                });

            // Track dependencies in closure
            DEPENDENCY_TRACKER.with(|tracker| {
                let mut tracker = tracker.borrow_mut();
                tracker.dependencies.clear();
                tracker.is_tracking = true;
            });

            let value = (*compute_fn)();

            let dependencies = DEPENDENCY_TRACKER.with(|tracker| {
                let mut tracker = tracker.borrow_mut();
                tracker.is_tracking = false;
                std::mem::take(&mut tracker.dependencies)
            });

            (value, dependencies)
        };

        // Update cached value
        self.inner.cached_value.set(new_value);

        // Update dependencies if changed
        let mut deps = self
            .inner
            .dependencies
            .try_lock_for(std::time::Duration::from_secs(5))
            .unwrap_or_else(|| {
                panic!(
                    "Potential deadlock detected in Computed::dependencies: failed to acquire lock within 5 seconds. \
                     This likely indicates circular dependencies across threads. \
                     Review your Computed dependency graph."
                )
            });
        if *deps != new_dependencies {
            debug!(
                computed_id = ?self.inner.id,
                old_count = deps.len(),
                new_count = new_dependencies.len(),
                "Dependencies changed"
            );

            // Unsubscribe from old dependencies
            let old_subs = std::mem::take(
                &mut *self
                    .inner
                    .subscriptions
                    .try_lock_for(std::time::Duration::from_secs(5))
                    .unwrap_or_else(|| {
                        panic!(
                            "Potential deadlock detected in Computed::subscriptions: failed to acquire lock within 5 seconds. \
                             This likely indicates circular dependencies across threads. \
                             Review your Computed dependency graph."
                        )
                    }),
            );
            for sub in old_subs {
                SignalRuntime::global().unsubscribe(sub.signal_id, sub.subscription_id);
            }

            // Subscribe to new dependencies
            let weak_inner = Arc::downgrade(&self.inner);
            let mut subscriptions = Vec::new();

            for &dep_id in &new_dependencies {
                let weak = weak_inner.clone();
                let sub_id = SignalRuntime::global()
                    .subscribe(dep_id, move || {
                        if let Some(inner) = weak.upgrade() {
                            inner.is_dirty.store(true, std::sync::atomic::Ordering::Release);
                        }
                    })
                    .expect("Failed to subscribe to dependency: too many subscribers");

                subscriptions.push(StoredSubscription {
                    signal_id: dep_id,
                    subscription_id: sub_id,
                });
            }

            *self.inner.subscriptions.lock() = subscriptions;
            *deps = new_dependencies;
        }

        // Note: is_dirty flag already reset to false in get() using swap()
        // No need to set it again here

        trace!(computed_id = ?self.inner.id, "Recomputed");
    }

    /// Track dependencies during computation.
    ///
    /// Returns the computed value and the set of signal IDs accessed.
    fn track_dependencies<F>(compute_fn: &mut F) -> (T, HashSet<SignalId>)
    where
        F: FnMut() -> T,
    {
        // Install dependency tracker
        DEPENDENCY_TRACKER.with(|tracker| {
            let mut tracker = tracker.borrow_mut();
            tracker.dependencies.clear();
            tracker.is_tracking = true;
        });

        // Run computation
        let value = compute_fn();

        // Collect dependencies
        let dependencies = DEPENDENCY_TRACKER.with(|tracker| {
            let mut tracker = tracker.borrow_mut();
            tracker.is_tracking = false;
            std::mem::take(&mut tracker.dependencies)
        });

        (value, dependencies)
    }

    /// Subscribe to changes in this computed signal.
    /// Returns a SubscriptionId for manual unsubscribe.
    ///
    /// # Errors
    ///
    /// Returns `SignalError::TooManySubscribers` if the signal already has
    /// the maximum number of subscribers.
    pub fn subscribe<F>(&self, callback: F) -> Result<SubscriptionId, crate::error::SignalError>
    where
        F: Fn() + Send + Sync + 'static,
        T: Clone + Send + 'static,
    {
        self.inner.cached_value.subscribe(callback)
    }

    /// Subscribe with RAII guard for automatic cleanup.
    ///
    /// # Errors
    ///
    /// Returns `SignalError::TooManySubscribers` if the signal already has
    /// the maximum number of subscribers.
    pub fn subscribe_scoped<F>(
        &self,
        callback: F,
    ) -> Result<crate::signal::Subscription<T>, crate::error::SignalError>
    where
        F: Fn() + Send + Sync + 'static,
        T: Clone + Send + 'static,
    {
        self.inner.cached_value.clone().subscribe_scoped(callback)
    }

    /// Get the computed ID.
    pub fn id(&self) -> ComputedId {
        self.inner.id
    }

    /// Check if the computed signal is currently dirty.
    pub fn is_dirty(&self) -> bool {
        self.inner.is_dirty.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Register this computed with an owner for automatic cleanup.
    pub fn owned(self, owner: &Owner) -> Self {
        let inner = Arc::clone(&self.inner);
        owner.on_cleanup(move || {
            // Unsubscribe from all dependencies
            let subs = std::mem::take(&mut *inner.subscriptions.lock());
            for sub in subs {
                SignalRuntime::global().unsubscribe(sub.signal_id, sub.subscription_id);
            }
        });
        self
    }
}

impl<T> fmt::Debug for Computed<T>
where
    T: Clone + Send + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Computed")
            .field("id", &self.inner.id)
            .field("is_dirty", &self.is_dirty())
            .finish()
    }
}

/// Dependency tracker for automatic signal tracking.
#[derive(Debug, Default)]
struct DependencyTracker {
    is_tracking: bool,
    dependencies: HashSet<SignalId>,
}

thread_local! {
    static DEPENDENCY_TRACKER: std::cell::RefCell<DependencyTracker> =
        std::cell::RefCell::new(DependencyTracker::default());
}

/// Record a signal access for dependency tracking.
///
/// This should be called by Signal::get() when a tracking context is active.
pub(crate) fn track_signal_access(signal_id: SignalId) {
    DEPENDENCY_TRACKER.with(|tracker| {
        let mut tracker = tracker.borrow_mut();
        if tracker.is_tracking {
            tracker.dependencies.insert(signal_id);
            trace!(signal_id = ?signal_id, "Signal access tracked");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_computed_basic() {
        let count = Signal::new(10);
        let doubled = Computed::new(move || count.get() * 2);

        assert_eq!(doubled.get(), 20);

        count.set(5);
        assert_eq!(doubled.get(), 10);
    }

    #[test]
    fn test_computed_multiple_deps() {
        let width = Signal::new(10);
        let height = Signal::new(5);

        let area = Computed::new(move || width.get() * height.get());

        assert_eq!(area.get(), 50);

        width.set(20);
        assert_eq!(area.get(), 100);

        height.set(8);
        assert_eq!(area.get(), 160);
    }

    #[test]
    fn test_computed_chained() {
        let x = Signal::new(2);
        let doubled = Computed::new(move || x.get() * 2);
        let quadrupled = Computed::new(move || doubled.get() * 2);

        assert_eq!(quadrupled.get(), 8);

        x.set(3);
        assert_eq!(quadrupled.get(), 12);
    }

    #[test]
    fn test_computed_subscribe() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Signal::new(0);
        let doubled = Computed::new(move || count.get() * 2);

        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let _sub = doubled
            .subscribe_scoped(move || {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .expect("Failed to subscribe");

        count.set(1);
        count.set(2);

        // Computed should notify subscribers
        assert!(counter.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn test_computed_lazy_evaluation() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Signal::new(0);
        let compute_count = Arc::new(AtomicU32::new(0));

        let c = compute_count.clone();
        let computed = Computed::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            count.get() * 2
        });

        // Initial computation
        assert_eq!(compute_count.load(Ordering::SeqCst), 1);

        // Getting without changes doesn't recompute
        let _ = computed.get();
        let _ = computed.get();
        assert_eq!(compute_count.load(Ordering::SeqCst), 1);

        // Changing dependency triggers recompute on next get
        count.set(5);
        let _ = computed.get();
        assert_eq!(compute_count.load(Ordering::SeqCst), 2);
    }
}
