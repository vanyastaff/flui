//! The Android host's lifecycle: what the Activity's lifecycle callbacks mean.
//!
//! `android-activity` hands the looper one `MainEvent` per callback of the
//! Activity (`onStart`, `onResume`, `onWindowFocusChanged`, `onLowMemory`, …),
//! `FluiActivity` forwards `onTrimMemory` through `FluiHost`'s queue
//! (ADR-0255 decision 2), and the looper hands each to the hook the host's
//! table registers, [`ActivityRule`], which answers what to deliver. Android
//! keeps four things apart that a desktop window folds into one:
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
//! Memory pressure comes from the system's own calls: `onLowMemory` is
//! `MemoryPressureLevel::Critical`, and each `onTrimMemory` is the level its
//! `ComponentCallbacks2` value asks for, when it arrives. [`rule`] decides what
//! each callback means as a pure rule every host checks.
//!
//! The session end and the memory pressure reach the application through the
//! loop's lifecycle hooks; the window's signals reach its window.

#[cfg(target_os = "android")]
use parking_lot::Mutex;

#[cfg(target_os = "android")]
use crate::owner::android::activity_callbacks::{ActivityCallbacks, ActivityEvent, Reaction};

/// The Activity's lifecycle as the host's table registers it with the looper:
/// the rule, with what it keeps between callbacks.
#[cfg(target_os = "android")]
#[derive(Default)]
pub(crate) struct ActivityRule {
    rule: Mutex<rule::ActivityLifecycle>,
}

#[cfg(target_os = "android")]
impl ActivityCallbacks for ActivityRule {
    fn react(&self, event: ActivityEvent) -> Reaction {
        self.rule.lock().react(event)
    }
}

/// What each Activity callback means for the window and the application, as a
/// pure rule.
#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
pub(crate) mod rule {
    use flui_platform::{MemoryPressureLevel, WindowExecutionState};

    use crate::owner::android::activity_callbacks::{ActivityEvent, ActivitySignal, Reaction};

    // `android.content.ComponentCallbacks2` constants.
    const TRIM_MEMORY_RUNNING_MODERATE: i32 = 5;
    const TRIM_MEMORY_RUNNING_LOW: i32 = 10;
    const TRIM_MEMORY_RUNNING_CRITICAL: i32 = 15;
    const TRIM_MEMORY_BACKGROUND: i32 = 40;
    const TRIM_MEMORY_MODERATE: i32 = 60;
    const TRIM_MEMORY_COMPLETE: i32 = 80;

    /// What an `onTrimMemory` level asks for, `None` for the levels that are
    /// not memory pressure.
    ///
    /// `TRIM_MEMORY_UI_HIDDEN` (20) says the UI went away, which `Stop` already
    /// reports; unknown future values ask for nothing.
    pub(crate) const fn pressure_for_trim_level(level: i32) -> Option<MemoryPressureLevel> {
        match level {
            TRIM_MEMORY_RUNNING_MODERATE
            | TRIM_MEMORY_RUNNING_LOW
            | TRIM_MEMORY_BACKGROUND
            | TRIM_MEMORY_MODERATE => Some(MemoryPressureLevel::Moderate),
            TRIM_MEMORY_RUNNING_CRITICAL | TRIM_MEMORY_COMPLETE => {
                Some(MemoryPressureLevel::Critical)
            }
            _ => None,
        }
    }

