//! Public AppKit observations sampled on the application owner lane.
//!
//! AppKit exposes no documented notification for an external double-click
//! setting change. A bounded owner-lane sampler therefore invalidates the
//! existing host subscription; it does not install a second subscription.
//! An active owner attempts sampling every 500ms, including with zero windows;
//! a blocked owner can delay that attempt. Retirement makes queued work inert.

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use objc2::MainThreadMarker;
use objc2_app_kit::NSEvent;
use parking_lot::Mutex;

use crate::{
    GesturePreferences, PlatformError, SystemPreferences,
    shared::{owner_signal::OwnerSignal, panic_boundary::contain_owner_callback},
};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

pub(super) struct PreferenceSource {
    accepted: Mutex<SystemPreferences>,
    signal: Weak<OwnerSignal>,
    admission: crate::shared::preference_read::ReadAdmission,
}

impl PreferenceSource {
    pub(super) fn new(signal: &Arc<OwnerSignal>) -> Result<Arc<Self>, PlatformError> {
        let source = Arc::new(Self {
            accepted: Mutex::new(sample()?),
            signal: Arc::downgrade(signal),
            admission: crate::shared::preference_read::ReadAdmission::default(),
        });
        Self::schedule(&source);
        Ok(source)
    }

    pub(super) fn read(&self) -> Result<SystemPreferences, PlatformError> {
        if MainThreadMarker::new().is_none() {
            return Err(PlatformError::Preferences {
                message: "AppKit preferences require the application owner thread".into(),
            });
        }
        if !self
            .signal
            .upgrade()
            .is_some_and(|signal| signal.accepting())
        {
            return Err(PlatformError::Preferences {
                message: "the AppKit preference owner has stopped".into(),
            });
        }
        let attempt = self.admission.begin(web_time::Instant::now())?;
        let current = sample()?;
        let changed = {
            let mut accepted = self.accepted.lock();
            let changed = *accepted != current;
            *accepted = current.clone();
            changed
        };
        attempt.accept();
        if changed && let Some(signal) = self.signal.upgrade() {
            // Publication precedes admission. OwnerSignal retains delivery
            // debt when the hook has not been installed yet.
            let _ = signal.wake();
        }
        Ok(current)
    }

    fn schedule(source: &Arc<Self>) {
        let weak = Arc::downgrade(source);
        super::owner_lane::owner_queue().exec_after(SAMPLE_INTERVAL, move || {
            let Some(source) = weak.upgrade() else { return };
            let Some(signal) = source.signal.upgrade() else {
                return;
            };
            if !signal.accepting() {
                return;
            }
            // Rearm before entering diagnostics/native work. A failed query or
            // reentrant diagnostic panic cannot erase the next refresh attempt.
            Self::schedule(&source);
            contain_owner_callback(|| {
                if let Err(error) = source.read() {
                    tracing::warn!(%error, "AppKit preference query failed");
                }
            });
        });
    }
}

fn sample() -> Result<SystemPreferences, PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::Preferences {
            message: "AppKit preferences require the application owner thread".into(),
        });
    }
    let seconds = NSEvent::doubleClickInterval();
    let interval =
        Duration::try_from_secs_f64(seconds).map_err(|error| PlatformError::Preferences {
            message: error.to_string(),
        })?;
    let gestures = GesturePreferences::default().with_double_click_interval(interval);
    Ok(SystemPreferences::default().with_gestures(gestures))
}
