//! The Activity's lifecycle callbacks as the looper hands them over, and what
//! it delivers for each.
//!
//! `android-activity` hands the looper one `MainEvent` per callback of the
//! Activity. The looper names the ones about the lifecycle ([`ActivityEvent`]),
//! asks the hook the host's table registers what each means
//! ([`ActivityCallbacks`]; the lifecycle feature implements it), and delivers
//! the [`Reaction`] to the window and the application ([`ActivityTarget`]).

use flui_platform::{MemoryPressureLevel, WindowExecutionState, WindowFocus};

/// The Activity callbacks that matter for the lifecycle: the ones
/// `android-activity` hands over, without the payloads
/// `android_activity::MainEvent` carries, and `onTrimMemory`, which
/// `FluiActivity` forwards through `FluiHost`'s queue.
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
    /// `onTrimMemory`, with the `ComponentCallbacks2` level.
    TrimMemory(i32),
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

/// The one fact an Activity callback tells the window. What the window
/// reports follows from the three together ([`ActivityStanding`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivitySignal {
    /// `onStart` (`true`) and `onStop` (`false`).
    Visibility(bool),
    /// `onResume` (`true`) and `onPause` (`false`).
    Foreground(bool),
    /// `onWindowFocusChanged`.
    Focus(bool),
}

/// The Activity's three lifecycle facts as its callbacks last reported them,
/// and what the window reports from them (ADR-0364).
///
/// Android keeps a paused Activity on screen wherever another one holds the
/// foreground without covering it: the other half of a split screen below API
/// 29, a translucent or dialog Activity on top, picture-in-picture. Such a
/// window keeps drawing; only `onStop` takes it off screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ActivityStanding {
    /// Between `onStart` and `onStop`.
    pub(crate) visible: bool,
    /// Between `onResume` and `onPause`.
    pub(crate) foreground: bool,
    /// The window's input focus, as `onWindowFocusChanged` reports it.
    pub(crate) focused: bool,
}

impl ActivityStanding {
    /// The standing of an Activity whose loop ended: off screen, in the
    /// background, unfocused, so no frame or input is delivered after it.
    pub(crate) const RETIRED: Self = Self {
        visible: false,
        foreground: false,
        focused: false,
    };

    const VISIBLE: u8 = 1;
    const FOREGROUND: u8 = 1 << 1;
    const FOCUSED: u8 = 1 << 2;

    /// Whether the loop delivers the window's frames and input and samples
    /// its preferences: while the Activity is on screen, paused or not.
    #[must_use]
    pub(crate) const fn runs(self) -> bool {
        self.visible
    }

    /// The window's execution state: running while it is on screen, suspended
    /// from `onStop` to `onStart`, whatever the focus and the native window.
    #[must_use]
    pub(crate) const fn execution(self) -> WindowExecutionState {
        if self.runs() {
            WindowExecutionState::Running
        } else {
            WindowExecutionState::Suspended
        }
    }

    /// The window's focus: focused while it has the input focus and the
    /// Activity is in the foreground. A paused Activity is not the one the
    /// user works in even where a focus report is still to come, so its
    /// lifecycle reads `Inactive` from `onPause` on. An Activity has no
    /// activation apart from its window's focus.
    #[must_use]
    pub(crate) const fn focus(self) -> WindowFocus {
        let focused = self.focused && self.foreground;
        WindowFocus::reported(focused, focused)
    }

    /// The standing once `signal` is recorded.
    #[must_use]
    pub(crate) const fn with(self, signal: ActivitySignal) -> Self {
        match signal {
            ActivitySignal::Visibility(visible) => Self { visible, ..self },
            ActivitySignal::Foreground(foreground) => Self { foreground, ..self },
            ActivitySignal::Focus(focused) => Self { focused, ..self },
        }
    }

    /// The standing as one byte, which the loop and its window share through
    /// one atomic, so a reader never sees half of a change.
    #[must_use]
    pub(crate) const fn to_bits(self) -> u8 {
        (if self.visible { Self::VISIBLE } else { 0 })
            | (if self.foreground { Self::FOREGROUND } else { 0 })
            | (if self.focused { Self::FOCUSED } else { 0 })
    }

    /// The standing [`Self::to_bits`] wrote; other bits are ignored.
    #[must_use]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self {
            visible: bits & Self::VISIBLE != 0,
            foreground: bits & Self::FOREGROUND != 0,
            focused: bits & Self::FOCUSED != 0,
        }
    }
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
    /// One of the Activity's three facts changed.
    fn fact_changed(&self, signal: ActivitySignal);
    /// The process may be killed now; see [`Reaction::session_end`].
    fn session_ended(&self);
    fn memory_pressure_raised(&self, level: MemoryPressureLevel);
}

impl ActivitySignal {
    /// Hand this signal to `target`.
    pub(crate) fn deliver(self, target: &impl ActivityTarget) {
        target.fact_changed(self);
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
