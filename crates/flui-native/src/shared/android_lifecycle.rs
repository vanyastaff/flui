//! What the Activity's lifecycle callbacks mean for a window, as a pure rule.
//!
//! `android-activity` hands the backend one [`MainEvent`] per callback of the
//! Activity (`onStart`, `onResume`, `onWindowFocusChanged`, `onLowMemory`, …).
//! Android keeps four things apart that a desktop window folds into one:
//!
//! | Callback | Meaning | Signal |
//! |---|---|---|
//! | `onStart` / `onStop` | the Activity is / is not on screen | visibility |
//! | `onResume` / `onPause` | the Activity may / may not run | execution state |
//! | `onWindowFocusChanged` | the window has / lost input focus | focus |
//! | `onStop` / `onSaveInstanceState` | the process may be killed without notice | session end |
//!
//! Focus alone must never stand for the other two: a dialog or the notification
//! shade takes focus from a running, visible Activity.
//!
//! `onTrimMemory` is not delivered by `android-activity` 0.6 (it needs a Java
//! `ComponentCallbacks2`, and this backend ships no Java), so memory pressure
//! comes from two places: `onLowMemory` is [`MemoryPressureLevel::Critical`], and the
//! level `ActivityManager.getMyMemoryState(..).lastTrimLevel` holds is read
//! when a callback that can change it arrives, never polled. This module
//! decides what each of them means; reading the level is the backend's.

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

    /// Whether the system may have moved `lastTrimLevel` by the time this
    /// callback runs: the moments an Activity goes to or comes from the
    /// background, and the system's own low-memory call.
    const fn may_move_trim_level(self) -> bool {
        matches!(
            self,
            Self::LowMemory | Self::Pause | Self::Stop | Self::Resume
        )
    }
}

/// The one thing an Activity callback tells the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivitySignal {
    Visibility(bool),
    Execution(WindowExecutionState),
    Focus(bool),
}

// `android.content.ComponentCallbacks2` constants.
const TRIM_MEMORY_RUNNING_MODERATE: i32 = 5;
const TRIM_MEMORY_RUNNING_LOW: i32 = 10;
const TRIM_MEMORY_RUNNING_CRITICAL: i32 = 15;
const TRIM_MEMORY_BACKGROUND: i32 = 40;
const TRIM_MEMORY_MODERATE: i32 = 60;
const TRIM_MEMORY_COMPLETE: i32 = 80;

/// What an `ActivityManager.RunningAppProcessInfo.lastTrimLevel` asks for,
/// `None` for the levels that are not memory pressure.
///
/// `TRIM_MEMORY_UI_HIDDEN` (20) says the UI went away, which `Stop` already
/// reports; `0` and unknown future values ask for nothing.
const fn pressure_for_trim_level(level: i32) -> Option<MemoryPressureLevel> {
    match level {
        TRIM_MEMORY_RUNNING_MODERATE
        | TRIM_MEMORY_RUNNING_LOW
        | TRIM_MEMORY_BACKGROUND
        | TRIM_MEMORY_MODERATE => Some(MemoryPressureLevel::Moderate),
        TRIM_MEMORY_RUNNING_CRITICAL | TRIM_MEMORY_COMPLETE => Some(MemoryPressureLevel::Critical),
        _ => None,
    }
}

/// What one callback asks the backend to deliver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reaction {
    pub signal: Option<ActivitySignal>,
    /// The process may be killed now. It asks for saving, not for closing:
    /// the Activity may start again, and this repeats on every trip to the
    /// background (see [`ActivityLifecycle::react`]).
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

/// The mapping from Activity callbacks to [`Reaction`]s. It keeps two things
/// between callbacks: the trim level last seen, which suppresses a level that is read
/// again unchanged, and whether this start-to-stop cycle already ended the session.
///
/// A level is remembered when it is read, not when a subscriber hears of it:
/// notices are not replayed, so a level seen before the host listens (the
/// first callbacks precede the application's bootstrap) is not announced again
/// later.
#[derive(Clone, Debug, Default)]
pub(crate) struct ActivityLifecycle {
    last_trim_level: Option<i32>,
    session_end_announced: bool,
}

impl ActivityLifecycle {
    /// What `event` asks to deliver. `read_trim_level` reads
    /// `lastTrimLevel`; it is called only for a callback that can have moved
    /// it, and may fail with `None`.
    ///
    /// A trim level that equals the one seen before delivers nothing, so
    /// `Pause` then `Stop` at the same level is one notice; the level going
    /// back to `0` and up again is a new one. `Critical` is the largest level, so
    /// when `LowMemory` asks for it the level read at the same time adds nothing.
    pub(crate) fn react(
        &mut self,
        event: ActivityEvent,
        read_trim_level: impl FnOnce() -> Option<i32>,
    ) -> Reaction {
        let signal = match event {
            ActivityEvent::Start => Some(ActivitySignal::Visibility(true)),
            ActivityEvent::Stop => Some(ActivitySignal::Visibility(false)),
            ActivityEvent::Resume => Some(ActivitySignal::Execution(WindowExecutionState::Running)),
            ActivityEvent::Pause => {
                Some(ActivitySignal::Execution(WindowExecutionState::Suspended))
            }
            ActivityEvent::GainedFocus => Some(ActivitySignal::Focus(true)),
            ActivityEvent::LostFocus => Some(ActivitySignal::Focus(false)),
            ActivityEvent::LowMemory | ActivityEvent::SaveState => None,
        };
        let from_event =
            matches!(event, ActivityEvent::LowMemory).then_some(MemoryPressureLevel::Critical);
        let from_level = if event.may_move_trim_level() {
            self.observe_trim_level(read_trim_level())
        } else {
            None
        };
        Reaction {
            signal,
            session_end: self.observe_session_end(event),
            pressure: from_event.or(from_level),
        }
    }

