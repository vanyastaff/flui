//! Native sampling admission shared by the bounded owner samplers.

use crate::PlatformError;
use parking_lot::Mutex;
use std::time::Duration;

const RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct ReadAdmission {
    state: Mutex<State>,
    accepted: Mutex<Option<crate::SystemPreferences>>,
}

#[derive(Clone, Copy, Default)]
enum State {
    #[default]
    Ready,
    Reading,
    RetryAt(web_time::Instant),
}

impl ReadAdmission {
    /// Read and commit a native observation before outgoing notification. A
    /// cold source has no accepted value; errors never manufacture one or return
    /// an older accepted value as a successful refresh. The returned flag
    /// requests owner delivery after either a changed observation or a recovered
    /// read obligation; healthy unchanged reads remain quiet.
    pub(crate) fn read_observation(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<crate::SystemPreferences, PlatformError>,
    ) -> Result<(crate::SystemPreferences, bool), PlatformError> {
        let attempt = self.begin(now)?;
        let recovering = attempt.recovering;
        let current = sample()?;
        let changed = {
            let mut accepted = self.accepted.lock();
            let changed = accepted.as_ref() != Some(&current);
            *accepted = Some(current.clone());
            changed
        };
        attempt.accept();
        Ok((current, changed || recovering))
    }

    pub(crate) fn begin(&self, now: web_time::Instant) -> Result<ReadAttempt<'_>, PlatformError> {
        let mut state = self.state.lock();
        match *state {
            State::Reading => return Err(PlatformError::PreferencesDeferred),
            State::RetryAt(deadline) if now < deadline => {
                return Err(PlatformError::PreferencesDeferred);
            }
            State::Ready | State::RetryAt(_) => {}
        }
        let recovering = matches!(*state, State::RetryAt(_));
        *state = State::Reading;
        Ok(ReadAttempt {
            admission: self,
            now,
            accepted: false,
            recovering,
        })
    }
}

pub(crate) struct ReadAttempt<'a> {
    admission: &'a ReadAdmission,
    now: web_time::Instant,
    accepted: bool,
    recovering: bool,
}

impl ReadAttempt<'_> {
    /// Commit the successful read before invoking any outgoing wake or callback.
    pub(crate) fn accept(mut self) {
        self.accepted = true;
    }
}

impl Drop for ReadAttempt<'_> {
    fn drop(&mut self) {
        let mut state = self.admission.state.lock();
        *state = if self.accepted {
            State::Ready
        } else {
            State::RetryAt(self.now + RETRY_INTERVAL)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_native_observation_recovers_without_forged_defaults() {
        let now = web_time::Instant::now();
        let cold = ReadAdmission::default();
        let (_, needs_delivery) = cold
            .read_observation(now, || Ok(crate::SystemPreferences::default()))
            .expect("first legitimate unknown observation");
        assert!(
            needs_delivery,
            "an unknown observation is accepted only after the first successful query"
        );
        let reader = ReadAdmission::default();
        let failed = reader.read_observation(now, || {
            Err(PlatformError::Preferences {
                message: "initial getter failure".into(),
            })
        });
        assert!(matches!(failed, Err(PlatformError::Preferences { .. })));
        assert!(matches!(
            reader.read_observation(now, || panic!("early native retry")),
            Err(PlatformError::PreferencesDeferred)
        ));
        let observation = crate::SystemPreferences::default().with_gestures(
            crate::GesturePreferences::default()
                .with_double_click_interval(Duration::from_millis(700)),
        );
        let (accepted, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL, || Ok(observation.clone()))
            .expect("first accepted native observation");
        assert_eq!(accepted, observation);
        assert!(
            needs_delivery,
            "cold source must notify its first accepted observation"
        );
        let failed = reader.read_observation(now + RETRY_INTERVAL, || {
            Err(PlatformError::Preferences {
                message: "later getter failure".into(),
            })
        });
        assert!(
            matches!(failed, Err(PlatformError::Preferences { .. })),
            "failure cannot publish a stale success"
        );
        let (accepted, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL * 2, || Ok(observation.clone()))
            .expect("recovered native observation");
        assert_eq!(accepted, observation);
        assert!(
            needs_delivery,
            "successful recovery must retry owner delivery even for an unchanged observation"
        );
        let (_, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL * 2, || Ok(observation))
            .expect("healthy unchanged observation");
        assert!(
            !needs_delivery,
            "healthy unchanged observations remain quiet"
        );
    }

    #[test]
    fn bounded_native_read_admission() {
        let admission = ReadAdmission::default();
        let now = web_time::Instant::now();
        let failed = admission.begin(now).expect("first admission");
        assert!(matches!(
            admission.begin(now),
            Err(PlatformError::PreferencesDeferred)
        ));
        drop(failed);
        for elapsed in [0, 10, 100, 499] {
            assert!(matches!(
                admission.begin(now + Duration::from_millis(elapsed)),
                Err(PlatformError::PreferencesDeferred)
            ));
        }
        admission
            .begin(now + RETRY_INTERVAL)
            .expect("bounded retry")
            .accept();
        admission
            .begin(now + RETRY_INTERVAL)
            .expect("healthy reads remain admitted")
            .accept();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _attempt = admission
                .begin(now + RETRY_INTERVAL)
                .expect("unwind admission");
            panic!("native sampling panic");
        }));
        assert!(failure.is_err());
        assert!(matches!(
            admission.begin(now + RETRY_INTERVAL),
            Err(PlatformError::PreferencesDeferred)
        ));
        admission
            .begin(now + RETRY_INTERVAL * 2)
            .expect("recovery after unwind")
            .accept();
    }
}
