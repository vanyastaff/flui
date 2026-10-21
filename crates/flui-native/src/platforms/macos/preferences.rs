//! Public AppKit observations sampled on the application owner lane.
//!
//! Three notifications only wake the owner, which samples every value again:
//! the workspace's accessibility display options (reduce motion, increase
//! contrast), the current locale (preferred languages), and the application
//! becoming active. AppKit posts nothing when the double-click interval
//! changes. The user changes it in System Settings, another application, so
//! the interval is read again when this one is active again; a change written
//! while it stays active (`defaults write`) waits for the next activation. A
//! failed read schedules one wake for its retry; nothing polls.

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use flui_platform::{GesturePreferences, MotionPreference, SystemPreferences};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplicationDidBecomeActiveNotification, NSEvent, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{NSCurrentLocaleDidChangeNotification, NSLocale, NSNotificationCenter};

use super::observation::Observation;
use crate::{
    PlatformError,
    shared::{
        owner_signal::OwnerSignal, panic_boundary::contain_owner_callback,
        preference_read::ReadAdmission, preferred_locales,
    },
};

/// The macOS host's preferences: what AppKit reports, sampled on the owner lane.
pub struct PreferenceSource {
    signal: Weak<OwnerSignal>,
    admission: ReadAdmission,
}

impl std::fmt::Debug for PreferenceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferenceSource").finish_non_exhaustive()
    }
}

/// The notifications that invalidate a [`PreferenceSource`]. Owned on the main
/// thread beside the source and removed when dropped.
pub(in crate::platforms) struct PreferenceObservers {
    _observations: [Observation; 3],
}

impl PreferenceSource {
    pub(in crate::platforms) fn new(signal: &Arc<OwnerSignal>) -> (Arc<Self>, PreferenceObservers) {
        let source = Arc::new(Self {
            signal: Arc::downgrade(signal),
            admission: ReadAdmission::default(),
        });
        // Registration precedes the first getter: a change during the cold
        // read still wakes the owner, and a failed or contained read leaves
        // the observers in place for the next one.
        let observers = PreferenceObservers::new(signal);
        contain_owner_callback(|| {
            if let Err(error) = source.read() {
                tracing::warn!(%error, "initial AppKit preference query failed");
            }
        });
        (source, observers)
    }

    pub(in crate::platforms) fn read(&self) -> Result<SystemPreferences, PlatformError> {
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
        // A failed or unwound read owes a retry, and nothing else may wake the
        // owner for it: schedule that wake unless the read settled.
        let mut retry = RetryWake {
            source: self,
            owed: true,
        };
        let read = self
            .admission
            .read_observation(web_time::Instant::now(), sample);
        retry.owed = matches!(
            read,
            Err(ref error) if !matches!(error, PlatformError::PreferencesDeferred)
        );
        drop(retry);
        let (current, needs_delivery) = read?;
        if needs_delivery && let Some(signal) = self.signal.upgrade() {
            // Publication precedes admission. OwnerSignal retains delivery
            // debt when the hook has not been installed yet.
            let _ = signal.wake();
        }
        Ok(current)
    }
}

/// Wakes the owner when the read a failure postponed is due, unless the read
/// settled first; runs on unwind too.
struct RetryWake<'a> {
    source: &'a PreferenceSource,
    owed: bool,
}

impl Drop for RetryWake<'_> {
    fn drop(&mut self) {
        let Some(deadline) = self
            .owed
            .then(|| self.source.admission.retry_deadline())
            .flatten()
        else {
            return;
        };
        let signal = self.source.signal.clone();
        let delay = deadline.saturating_duration_since(web_time::Instant::now());
        super::owner_lane::owner_queue().exec_after(delay, move || {
            if let Some(signal) = signal.upgrade() {
                let _ = signal.wake();
            }
        });
    }
}

impl PreferenceObservers {
    fn new(signal: &Arc<OwnerSignal>) -> Self {
        let wake = || {
            let signal = Arc::downgrade(signal);
            move || {
                if let Some(signal) = signal.upgrade() {
                    let _ = signal.wake();
                }
            }
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the names are AppKit's and Foundation's immutable
        // notification-name constants.
        let observations = unsafe {
            [
                Observation::new(
                    workspace,
                    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
                    wake(),
                ),
                Observation::new(
                    center.clone(),
                    NSApplicationDidBecomeActiveNotification,
                    wake(),
                ),
                Observation::new(center, NSCurrentLocaleDidChangeNotification, wake()),
            ]
        };
        Self {
            _observations: observations,
        }
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
    let workspace = NSWorkspace::sharedWorkspace();
    let motion = if workspace.accessibilityDisplayShouldReduceMotion() {
        MotionPreference::Reduce
    } else {
        MotionPreference::NoPreference
    };
    let preferences = SystemPreferences::default()
        .with_gestures(gestures)
        .with_motion(motion)
        .with_high_contrast(workspace.accessibilityDisplayShouldIncreaseContrast());
    let languages = NSLocale::preferredLanguages();
    Ok(
        match preferred_locales::from_language_tags(
            languages.iter().map(|language| language.to_string()),
        ) {
            Some(locales) => preferences.with_locales(locales),
            None => preferences,
        },
    )
}
