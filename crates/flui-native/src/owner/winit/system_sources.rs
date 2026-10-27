//! The operating system's own sources the winit loop starts, which winit
//! lacks: the system's preferences on every system, and on Linux the system
//! notices logind and the kernel report. The host's table registers them
//! ([`WinitHost`](super::WinitHost)); the loop starts each as it runs, once
//! its signal exists, and is the table's preference position.

use std::sync::Arc;

#[cfg(windows)]
use super::PreferenceObservation;
use super::{WinitApp, WinitLoop};
use crate::error::PlatformError;
#[cfg(target_os = "linux")]
use crate::executor::BackgroundExecutor;
use crate::host;
use crate::owner::signal::OwnerSignal;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::owner::sources::SystemPreferenceSource;
#[cfg(windows)]
use crate::owner::windows::control::OwnerPreferences;

/// The operating system's preference source as the winit loop starts it on
/// its owner thread, as it resumes.
pub(crate) trait PreferenceStart: Send + Sync {
    /// Windows' own source, which a broadcast receiver of the loop's tells of
    /// setting changes.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the system's settings cannot be
    /// observed.
    #[cfg(windows)]
    fn start(
        &self,
        signal: &Arc<OwnerSignal>,
    ) -> Result<std::rc::Rc<dyn OwnerPreferences>, PlatformError>;

    /// AppKit's source, and the observers that wake the owner when it
    /// changes; the loop ends the watch before the source goes.
    #[cfg(target_os = "macos")]
    fn start(
        &self,
        signal: &Arc<OwnerSignal>,
    ) -> (
        Arc<dyn SystemPreferenceSource + Send + Sync>,
        crate::owner::watch::Watch,
    );

    /// The Settings portal's source, observed on `executor`.
    #[cfg(target_os = "linux")]
    fn start(
        &self,
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn PortalPreferences>;
}

/// The sources of the Linux host's system notices as the winit loop starts
/// them, once the owner can deliver what they queue.
#[cfg(target_os = "linux")]
pub(crate) trait HostNoticeStart: Send + Sync {
    /// Start the sources on `executor`; they wake the owner behind `signal`
    /// when something is queued.
    fn start(
        &self,
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Box<dyn HostNotices>;
}

/// The Settings portal's preferences as the winit loop on Linux keeps them:
/// the snapshot, and the colour scheme a window without a theme of its own
/// follows.
#[cfg(target_os = "linux")]
pub(crate) trait PortalPreferences: SystemPreferenceSource + Send + Sync {
    /// The colour scheme the portal reports.
    fn appearance(&self) -> flui_platform::WindowAppearance;
}

/// The sources of the host's system notices the winit loop keeps on Linux.
#[cfg(target_os = "linux")]
pub(crate) trait HostNotices {
    /// Deliver what the sources queued since the loop's last turn, oldest
    /// first, to `platform`'s notice channel and session-end hook.
    fn deliver(&self, platform: &WinitLoop);
}

/// The preference position is the loop's: the source the table registered
/// ([`WinitHost::preferences`](super::WinitHost::preferences)), started as
/// the loop resumes and read on the live owner only. On Windows the owner
/// samples it on its turns and the position reads what was published.
impl host::Preferences<WinitLoop> for WinitLoop {
    fn sample(owner: &WinitLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        let signal = owner.owner_signal.lock().clone();
        if !signal.as_ref().is_some_and(|signal| {
            signal.accepting() && signal.owner() == std::thread::current().id()
        }) {
            return Err(PlatformError::Preferences {
                message: "the system's preferences require the live winit event-loop owner".into(),
            });
        }
        #[cfg(windows)]
        {
            match owner.with_state(|state| state.preferences.clone()) {
                PreferenceObservation::Observed(values) => Ok(values),
                PreferenceObservation::Deferred => Err(PlatformError::PreferencesDeferred),
                PreferenceObservation::Failed(message) => {
                    Err(PlatformError::Preferences { message })
                }
            }
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let source = owner.with_state(|state| {
                state
                    .preferences
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade)
            });
            source.ok_or(PlatformError::PreferencesDeferred)?.sample()
        }
    }
}

impl WinitApp {
    #[cfg(target_os = "linux")]
    pub(super) fn initialize_preferences(&mut self) -> Result<(), PlatformError> {
        if self.preference_source.is_none() {
            let signal = self.platform.owner_signal.lock().clone().ok_or_else(|| {
                PlatformError::Preferences {
                    message: "the Linux preference owner is unavailable".into(),
                }
            })?;
            let executor = self
                .platform
                .with_state(|state| Arc::clone(&state.background_executor));
            let source = self.platform.preference_start.start(&executor, &signal);
            self.platform
                .with_state(|state| state.preferences = Some(Arc::downgrade(&source)));
            self.preference_source = Some(source);
        }
        Ok(())
    }

    /// Start the sources of the host's system notices: sleep, shutdown, lock
    /// and memory pressure. A source that cannot start says so in the log and
    /// leaves the others running.
    #[cfg(target_os = "linux")]
    pub(super) fn start_host_notices(&mut self) {
        if self.host_notices.is_some() {
            return;
        }
        let Some(signal) = self.platform.owner_signal.lock().clone() else {
            return;
        };
        let executor = self
            .platform
            .with_state(|state| Arc::clone(&state.background_executor));
        self.host_notices = Some(self.platform.host_notice_start.start(&executor, &signal));
    }

