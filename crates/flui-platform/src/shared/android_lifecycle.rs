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
//! | `onSaveInstanceState` | the process may be killed without notice | session end |
//!
//! Focus alone must never stand for the other two: a dialog or the notification
//! shade takes focus from a running, visible Activity.
//!
//! `onTrimMemory` is not delivered by `android-activity` 0.6 (it needs a Java
//! `ComponentCallbacks2`, and this backend ships no Java), so memory pressure
//! comes from two places: `onLowMemory` is [`Pressure::Critical`], and the
//! level `ActivityManager.getMyMemoryState(..).lastTrimLevel` holds is read
//! when a callback that can change it arrives, never polled. This module
//! decides what each of them means; reading the level is the backend's.

use flui_platform_api::WindowExecutionState;

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
    pub(crate) fn from_main(event: &android_activity::MainEvent<'_>) -> Option<Self> {
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
    /// The process may be killed now: placement and storage are flushed.
    SessionEnded,
}

/// How much memory the system wants back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Pressure {
    Moderate,
    Critical,
}

impl Pressure {
    /// What an `ActivityManager.RunningAppProcessInfo.lastTrimLevel` asks for,
    /// `None` for the levels that are not memory pressure.
    ///
    /// `TRIM_MEMORY_UI_HIDDEN` (20) says the UI went away, which `Stop` already
    /// reports; `0` and unknown future values ask for nothing.
    pub(crate) const fn from_trim_level(level: i32) -> Option<Self> {
        match level {
            TRIM_MEMORY_RUNNING_MODERATE
            | TRIM_MEMORY_RUNNING_LOW
            | TRIM_MEMORY_BACKGROUND
            | TRIM_MEMORY_MODERATE => Some(Self::Moderate),
            TRIM_MEMORY_RUNNING_CRITICAL | TRIM_MEMORY_COMPLETE => Some(Self::Critical),
            _ => None,
        }
    }
}

// `android.content.ComponentCallbacks2` constants.
const TRIM_MEMORY_RUNNING_MODERATE: i32 = 5;
const TRIM_MEMORY_RUNNING_LOW: i32 = 10;
const TRIM_MEMORY_RUNNING_CRITICAL: i32 = 15;
const TRIM_MEMORY_BACKGROUND: i32 = 40;
const TRIM_MEMORY_MODERATE: i32 = 60;
const TRIM_MEMORY_COMPLETE: i32 = 80;

/// What one callback asks the backend to deliver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reaction {
    pub signal: Option<ActivitySignal>,
    pub pressure: Option<Pressure>,
}

/// The state the mapping needs between callbacks: the trim level last seen.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ActivityLifecycle {
    last_trim_level: Option<i32>,
}

impl ActivityLifecycle {
    /// What `event` asks to deliver. `read_trim_level` reads
    /// `lastTrimLevel`; it is called only for a callback that can have moved
    /// it, and may fail with `None`.
    ///
    /// A trim level that equals the one seen before delivers nothing, so
    /// `Pause` then `Stop` at the same level is one notice; the level going
    /// back to `0` and up again is a new one. When `LowMemory` and the level it
    /// reads both ask for pressure, only the larger is delivered.
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
            ActivityEvent::SaveState => Some(ActivitySignal::SessionEnded),
            ActivityEvent::LowMemory => None,
        };
        let from_event = matches!(event, ActivityEvent::LowMemory).then_some(Pressure::Critical);
        let from_level = if event.may_move_trim_level() {
            self.observe_trim_level(read_trim_level())
        } else {
            None
        };
        Reaction {
            signal,
            pressure: from_event.max(from_level),
        }
    }

    fn observe_trim_level(&mut self, level: Option<i32>) -> Option<Pressure> {
        let level = level?;
        if self.last_trim_level.replace(level) == Some(level) {
            return None;
        }
        Pressure::from_trim_level(level)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivityEvent::{self, *},
        ActivityLifecycle, ActivitySignal, Pressure, Reaction,
    };
    use flui_platform_api::WindowExecutionState::{Running, Suspended};

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
            (SaveState, Some(ActivitySignal::SessionEnded)),
            (LowMemory, None),
        ] {
            assert_eq!(react(event, None).signal, signal, "{event:?}");
        }

        // The system's own low-memory call is `Critical` with no level read.
        assert_eq!(react(LowMemory, None).pressure, Some(Pressure::Critical));

        // `lastTrimLevel` maps to the two levels.
        for (level, pressure) in [
            (0, None),
            (5, Some(Pressure::Moderate)),
            (10, Some(Pressure::Moderate)),
            (15, Some(Pressure::Critical)),
            (20, None),
            (40, Some(Pressure::Moderate)),
            (60, Some(Pressure::Moderate)),
            (80, Some(Pressure::Critical)),
            (-1, None),
            (81, None),
        ] {
            assert_eq!(Pressure::from_trim_level(level), pressure, "level {level}");
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
            Some(Pressure::Moderate)
        );
        assert_eq!(lifecycle.react(Stop, || None).pressure, None);
        assert_eq!(lifecycle.react(Resume, || Some(40)).pressure, None);
    }

    #[test]
    fn android_lifecycle_mapping_suppresses_a_repeated_level() {
        let mut lifecycle = ActivityLifecycle::default();
        let mut deliver = |event, level| lifecycle.react(event, || Some(level)).pressure;

        // `Pause` and `Stop` at the same level are one notice.
        assert_eq!(deliver(Pause, 40), Some(Pressure::Moderate));
        assert_eq!(deliver(Stop, 40), None);
        // A deeper level is new, the level back to none and up again is new.
        assert_eq!(deliver(LowMemory, 80), Some(Pressure::Critical));
        assert_eq!(deliver(Resume, 0), None);
        assert_eq!(deliver(Pause, 40), Some(Pressure::Moderate));
        // A level that asks for nothing is still remembered.
        assert_eq!(deliver(Stop, 20), None);
        assert_eq!(deliver(Resume, 20), None);
        assert_eq!(deliver(Pause, 40), Some(Pressure::Moderate));
    }

    #[test]
    fn android_lifecycle_mapping_low_memory_delivers_the_larger_level_once() {
        // `LowMemory` is `Critical`; the level it reads cannot add a second one.
        let mut lifecycle = ActivityLifecycle::default();
        assert_eq!(
            lifecycle.react(LowMemory, || Some(15)).pressure,
            Some(Pressure::Critical)
        );
        // A smaller level read at the same moment is subsumed...
        assert_eq!(
            lifecycle.react(LowMemory, || Some(40)).pressure,
            Some(Pressure::Critical)
        );
        // ...and the level is remembered, so `Stop` at it is silent.
        assert_eq!(lifecycle.react(Stop, || Some(40)).pressure, None);
        // `LowMemory` itself is delivered every time the system calls it.
        assert_eq!(
            lifecycle.react(LowMemory, || Some(40)).pressure,
            Some(Pressure::Critical)
        );
    }
}
