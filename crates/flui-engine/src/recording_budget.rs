//! Bounds requested DrawSegment arena capacity and live element count only.
//! Allocator slack, DrawItem metadata, caches, and tessellator output are excluded.
use crate::command_ir::RecordError;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

// Recording has one mutator per painter. Immutable segments may cross threads,
// so atomic counters preserve Send without a lock in draw admission. The sticky
// diagnostic is initialized only on rejection, never on the successful hot path.
#[derive(Debug)]
pub(crate) struct RecordingBudget {
    used_bytes: AtomicUsize,
    used_elements: AtomicUsize,
    clip_work: AtomicUsize,
    effect_work: AtomicUsize,
    error: OnceLock<RecordError>,
    bytes: usize,
    elements: usize,
}
impl RecordingBudget {
    pub(crate) fn new(bytes: usize, elements: usize) -> Arc<Self> {
        Arc::new(Self {
            used_bytes: AtomicUsize::new(0),
            used_elements: AtomicUsize::new(0),
            clip_work: AtomicUsize::new(0),
            effect_work: AtomicUsize::new(0),
            error: OnceLock::new(),
            bytes,
            elements,
        })
    }
    pub(crate) fn default_frame() -> Arc<Self> {
        Self::new(128 * 1024 * 1024, 1_000_000)
    }
    pub(crate) fn error(&self) -> Option<RecordError> {
        self.error.get().cloned()
    }
    pub(crate) fn record_error(&self, error: RecordError) {
        let _ = self.error.set(error);
    }
    pub(crate) fn charge(&self, bytes: usize, elements: usize) -> bool {
        if self.error.get().is_some() {
            return false;
        }
        if let Err(used) =
            self.used_bytes
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(bytes).filter(|next| *next <= self.bytes)
                })
        {
            let _ = self.error.set(RecordError::Limit {
                resource: "recorded arena requested capacity bytes",
                requested: used.saturating_add(bytes),
                limit: self.bytes,
            });
            return false;
        }
        if let Err(used) =
            self.used_elements
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(elements)
                        .filter(|next| *next <= self.elements)
                })
        {
            self.used_bytes.fetch_sub(bytes, Ordering::Relaxed);
            let _ = self.error.set(RecordError::Limit {
                resource: "recorded arena live elements",
                requested: used.saturating_add(elements),
                limit: self.elements,
            });
            return false;
        }
        true
    }
    /// Work is cumulative for a frame: restore and dropping masks do not refund it.
    pub(crate) fn admit_clip_work(&self, work: usize) -> crate::error::EngineResult<()> {
        const LIMIT: usize = 1_000_000_000;
        self.clip_work
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(work).filter(|next| *next <= LIMIT)
            })
            .map(|_| ())
            .map_err(|used| crate::error::EngineError::PreparedResourceLimit {
                resource: "cumulative clip membership work",
                requested: used.saturating_add(work),
                limit: LIMIT,
            })
    }
    /// Bound cumulative texture sampling work before encoding filter passes.
    pub(crate) fn admit_effect_work(&self, work: usize) -> crate::error::EngineResult<()> {
        const LIMIT: usize = 1_000_000_000;
        self.effect_work
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(work).filter(|next| *next <= LIMIT)
            })
            .map(|_| ())
            .map_err(|used| crate::error::EngineError::PreparedResourceLimit {
                resource: "cumulative effect sampling work",
                requested: used.saturating_add(work),
                limit: LIMIT,
            })
    }
    fn available_bytes(&self) -> usize {
        self.bytes
            .saturating_sub(self.used_bytes.load(Ordering::Relaxed))
    }
    pub(crate) fn release(&self, bytes: usize, elements: usize) {
        self.used_bytes.fetch_sub(bytes, Ordering::Relaxed);
        self.used_elements.fetch_sub(elements, Ordering::Relaxed);
    }
    fn allocation_failed(&self) {
        let _ = self.error.set(RecordError::Limit {
            resource: "recorded arena allocation",
            requested: usize::MAX,
            limit: self.bytes,
        });
    }
}

