//! The Activity's lifecycle callbacks as the looper hands them over, and what
//! it delivers for each.
//!
//! `android-activity` hands the looper one `MainEvent` per callback of the
//! Activity. The looper names the ones about the lifecycle ([`ActivityEvent`]),
//! asks the hook the host's table registers what each means
//! ([`ActivityCallbacks`]; the lifecycle feature implements it), and delivers
//! the [`Reaction`] to the window and the application ([`ActivityTarget`]).

use flui_platform::{MemoryPressureLevel, WindowExecutionState};

/// The Activity callbacks that matter for the lifecycle, without the payloads
/// `android_activity::MainEvent` carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityEvent {
    Start,
    Stop,
    Resume,
    Pause,
    GainedFocus,
    LostFocus,
    LowMemory,
    SaveState,
}

impl ActivityEvent {
    /// The lifecycle event `event` is, `None` for the ones that are not about
    /// the lifecycle (window creation, input, configuration).
    #[cfg(target_os = "android")]
    pub(crate) fn from_main_event(event: &android_activity::MainEvent<'_>) -> Option<Self> {
        use android_activity::MainEvent;
        match event {
            MainEvent::Start => Some(Self::Start),
            MainEvent::Stop => Some(Self::Stop),
            MainEvent::Resume { .. } => Some(Self::Resume),
            MainEvent::Pause => Some(Self::Pause),
            MainEvent::GainedFocus => Some(Self::GainedFocus),
            MainEvent::LostFocus => Some(Self::LostFocus),
            MainEvent::LowMemory => Some(Self::LowMemory),
            MainEvent::SaveState { .. } => Some(Self::SaveState),
            _ => None,
        }
    }
}

/// What the Activity's lifecycle callbacks mean: the hook the looper hands
/// each [`ActivityEvent`] to, which the host's table registers.
#[cfg(target_os = "android")]
pub(crate) trait ActivityCallbacks: Send + Sync {
    /// What `event` asks the loop to deliver.
    fn react(&self, event: ActivityEvent) -> Reaction;
}

/// The one thing an Activity callback tells the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivitySignal {
    Visibility(bool),
    Execution(WindowExecutionState),
    Focus(bool),
}

/// What one callback asks the backend to deliver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reaction {
    pub signal: Option<ActivitySignal>,
    /// The process may be killed now. It asks for saving, not for closing:
    /// the Activity may start again, and this repeats on every trip to the
    /// background.
    pub session_end: bool,
    pub pressure: Option<MemoryPressureLevel>,
}

/// Where a [`Reaction`] ends up: the backend's window and host callbacks.
pub(crate) trait ActivityTarget {
    fn visibility_changed(&self, visible: bool);
    fn execution_changed(&self, state: WindowExecutionState);
    fn focus_changed(&self, focused: bool);
    /// The process may be killed now; see [`Reaction::session_end`].
    fn session_ended(&self);
    fn memory_pressure_raised(&self, level: MemoryPressureLevel);
}

impl ActivitySignal {
    /// Hand this signal to `target`.
    pub(crate) fn deliver(self, target: &impl ActivityTarget) {
        match self {
            Self::Visibility(visible) => target.visibility_changed(visible),
            Self::Execution(state) => target.execution_changed(state),
            Self::Focus(focused) => target.focus_changed(focused),
        }
    }
}

impl Reaction {
    /// Hand what is left of this reaction to `target`: the signal, if the
    /// caller did not take it to deliver at a point of its own, then the end
    /// of the session, then the memory pressure.
    pub(crate) fn deliver(self, target: &impl ActivityTarget) {
        if let Some(signal) = self.signal {
            signal.deliver(target);
        }
        if self.session_end {
            target.session_ended();
        }
        if let Some(level) = self.pressure {
            target.memory_pressure_raised(level);
        }
    }
}
