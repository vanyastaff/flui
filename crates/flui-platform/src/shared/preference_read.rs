//! Native sampling admission shared by the bounded owner samplers.

use crate::PlatformError;
use parking_lot::Mutex;
use std::time::Duration;

const RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct ReadAdmission {
    state: Mutex<State>,
}

#[derive(Clone, Copy, Default)]
enum State {
    #[default]
    Ready,
    Reading,
    RetryAt(web_time::Instant),
}

impl ReadAdmission {
    pub(crate) fn begin(&self, now: web_time::Instant) -> Result<ReadAttempt<'_>, PlatformError> {
        let mut state = self.state.lock();
        match *state {
            State::Reading => return Err(PlatformError::PreferencesDeferred),
            State::RetryAt(deadline) if now < deadline => {
                return Err(PlatformError::PreferencesDeferred);
            }
            State::Ready | State::RetryAt(_) => {}
        }
        *state = State::Reading;
        Ok(ReadAttempt {
            admission: self,
            now,
            accepted: false,
        })
    }
}

pub(crate) struct ReadAttempt<'a> {
    admission: &'a ReadAdmission,
    now: web_time::Instant,
    accepted: bool,
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
