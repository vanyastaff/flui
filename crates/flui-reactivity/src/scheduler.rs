//! Async effect scheduling for batched updates.
//!
//! This module provides a scheduler that batches effect executions,
//! preventing redundant updates and improving performance.

use parking_lot::Mutex;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tracing::{debug, trace, warn};

/// Maximum number of pending effects before forcing a flush.
///
/// This prevents unbounded memory growth from accumulated effects.
/// If this limit is reached, effects are flushed early with a warning.
const MAX_PENDING_EFFECTS: usize = 10_000;

/// Unique identifier for a scheduled effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectId(u64);

impl EffectId {
    /// Create a new effect ID.
    ///
    /// # Panics
    ///
    /// Panics if u64::MAX effects have been created (practically impossible).
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
            .expect("EffectId counter overflow! Cannot create more effects.");

        Self(id)
    }

    /// Get the inner ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Default for EffectId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<u64> for EffectId {
    #[inline]
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<EffectId> for u64 {
    #[inline]
    fn from(id: EffectId) -> Self {
        id.0
    }
}

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Effect({})", self.0)
    }
}

/// Callback function for effects.
///
/// Uses Arc<Mutex<>> to allow safe sharing across threads and cloning for execution.
pub type EffectCallback = Arc<Mutex<Box<dyn FnMut() + Send + 'static>>>;

/// Effect with callback and metadata.
struct ScheduledEffect {
    id: EffectId,
    callback: EffectCallback,
    #[allow(dead_code)]
    priority: EffectPriority,
}

/// Priority level for effect execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EffectPriority {
    /// Low priority (e.g., logging, analytics)
    Low = 0,
    /// Normal priority (default)
    #[default]
    Normal = 1,
    /// High priority (e.g., UI updates)
    High = 2,
    /// Critical priority (e.g., error handlers)
    Critical = 3,
}

impl EffectPriority {
    /// Returns true if this priority is higher than or equal to the other.
    #[inline]
    pub const fn is_at_least(self, other: Self) -> bool {
        self as u8 >= other as u8
    }

    /// Returns the priority level as a number (0-3).
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl std::fmt::Display for EffectPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Normal => write!(f, "Normal"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

impl From<u8> for EffectPriority {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Low,
            1 => Self::Normal,
            2 => Self::High,
            _ => Self::Critical,
        }
    }
}

/// Scheduler state.
struct SchedulerState {
    /// Queue of pending effects (FIFO order within priority)
    queue: VecDeque<ScheduledEffect>,
    /// Set of queued effect IDs (for deduplication)
    queued: HashSet<EffectId>,
    /// Registered effects by ID
    effects: std::collections::HashMap<EffectId, EffectCallback>,
    /// Whether the scheduler is currently flushing
    is_flushing: bool,
}

/// Async effect scheduler for batched updates.
///
/// The scheduler batches effect executions to prevent redundant updates.
/// Effects are queued and executed in batches when `flush()` is called.
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::scheduler::{EffectScheduler, EffectPriority};
///
/// let scheduler = EffectScheduler::new();
///
/// // Register an effect
/// let effect_id = scheduler.register(
///     || println!("Effect executed"),
///     EffectPriority::Normal
/// );
///
/// // Schedule the effect
/// scheduler.schedule(effect_id);
///
/// // Flush all pending effects
/// scheduler.flush();
/// ```
#[derive(Clone)]
pub struct EffectScheduler {
    state: Arc<Mutex<SchedulerState>>,
}

