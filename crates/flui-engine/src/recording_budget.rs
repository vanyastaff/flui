//! Bounds requested DrawSegment arena capacity and live element count only.
//! Allocator slack, DrawItem metadata, caches, and tessellator output are excluded.
use crate::command_ir::RecordError;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub(crate) struct RecordingBudget {
    state: Mutex<State>,
    bytes: usize,
    elements: usize,
}
#[derive(Debug, Default)]
struct State {
    bytes: usize,
    elements: usize,
    error: Option<RecordError>,
}
impl RecordingBudget {
    pub(crate) fn new(bytes: usize, elements: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            bytes,
            elements,
        })
    }
    pub(crate) fn default_frame() -> Arc<Self> {
        Self::new(128 * 1024 * 1024, 1_000_000)
    }
    pub(crate) fn error(&self) -> Option<RecordError> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .error
            .clone()
    }
    fn charge(&self, bytes: usize, elements: usize) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.error.is_some() {
            return false;
        }
        let next_bytes = state.bytes.checked_add(bytes);
        let next_elements = state.elements.checked_add(elements);
        let error = if next_bytes.is_none_or(|n| n > self.bytes) {
            Some(RecordError::Limit {
                resource: "recorded arena requested capacity bytes",
                requested: next_bytes.unwrap_or(usize::MAX),
                limit: self.bytes,
            })
        } else if next_elements.is_none_or(|n| n > self.elements) {
            Some(RecordError::Limit {
                resource: "recorded arena live elements",
                requested: next_elements.unwrap_or(usize::MAX),
                limit: self.elements,
            })
        } else {
            None
        };
        if error.is_some() {
            state.error = error;
            return false;
        }
        state.bytes = next_bytes.expect("BUG: checked recording byte addition");
        state.elements = next_elements.expect("BUG: checked recording count addition");
        true
    }
    fn available_bytes(&self) -> usize {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.bytes.saturating_sub(state.bytes)
    }
    fn release(&self, bytes: usize, elements: usize) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.bytes = state.bytes.saturating_sub(bytes);
        state.elements = state.elements.saturating_sub(elements);
    }
    fn allocation_failed(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.error.get_or_insert(RecordError::Limit {
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
        for value in values {
            if self.budget.as_ref().is_some_and(|b| b.error().is_some()) {
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