#[derive(Debug)]
pub(crate) struct BudgetVec<T> {
    values: Vec<T>,
    budget: Option<Arc<RecordingBudget>>,
    charged_bytes: usize,
    charged_elements: usize,
}
impl<T> BudgetVec<T> {
    pub(crate) fn new() -> Self {
        Self {
            values: Vec::new(),
            budget: None,
            charged_bytes: 0,
            charged_elements: 0,
        }
    }
    pub(crate) fn with_budget(budget: &Arc<RecordingBudget>) -> Self {
        Self {
            budget: Some(Arc::clone(budget)),
            values: Vec::new(),
            charged_bytes: 0,
            charged_elements: 0,
        }
    }
    pub(crate) fn unbounded_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
            budget: None,
            charged_bytes: 0,
            charged_elements: 0,
        }
    }
    fn admit(&mut self, count: usize) -> bool {
        let Some(budget) = &self.budget else {
            return true;
        };
        let Some(new_len) = self.values.len().checked_add(count) else {
            budget.allocation_failed();
            return false;
        };
        // Geometric growth avoids reallocating/copying on every append. Clamp the
        // requested capacity to the currently available shared payload allowance.
        let capacity = self.values.capacity();
        let element_size = std::mem::size_of::<T>();
        let desired = capacity.saturating_mul(2).max(new_len);
        let available_elements = if new_len <= capacity || element_size == 0 {
            usize::MAX
        } else {
            budget.available_bytes() / element_size
        };
        let affordable = capacity.saturating_add(available_elements);
        let target = if new_len > capacity {
            desired.min(affordable).max(new_len)
        } else {
            capacity
        };
        let extra = target.saturating_sub(capacity);
        let Some(bytes) = extra.checked_mul(element_size) else {
            budget.allocation_failed();
            return false;
        };
        if !budget.charge(bytes, count) {
            return false;
        }
        if extra > 0
            && self
                .values
                .try_reserve_exact(target - self.values.len())
                .is_err()
        {
            budget.release(bytes, count);
            budget.allocation_failed();
            return false;
        }
        self.charged_bytes += bytes;
        self.charged_elements += count;
        true
    }
    pub(crate) fn push(&mut self, value: T) {
        if self.admit(1) {
            self.values.push(value);
        }
    }
    pub(crate) fn extend_from_slice(&mut self, values: &[T])
    where
        T: Clone,
    {
        if self.admit(values.len()) {
            self.values.extend_from_slice(values);
        }
    }
    pub(crate) fn failed(&self) -> bool {
        self.budget.as_ref().is_some_and(|b| b.error().is_some())
    }
    pub(crate) fn clear(&mut self) {
        self.values.clear();
        if let Some(budget) = &self.budget {
            budget.release(0, self.charged_elements);
        }
        self.charged_elements = 0;
    }
}
impl<T> Default for BudgetVec<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Clone> Clone for BudgetVec<T> {
    fn clone(&self) -> Self {
        let mut cloned = self
            .budget
            .as_ref()
            .map_or_else(Self::new, Self::with_budget);
        cloned.extend_from_slice(&self.values);
        cloned
    }
}
impl<T> Extend<T> for BudgetVec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, values: I) {
        let mut values = values.into_iter();
        let (lower, upper) = values.size_hint();
        if upper == Some(lower) {
            if !self.admit(lower) {
                return;
            }
            // Size hints are not a trusted length contract: cap the admitted
            // batch and account any unexpected trailing elements individually.
            self.values.extend(values.by_ref().take(lower));
        }
        // Unknown-length sources remain bounded; exact-size tessellation/index
        // iterators above charge and reserve once for their whole batch.
        for value in values {
            if self.failed() {
                break;
            }
            self.push(value);
        }
    }
}
impl<T> std::ops::Deref for BudgetVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for BudgetVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
impl<'a, T> IntoIterator for &'a BudgetVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut BudgetVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter_mut()
    }
}
impl<T> Drop for BudgetVec<T> {
    fn drop(&mut self) {
        if (self.charged_bytes != 0 || self.charged_elements != 0)
            && let Some(budget) = &self.budget
        {
            budget.release(self.charged_bytes, self.charged_elements);
        }
    }
}