impl EffectScheduler {
    /// Create a new effect scheduler.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SchedulerState {
                queue: VecDeque::new(),
                queued: HashSet::new(),
                effects: std::collections::HashMap::new(),
                is_flushing: false,
            })),
        }
    }

    /// Register an effect with the scheduler.
    ///
    /// Returns an EffectId that can be used to schedule the effect.
    pub fn register<F>(&self, callback: F, priority: EffectPriority) -> EffectId
    where
        F: FnMut() + Send + 'static,
    {
        let id = EffectId::new();
        let mut state = self.state.lock();

        // Wrap callback in Arc<Mutex<>> for thread-safe sharing
        let callback = Arc::new(Mutex::new(
            Box::new(callback) as Box<dyn FnMut() + Send + 'static>
        ));
        state.effects.insert(id, callback);

        debug!(effect_id = ?id, priority = ?priority, "Effect registered");
        id
    }

    /// Schedule an effect for execution.
    ///
    /// If the effect is already queued, this is a no-op (deduplication).
    pub fn schedule(&self, effect_id: EffectId) {
        let mut state = self.state.lock();

        // Skip if already queued (deduplication)
        if state.queued.contains(&effect_id) {
            trace!(effect_id = ?effect_id, "Effect already queued, skipping");
            return;
        }

        // Check if we've exceeded the limit (backpressure)
        if state.queue.len() >= MAX_PENDING_EFFECTS {
            warn!(
                "Pending effects exceeded limit ({}), flushing early to prevent memory accumulation",
                MAX_PENDING_EFFECTS
            );

            // Emergency flush: execute all pending effects immediately
            let effects = std::mem::take(&mut state.queue);
            let queued = std::mem::take(&mut state.queued);

            drop(state); // Release lock before executing

            debug!(count = effects.len(), "Emergency flush of pending effects");

            for effect in effects {
                let mut callback = effect.callback.lock();
                (*callback)();
                drop(callback); // Release lock immediately
            }

            // Re-acquire lock with a new variable name to prevent confusion
            let mut state_after_flush = self.state.lock();
            state_after_flush.queued = queued; // Restore queued set (now empty)
            state = state_after_flush; // Assign back to original variable for subsequent code
        }

        // Clone the effect callback instead of removing it
        // This prevents use-after-free if effect is unregistered during execution
        if let Some(callback) = state.effects.get(&effect_id).cloned() {
            let priority = EffectPriority::Normal; // Could be stored with effect
            state.queue.push_back(ScheduledEffect {
                id: effect_id,
                callback,
                priority,
            });
            state.queued.insert(effect_id);

            trace!(effect_id = ?effect_id, queue_size = state.queue.len(), "Effect scheduled");
        } else {
            debug!(effect_id = ?effect_id, "Attempted to schedule unknown effect");
        }
    }

    /// Unregister an effect from the scheduler.
    pub fn unregister(&self, effect_id: EffectId) {
        let mut state = self.state.lock();
        state.effects.remove(&effect_id);
        debug!(effect_id = ?effect_id, "Effect unregistered");
    }

    /// Flush all pending effects.
    ///
    /// Executes all queued effects in order. Effects scheduled during
    /// flush are queued for the next flush cycle.
    pub fn flush(&self) {
        let mut state = self.state.lock();

        if state.is_flushing {
            debug!("Already flushing, skipping nested flush");
            return;
        }

        state.is_flushing = true;
        let batch_size = state.queue.len();

        if batch_size == 0 {
            state.is_flushing = false;
            return;
        }

        debug!(batch_size, "Flushing effect batch");

        // Take all pending effects
        let mut effects = std::mem::take(&mut state.queue);
        state.queued.clear();

        // Release lock while executing effects
        drop(state);

        // Execute effects in order
        let mut executed = 0;
        while let Some(effect) = effects.pop_front() {
            trace!(effect_id = ?effect.id, "Executing effect");

            // Lock the callback and execute it
            // SAFETY: We cloned the Arc, so the callback stays alive even if unregistered
            let mut callback = effect.callback.lock();
            (*callback)();
            drop(callback); // Explicitly release lock

            executed += 1;
        }

        debug!(executed, "Effect batch completed");

        // Mark flush as complete
        let mut state = self.state.lock();
        state.is_flushing = false;
    }

    /// Check if there are pending effects.
    pub fn has_pending(&self) -> bool {
        let state = self.state.lock();
        !state.queue.is_empty()
    }

    /// Get the number of pending effects.
    pub fn pending_count(&self) -> usize {
        let state = self.state.lock();
        state.queue.len()
    }

    /// Clear all pending effects without executing them.
    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.queue.clear();
        state.queued.clear();
        debug!("Scheduler cleared");
    }
}

impl Default for EffectScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EffectScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("EffectScheduler")
            .field("pending_count", &state.queue.len())
            .field("registered_count", &state.effects.len())
            .field("is_flushing", &state.is_flushing)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_scheduler_creation() {
        let scheduler = EffectScheduler::new();
        assert_eq!(scheduler.pending_count(), 0);
        assert!(!scheduler.has_pending());
    }

    #[test]
    fn test_register_and_schedule() {
        let scheduler = EffectScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        let effect_id = scheduler.register(
            move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
            EffectPriority::Normal,
        );

        scheduler.schedule(effect_id);
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.flush();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_deduplication() {
        let scheduler = EffectScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        let effect_id = scheduler.register(
            move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
            EffectPriority::Normal,
        );

        // Schedule same effect multiple times
        scheduler.schedule(effect_id);
        scheduler.schedule(effect_id);
        scheduler.schedule(effect_id);

        // Should only queue once
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.flush();
        // Should only execute once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_batch_execution() {
        let scheduler = EffectScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let effects: Vec<_> = (0..5)
            .map(|_| {
                let c = counter.clone();
                scheduler.register(
                    move || {
                        c.fetch_add(1, Ordering::SeqCst);
                    },
                    EffectPriority::Normal,
                )
            })
            .collect();

        // Schedule all effects
        for effect_id in effects {
            scheduler.schedule(effect_id);
        }

        assert_eq!(scheduler.pending_count(), 5);

        scheduler.flush();
        assert_eq!(counter.load(Ordering::SeqCst), 5);
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_unregister() {
        let scheduler = EffectScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        let effect_id = scheduler.register(
            move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
            EffectPriority::Normal,
        );

        scheduler.unregister(effect_id);
        scheduler.schedule(effect_id); // Should be a no-op

        scheduler.flush();
        assert_eq!(counter.load(Ordering::SeqCst), 0); // Not executed
    }

    #[test]
    fn test_clear() {
        let scheduler = EffectScheduler::new();

        let effect_id = scheduler.register(|| {}, EffectPriority::Normal);
        scheduler.schedule(effect_id);

        assert_eq!(scheduler.pending_count(), 1);

        scheduler.clear();
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_re_schedule_after_flush() {
        let scheduler = EffectScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        let effect_id = scheduler.register(
            move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
            EffectPriority::Normal,
        );

        // First execution
        scheduler.schedule(effect_id);
        scheduler.flush();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Second execution
        scheduler.schedule(effect_id);
        scheduler.flush();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
