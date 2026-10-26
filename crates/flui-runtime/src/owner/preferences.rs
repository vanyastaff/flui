//! Accepted host observations, separate from native refresh and delivery work.

use std::{rc::Rc, sync::Arc};

use flui_platform::SystemPreferences;

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

#[cfg(test)]
mod tests {
    use std::{rc::Rc, sync::Arc};

    use flui_foundation::{PresentationAddress, UiRuntimeId, geometry::WindowMetrics};
    use flui_platform::SystemPreferences;

    use super::SystemPreferencesSnapshot;
    use crate::owner::{
        DispatchError, InitializationOutcome, InstallInitialization, InstallToken, OwnerEffects,
        OwnerHost, RecoveryState,
    };
    use crate::ui_runtime::UiRuntime;

    /// A host with no native side, to which nothing is ever installed or drawn.
    struct Effects;

    impl OwnerEffects for Effects {
        fn runtime_lifecycle(&self, _: UiRuntimeId, _: flui_scheduler::AppLifecycleState) {}
        fn runtimes_stopped(&self, _: RecoveryState) {}
        fn retire_host(&self, _: &[PresentationAddress], _: RecoveryState) {}
        fn commit_install(
            &self,
            _: InstallToken,
            _: RecoveryState,
        ) -> Option<InstallInitialization> {
            unreachable!("no install is pending")
        }
        fn finish_install(
            &self,
            _: PresentationAddress,
            _: InitializationOutcome,
            _: RecoveryState,
        ) {
        }
        fn cancel_install(&self, _: InstallToken, _: RecoveryState) {
            unreachable!("no install is pending")
        }
        fn resize_surface(&self, _: PresentationAddress, _: WindowMetrics) {}
        fn frame(&self, _: PresentationAddress, _: &mut UiRuntime) {}
        fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
        fn after_turn(&self, _: RecoveryState) {}
        fn request_continuation(&self) -> bool {
            true
        }
    }

    fn text_scale(scale: f64) -> SystemPreferences {
        SystemPreferences::default()
            .with_text_scale(scale)
            .expect("valid scale")
    }

    /// The last revision is a usable revision; the update after it is refused for
    /// good, and the refusal accepts and queues nothing, however often it is
    /// offered other values.
    fn an_exhausted_revision_refuses_without_changing_or_queueing() {
        let owner = OwnerHost::new();
        let _ = owner
            .publication(owner.prepare_runtime(UiRuntime::for_test()))
            .expect("publish the recipient")
            .commit();
        // No sequence of updates reaches the last revision, so the accepted
        // snapshot is placed one short of it.
        {
            let mut state = owner.core.state.borrow_mut();
            let origin = Rc::clone(&state.preferences.origin);
            state.preferences.current = Some(SystemPreferencesSnapshot {
                origin,
                revision: u64::MAX - 1,
                values: Arc::new(text_scale(1.5)),
            });
        }
        assert!(
            owner.update_preferences(text_scale(2.0), &Effects).is_ok(),
            "the last revision is still issued"
        );
        let accepted = owner
            .preferences()
            .expect("live host")
            .expect("an accepted snapshot");
        assert_eq!(accepted.revision, u64::MAX);
        for offered in [text_scale(3.0), SystemPreferences::default()] {
            assert_eq!(
                owner.update_preferences(offered, &Effects),
                Err(DispatchError::PreferenceRevisionExhausted),
                "a revision past the last must be refused, never reissued or saturated"
            );
            let kept = owner
                .preferences()
                .expect("live host")
                .expect("an accepted snapshot");
            assert_eq!(
                (kept.revision, &*kept.values),
                (u64::MAX, &*accepted.values),
                "a refused update replaced the accepted snapshot"
            );
            assert!(
                owner.core.state.borrow().queue.is_empty(),
                "a refused update queued delivery"
            );
        }
    }

    #[test]
    fn preference_revisions() {
        crate::table_test::run_table(
            "preference_revisions",
            &[(
                "an_exhausted_revision_refuses_without_changing_or_queueing",
                an_exhausted_revision_refuses_without_changing_or_queueing as fn(),
            )],
        );
    }
}
