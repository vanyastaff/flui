//! Accepted host observations, separate from native refresh and delivery work.

use std::{rc::Rc, sync::Arc};

use flui_platform_api::SystemPreferences;

use super::runtime_dispatch::RuntimeWork;
use super::{Delivery, DeliveryClaim, DispatchError, OwnerEffects, OwnerHost, OwnerWork};

/// An immutable preference revision minted by one host. Pass it into that
/// host's runtime services before mounting the first root.
#[derive(Clone, Debug)]
pub struct SystemPreferencesSnapshot {
    pub(crate) origin: Rc<()>,
    pub(crate) revision: u64,
    pub(crate) values: Arc<SystemPreferences>,
}

#[derive(Default)]
pub(super) struct PreferenceState {
    pub(super) origin: Rc<()>,
    pub(super) current: Option<SystemPreferencesSnapshot>,
}

impl OwnerHost {
    /// Latest accepted observations for a runtime being constructed by this host.
    ///
    /// # Errors
    /// Refuses a closed host or an active publication.
    pub fn preferences(&self) -> Result<Option<SystemPreferencesSnapshot>, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        if state.closed {
            return Err(DispatchError::Closed);
        }
        Ok(state.preferences.current.clone())
    }

    /// Commit observations and admit delivery to every current runtime together.
    /// Native read failures must not call this with fallback or empty values.
    ///
    /// # Errors
    /// Refuses a closed/busy host or permanent revision exhaustion. All recipient
    /// deliveries are queued before any callback or wake can run.
    pub fn update_preferences(
        &self,
        values: SystemPreferences,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let _callback = self.core.begin_callback(effects)?;
        let starts = {
            let mut state = self
                .core
                .state
                .try_borrow_mut()
                .map_err(|_| DispatchError::Busy)?;
            if state.closed {
                return Err(DispatchError::Closed);
            }
            if state
                .preferences
                .current
                .as_ref()
                .is_some_and(|current| *current.values == values)
            {
                return Ok(Delivery::Queued);
            }
            let revision = state
                .preferences
                .current
                .as_ref()
                .map_or(0, |current| current.revision)
                .checked_add(1)
                .ok_or(DispatchError::PreferenceRevisionExhausted)?;
            let snapshot = SystemPreferencesSnapshot {
                origin: Rc::clone(&state.preferences.origin),
                revision,
                values: Arc::new(values),
            };
            let recipients: Vec<_> = state.runtimes.iter().map(|runtime| runtime.id).collect();
            state.preferences.current = Some(snapshot.clone());
            for id in recipients {
                state
                    .queue
                    .push_back(OwnerWork::Runtime(RuntimeWork::Preferences(
                        id,
                        snapshot.clone(),
                    )));
            }
            if !state.queue.is_empty()
                && state.claim == DeliveryClaim::Idle
                && state.callback_remaining != 0
            {
                state.claim = DeliveryClaim::Executing;
                true
            } else {
                false
            }
        };
        if starts {
            self.core.drive(effects);
        }
        Ok(if starts {
            Delivery::Driven
        } else {
            Delivery::Queued
        })
    }
}