    /// Whether `event` is the first of its cycle to say the process may be
    /// killed. `onStop` says it, and `onSaveInstanceState` does too, before
    /// `onStop` below API 28 and after it from there on; a Back that finishes the
    /// Activity stops it without saving any state. The cycle ends at the next
    /// `onStart`.
    fn observe_session_end(&mut self, event: ActivityEvent) -> bool {
        match event {
            ActivityEvent::Start => {
                self.session_end_announced = false;
                false
            }
            ActivityEvent::Stop | ActivityEvent::SaveState => {
                !std::mem::replace(&mut self.session_end_announced, true)
            }
            _ => false,
        }
    }

    fn observe_trim_level(&mut self, level: Option<i32>) -> Option<MemoryPressureLevel> {
        let level = level?;
        if self.last_trim_level.replace(level) == Some(level) {
            return None;
        }
        pressure_for_trim_level(level)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivityEvent::{self, *},
        ActivityLifecycle, ActivitySignal, MemoryPressureLevel, Reaction, pressure_for_trim_level,
    };
    use flui_platform::WindowExecutionState::{Running, Suspended};

    const ALL_EVENTS: [ActivityEvent; 8] = [
        Start,
        Stop,
        Resume,
        Pause,
        GainedFocus,
        LostFocus,
        LowMemory,
        SaveState,
    ];

    /// One event with the trim level it finds, from a fresh lifecycle.
    fn react(event: ActivityEvent, trim_level: Option<i32>) -> Reaction {
        ActivityLifecycle::default().react(event, || trim_level)
    }

    /// Everything a target was told, in order.
    #[derive(Default)]
    struct Recorder(std::cell::RefCell<Vec<String>>);

    impl Recorder {
        fn told(&self) -> Vec<String> {
            self.0.borrow().clone()
        }
    }

    impl super::ActivityTarget for Recorder {
        fn visibility_changed(&self, visible: bool) {
            self.0.borrow_mut().push(format!("visible {visible}"));
        }
        fn execution_changed(&self, state: flui_platform::WindowExecutionState) {
            self.0.borrow_mut().push(format!("execution {state:?}"));
        }
        fn focus_changed(&self, focused: bool) {
            self.0.borrow_mut().push(format!("focus {focused}"));
        }
        fn session_ended(&self) {
            self.0.borrow_mut().push("session ended".into());
        }
        fn memory_pressure_raised(&self, level: MemoryPressureLevel) {
            self.0.borrow_mut().push(format!("pressure {level:?}"));
        }
    }

    #[test]
    fn android_lifecycle_mapping_reaches_the_target() {
        // Each callback is told to the target once: signal, then the end of
        // the session, then pressure, and nothing else.
        let mut lifecycle = ActivityLifecycle::default();
        let mut told = |event, level| {
            let target = Recorder::default();
            lifecycle.react(event, || Some(level)).deliver(&target);
            target.told()
        };
        assert_eq!(told(Start, 0), ["visible true"]);
        assert_eq!(told(Resume, 0), ["execution Running"]);
        assert_eq!(told(GainedFocus, 0), ["focus true"]);
        assert_eq!(told(LostFocus, 0), ["focus false"]);
        assert_eq!(told(LowMemory, 0), ["pressure Critical"]);
        assert_eq!(
            told(Pause, 40),
            ["execution Suspended", "pressure Moderate"]
        );
        assert_eq!(
            told(Stop, 40),
            ["visible false", "session ended"],
            "the level is unchanged"
        );
    }

    #[test]
    fn android_lifecycle_mapping_ends_the_session_once_per_cycle() {
        // `Stop` ends the session itself (a Back that finishes the Activity
        // saves no state), and the `SaveState` of the same cycle, before or
        // after it, adds nothing; the next `Start` begins a new cycle.
        let mut lifecycle = ActivityLifecycle::default();
        let mut ends_session = |event| lifecycle.react(event, || None).session_end;
        assert!(!ends_session(Start));
        assert!(ends_session(Stop));
        assert!(!ends_session(SaveState), "API 28 and later: after `Stop`");
        assert!(!ends_session(Start));
        assert!(ends_session(SaveState), "before API 28: before `Stop`");
        assert!(!ends_session(Stop));
        assert!(!ends_session(Resume));
        assert!(!ends_session(Pause));
        assert!(!ends_session(LostFocus));
        assert!(!ends_session(LowMemory));
        assert!(!ends_session(Start));
        assert!(ends_session(Stop), "a second trip to the background");
    }