    /// The mapping from Activity callbacks to [`Reaction`]s. It keeps one
    /// thing between callbacks: whether this start-to-stop cycle already ended
    /// the session.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct ActivityLifecycle {
        session_end_announced: bool,
    }

    impl ActivityLifecycle {
        /// What `event` asks to deliver. Each memory call of the system is
        /// delivered as it comes: every one asks for memory back again.
        pub(crate) fn react(&mut self, event: ActivityEvent) -> Reaction {
            let signal = match event {
                ActivityEvent::Start => Some(ActivitySignal::Visibility(true)),
                ActivityEvent::Stop => Some(ActivitySignal::Visibility(false)),
                ActivityEvent::Resume => {
                    Some(ActivitySignal::Execution(WindowExecutionState::Running))
                }
                ActivityEvent::Pause => {
                    Some(ActivitySignal::Execution(WindowExecutionState::Suspended))
                }
                ActivityEvent::GainedFocus => Some(ActivitySignal::Focus(true)),
                ActivityEvent::LostFocus => Some(ActivitySignal::Focus(false)),
                ActivityEvent::LowMemory
                | ActivityEvent::SaveState
                | ActivityEvent::TrimMemory(_) => None,
            };
            let pressure = match event {
                ActivityEvent::LowMemory => Some(MemoryPressureLevel::Critical),
                ActivityEvent::TrimMemory(level) => pressure_for_trim_level(level),
                _ => None,
            };
            Reaction {
                signal,
                session_end: self.observe_session_end(event),
                pressure,
            }
        }

        /// Whether `event` is the first of its cycle to say the process may be
        /// killed. `onStop` says it, and `onSaveInstanceState` does too, before
        /// `onStop` below API 28 and after it from there on; a Back that
        /// finishes the Activity stops it without saving any state. The cycle
        /// ends at the next `onStart`.
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
    }

    #[cfg(test)]
    mod tests {
        use super::{ActivityLifecycle, MemoryPressureLevel, pressure_for_trim_level};
        use crate::owner::android::activity_callbacks::{
            ActivityEvent::{self, *},
            ActivitySignal, Reaction,
        };
        use flui_platform::WindowExecutionState::{Running, Suspended};

        /// One event, from a fresh lifecycle.
        fn react(event: ActivityEvent) -> Reaction {
            ActivityLifecycle::default().react(event)
        }

        /// Everything a target was told, in order.
        #[derive(Default)]
        struct Recorder(std::cell::RefCell<Vec<String>>);

        impl Recorder {
            fn told(&self) -> Vec<String> {
                self.0.borrow().clone()
            }
        }

        impl crate::owner::android::activity_callbacks::ActivityTarget for Recorder {
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
            let mut told = |event| {
                let target = Recorder::default();
                lifecycle.react(event).deliver(&target);
                target.told()
            };
            assert_eq!(told(Start), ["visible true"]);
            assert_eq!(told(Resume), ["execution Running"]);
            assert_eq!(told(GainedFocus), ["focus true"]);
            assert_eq!(told(LostFocus), ["focus false"]);
            assert_eq!(told(LowMemory), ["pressure Critical"]);
            assert_eq!(told(TrimMemory(5)), ["pressure Moderate"]);
            assert_eq!(told(Pause), ["execution Suspended"]);
            assert_eq!(told(Stop), ["visible false", "session ended"]);
            assert_eq!(told(TrimMemory(20)), Vec::<String>::new());
            assert_eq!(told(TrimMemory(80)), ["pressure Critical"]);
        }

        #[test]
        fn android_lifecycle_mapping_ends_the_session_once_per_cycle() {
            // `Stop` ends the session itself (a Back that finishes the Activity
            // saves no state), and the `SaveState` of the same cycle, before or
            // after it, adds nothing; the next `Start` begins a new cycle.
            let mut lifecycle = ActivityLifecycle::default();
            let mut ends_session = |event| lifecycle.react(event).session_end;
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
            assert!(!ends_session(TrimMemory(80)));
            assert!(!ends_session(Start));
            assert!(ends_session(Stop), "a second trip to the background");
        }

        #[test]
        fn android_lifecycle_mapping() {
            // Each callback gives its own signal; focus never stands for the
            // others, and no memory call gives one.
            for (event, signal) in [
                (Start, Some(ActivitySignal::Visibility(true))),
                (Stop, Some(ActivitySignal::Visibility(false))),
                (Resume, Some(ActivitySignal::Execution(Running))),
                (Pause, Some(ActivitySignal::Execution(Suspended))),
                (GainedFocus, Some(ActivitySignal::Focus(true))),
                (LostFocus, Some(ActivitySignal::Focus(false))),
                (SaveState, None),
                (LowMemory, None),
                (TrimMemory(15), None),
            ] {
                assert_eq!(react(event).signal, signal, "{event:?}");
            }

            // Only the system's memory calls ask for memory: `onLowMemory` is
            // `Critical`, and `onTrimMemory` maps its level to the two levels.
            for event in [
                Start,
                Stop,
                Resume,
                Pause,
                GainedFocus,
                LostFocus,
                SaveState,
            ] {
                assert_eq!(react(event).pressure, None, "{event:?}");
            }
            assert_eq!(
                react(LowMemory).pressure,
                Some(MemoryPressureLevel::Critical)
            );
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
                assert_eq!(react(TrimMemory(level)).pressure, pressure, "level {level}");
            }
        }

        #[test]
        fn android_lifecycle_mapping_delivers_every_memory_call() {
            // The system calls again while memory stays low; each call asks
            // again, so a repeated level is delivered, not suppressed.
            let mut lifecycle = ActivityLifecycle::default();
            let mut deliver = |event| lifecycle.react(event).pressure;
            assert_eq!(deliver(TrimMemory(10)), Some(MemoryPressureLevel::Moderate));
            assert_eq!(deliver(TrimMemory(10)), Some(MemoryPressureLevel::Moderate));
            assert_eq!(deliver(LowMemory), Some(MemoryPressureLevel::Critical));
            assert_eq!(deliver(LowMemory), Some(MemoryPressureLevel::Critical));
            assert_eq!(deliver(Stop), None);
            assert_eq!(deliver(TrimMemory(80)), Some(MemoryPressureLevel::Critical));
        }

        #[test]
        fn android_lifecycle_mapping_lets_the_caller_deliver_the_signal_first() {
            // An arm of the looper that acts around its signal takes it; the
            // rest of the reaction is delivered without it.
            let target = Recorder::default();
            let mut reaction = react(Stop);
            let signal = reaction.signal.take().expect("Stop has a signal");
            reaction.deliver(&target);
            assert_eq!(target.told(), ["session ended"]);
            signal.deliver(&target);
            assert_eq!(target.told(), ["session ended", "visible false"]);
        }
    }
}