    /// Deliver the notices the host's sources queued since the last turn.
    #[cfg(target_os = "linux")]
    pub(super) fn deliver_host_notices(&self) {
        if let Some(source) = &self.host_notices {
            source.deliver(&self.platform);
        }
    }

    /// Report a changed portal `color-scheme` to each window that has no winit
    /// theme of its own. The portal's wake reaches here on the owner; the
    /// window list is copied out so callbacks run without the state lock.
    #[cfg(target_os = "linux")]
    pub(super) fn deliver_portal_appearance(&self) {
        let Some(source) = &self.preference_source else {
            return;
        };
        let appearance = source.appearance();
        let windows: Vec<_> = self
            .platform
            .with_state(|state| state.windows.values().cloned().collect());
        for window in windows {
            if window.observe_portal_appearance(appearance) {
                window.refresh_state();
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn initialize_preferences(&mut self) -> Result<(), PlatformError> {
        if self.preference_source.is_none() {
            let signal = self.platform.owner_signal.lock().clone().ok_or_else(|| {
                PlatformError::Preferences {
                    message: "the AppKit preference owner is unavailable".into(),
                }
            })?;
            let (source, observers) = self.platform.preference_start.start(&signal);
            self.platform
                .with_state(|state| state.preferences = Some(Arc::downgrade(&source)));
            self.preference_source = Some(source);
            self.preference_observers = Some(observers);
        }
        Ok(())
    }

    #[cfg(windows)]
    pub(super) fn initialize_preferences(&mut self) -> Result<(), PlatformError> {
        let platform = Arc::clone(&self.platform);
        self.initialize_preferences_with(|signal| platform.preference_start.start(signal))
    }

    /// [`Self::initialize_preferences`] with the source `open` makes. The
    /// system's broadcasts reach the displays and the source through a
    /// receiver of their own (winit has no lifecycle subscriber on Windows),
    /// built from the subscribers that exist: a source that cannot open costs
    /// the preferences, never the displays' notifications, and its failure is
    /// reported once the receiver stands. A source that opens later joins a
    /// receiver built anew.
    #[cfg(windows)]
    pub(super) fn initialize_preferences_with(
        &mut self,
        open: impl FnOnce(&Arc<OwnerSignal>) -> Result<std::rc::Rc<dyn OwnerPreferences>, PlatformError>,
    ) -> Result<(), PlatformError> {
        use crate::owner::windows::broadcast::{Receiver, Subscriber};
        use std::rc::Rc;

        let Some(signal) = self.platform.owner_signal.lock().clone() else {
            return Ok(());
        };
        // Whether a source opened now, or why it could not.
        let opened = if self.preference_source.is_some() {
            Ok(false)
        } else {
            open(&signal).map(|source| {
                self.preference_source = Some(source);
                true
            })
        };
        if self.broadcasts.is_none() || matches!(opened, Ok(true)) {
            let mut subscribers: Vec<Rc<dyn Subscriber>> =
                vec![Arc::clone(&self.platform.displays).broadcasts(&signal)];
            if let Some(source) = &self.preference_source {
                subscribers.push(source.broadcasts());
            }
            // The receiver it replaces retires after its successor stands.
            let receiver =
                Receiver::new(subscribers).map_err(|error| PlatformError::Preferences {
                    message: error.to_string(),
                })?;
            self.broadcasts = Some(receiver);
        }
        opened?;
        // Outside any turn: an accepted reading asks for one.
        if self.refresh_preferences() {
            let _ = signal.wake();
        }
        Ok(())
    }

    /// Publishes before owner delivery; errors retain the native source's retry.
    /// `true` when a reading was accepted, which the application's turn must
    /// see: the caller owes it that turn, inside a turn it has begun
    /// ([`OwnerTurn::owe`](crate::owner::signal::OwnerTurn::owe)) or with a
    /// wake.
    #[cfg(windows)]
    pub(super) fn refresh_preferences(&self) -> bool {
        let Some(source) = &self.preference_source else {
            return false;
        };
        if !source.pending() {
            return false;
        }
        let observation = match source.sample() {
            Ok(values) => PreferenceObservation::Observed(values),
            Err(PlatformError::PreferencesDeferred) => PreferenceObservation::Deferred,
            Err(error) => PreferenceObservation::Failed(error.to_string()),
        };
        let accepted = matches!(&observation, PreferenceObservation::Observed(_));
        self.platform
            .with_state(|state| state.preferences = observation);
        accepted
    }

    /// Read the preferences and, when a reading was accepted, run the
    /// application's turn that sees it: owed to a turn begun here, so the
    /// reading costs no posted turn of its own.
    #[cfg(windows)]
    pub(super) fn deliver_preferences(&self) -> bool {
        if !self.refresh_preferences() {
            return false;
        }
        let signal = self.platform.owner_signal.lock().clone();
        signal.is_some_and(|signal| {
            let turn = signal.begin_turn();
            turn.owe();
            turn.drive() || signal.quitting()
        })
    }
}