    #[test]
    fn android_lifecycle_mapping_lets_the_caller_deliver_the_signal_first() {
        // `Pause` releases the surface before it suspends: the caller takes the
        // signal, and the rest of the reaction is only the pressure.
        let target = Recorder::default();
        let mut reaction = ActivityLifecycle::default().react(Pause, || Some(15));
        let signal = reaction.signal.take().expect("Pause has a signal");
        reaction.deliver(&target);
        assert_eq!(target.told(), ["pressure Critical"]);
        signal.deliver(&target);
        assert_eq!(target.told(), ["pressure Critical", "execution Suspended"]);
    }

    #[test]
    fn android_lifecycle_mapping() {
        // Each callback gives its own signal; focus never stands for the others.
        for (event, signal) in [
            (Start, Some(ActivitySignal::Visibility(true))),
            (Stop, Some(ActivitySignal::Visibility(false))),
            (Resume, Some(ActivitySignal::Execution(Running))),
            (Pause, Some(ActivitySignal::Execution(Suspended))),
            (GainedFocus, Some(ActivitySignal::Focus(true))),
            (LostFocus, Some(ActivitySignal::Focus(false))),
            (SaveState, None),
            (LowMemory, None),
        ] {
            assert_eq!(react(event, None).signal, signal, "{event:?}");
        }

        // The system's own low-memory call is `Critical` with no level read.
        assert_eq!(
            react(LowMemory, None).pressure,
            Some(MemoryPressureLevel::Critical)
        );

        // `lastTrimLevel` maps to the two levels.
        for (level, pressure) in [
            (0, None),
            (5, Some(MemoryPressureLevel::Moderate)),
            (10, Some(MemoryPressureLevel::Moderate)),
            (15, Some(MemoryPressureLevel::Critical)),
            (20, None),
            (40, Some(MemoryPressureLevel::Moderate)),
            (60, Some(MemoryPressureLevel::Moderate)),
            (80, Some(MemoryPressureLevel::Critical)),
            (-1, None),
            (81, None),
        ] {
            assert_eq!(pressure_for_trim_level(level), pressure, "level {level}");
        }

        // Only a callback that can have moved the level reads it.
        for event in ALL_EVENTS {
            let mut reads = 0;
            let reaction = ActivityLifecycle::default().react(event, || {
                reads += 1;
                Some(80)
            });
            let reads_level = matches!(event, LowMemory | Pause | Stop | Resume);
            assert_eq!(reads, usize::from(reads_level), "{event:?} reads the level");
            assert_eq!(
                reaction.pressure.is_some(),
                reads_level,
                "{event:?} delivers pressure"
            );
        }

        // A failed read leaves the last level as it was.
        let mut lifecycle = ActivityLifecycle::default();
        assert_eq!(
            lifecycle.react(Pause, || Some(40)).pressure,
            Some(MemoryPressureLevel::Moderate)
        );
        assert_eq!(lifecycle.react(Stop, || None).pressure, None);
        assert_eq!(lifecycle.react(Resume, || Some(40)).pressure, None);
    }

    #[test]
    fn android_lifecycle_mapping_suppresses_a_repeated_level() {
        let mut lifecycle = ActivityLifecycle::default();
        let mut deliver = |event, level| lifecycle.react(event, || Some(level)).pressure;

        // `Pause` and `Stop` at the same level are one notice.
        assert_eq!(deliver(Pause, 40), Some(MemoryPressureLevel::Moderate));
        assert_eq!(deliver(Stop, 40), None);
        // A deeper level is new, the level back to none and up again is new.
        assert_eq!(deliver(LowMemory, 80), Some(MemoryPressureLevel::Critical));
        assert_eq!(deliver(Resume, 0), None);
        assert_eq!(deliver(Pause, 40), Some(MemoryPressureLevel::Moderate));
        // A level that asks for nothing is still remembered.
        assert_eq!(deliver(Stop, 20), None);
        assert_eq!(deliver(Resume, 20), None);
        assert_eq!(deliver(Pause, 40), Some(MemoryPressureLevel::Moderate));
    }

    #[test]
    fn android_lifecycle_mapping_low_memory_always_delivers_critical() {
        // `LowMemory` is `Critical`; the level it reads cannot add a second one.
        let mut lifecycle = ActivityLifecycle::default();
        assert_eq!(
            lifecycle.react(LowMemory, || Some(15)).pressure,
            Some(MemoryPressureLevel::Critical)
        );
        // A smaller level read at the same moment is subsumed...
        assert_eq!(
            lifecycle.react(LowMemory, || Some(40)).pressure,
            Some(MemoryPressureLevel::Critical)
        );
        // ...and the level is remembered, so `Stop` at it is silent.
        assert_eq!(lifecycle.react(Stop, || Some(40)).pressure, None);
        // `LowMemory` itself is delivered every time the system calls it.
        assert_eq!(
            lifecycle.react(LowMemory, || Some(40)).pressure,
            Some(MemoryPressureLevel::Critical)
        );
    }
}
