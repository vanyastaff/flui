use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use super::{DispatchError, OwnerCore, OwnerEffects, OwnerHost, finish_retirement};

const CALLBACK_BUDGET: usize = 32;

/// One physical host callback. Nested scopes share its delivery budget.
#[must_use = "keep the scope alive until the native callback finishes"]
pub struct OwnerCallback<'a> {
    core: Rc<OwnerCore>,
    effects: &'a dyn OwnerEffects,
    resumes_carried_work: bool,
}

impl std::fmt::Debug for OwnerCallback<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerCallback")
            .field("resumes_carried_work", &self.resumes_carried_work)
            .finish_non_exhaustive()
    }
}

impl OwnerCallback<'_> {
    /// Whether this outer callback resumes already accepted work.
    #[must_use]
    pub fn resumes_carried_work(&self) -> bool {
        self.resumes_carried_work
    }
}

impl OwnerHost {
    /// Group native roots and nested callbacks into one finite opportunity.
    ///
    /// # Errors
    /// An active publication prevents starting a callback.
    pub fn begin_callback<'a>(
        &self,
        effects: &'a dyn OwnerEffects,
    ) -> Result<OwnerCallback<'a>, DispatchError> {
        self.core.begin_callback(effects)
    }
}

impl OwnerCore {
    pub(super) fn begin_callback<'a>(
        self: &Rc<Self>,
        effects: &'a dyn OwnerEffects,
    ) -> Result<OwnerCallback<'a>, DispatchError> {
        let resumes_carried_work = {
            let mut state = self
                .state
                .try_borrow_mut()
                .map_err(|_| DispatchError::Busy)?;
            let outermost = state.callback_depth == 0;
            let resumes = outermost && !state.queue.is_empty();
            if outermost {
                state.callback_remaining = CALLBACK_BUDGET;
                state.continuation = None;
            }
            state.callback_depth = state
                .callback_depth
                .checked_add(1)
                .expect("BUG: callback nesting exhausted");
            resumes
        };
        Ok(OwnerCallback {
            core: Rc::clone(self),
            effects,
            resumes_carried_work,
        })
    }
}

impl Drop for OwnerCallback<'_> {
    fn drop(&mut self) {
        {
            let mut state = self.core.state.borrow_mut();
            if state.callback_depth > 1 {
                state.callback_depth -= 1;
                return;
            }
        }
        // Keep the physical callback active through completion and its wake:
        // synchronous reentry must not acquire a fresh delivery budget.
        let _reset = CallbackReset(Rc::clone(&self.core));
        let mut first_failure = if std::thread::panicking() {
            self.core.state.borrow_mut().callback_remaining = 0;
            None
        } else {
            catch_unwind(AssertUnwindSafe(|| self.core.continue_work(self.effects))).err()
        };
        self.core
            .post_continuation(self.effects, &mut first_failure);
        finish_retirement(first_failure);
    }
}

struct CallbackReset(Rc<OwnerCore>);

impl Drop for CallbackReset {
    fn drop(&mut self) {
        let mut state = self.0.state.borrow_mut();
        state.callback_depth = 0;
        state.callback_remaining = 0;
    }
}
