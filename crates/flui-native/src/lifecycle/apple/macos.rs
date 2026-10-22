//! The macOS host's lifecycle: the machine sleeping and waking, the session
//! locking and unlocking, memory pressure and the announcement that the
//! user's session is about to end.
//!
//! - `notices`: the AppKit, distributed-notification and dispatch sources
//!   (`SystemNoticeSources`), which the loop starts listening once the
//!   application has bootstrapped;
//! - [`memory_pressure`]: what a memory-pressure dispatch source reports, as a
//!   level.
//!
//! The answer to `applicationShouldTerminate:` while the session ends is the
//! loop's (`owner::apple::terminate_gate`): the announcement here only tells
//! the loop control. The rule module makes no system call, so every host
//! checks it.

#[cfg(target_os = "macos")]
pub(crate) use notices::SystemNoticeSources;

#[cfg(target_os = "macos")]
mod notices {
    //! The macOS sources of system notices: the machine sleeping and waking,
    //! the session locking and unlocking, memory pressure, and the
    //! announcement that the user's session is about to end.
    //!
    //! Every source delivers on the main thread, which is the owner thread, and
    //! only forwards: a notice goes to the platform's notice channel, a
    //! session-end announcement to the loop control that answers the terminate
    //! request. Each source is removed when the watch the loop keeps drops.

    // The dispatch source type and the notification names are libdispatch's and
    // AppKit's statics.
    #![expect(unsafe_code)]

    use std::sync::{Arc, Weak};

    use block2::RcBlock;
    use dispatch2::{
        _dispatch_source_type_memorypressure, DispatchObject, DispatchQueue, DispatchRetained,
        DispatchSource,
    };
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillPowerOffNotification,
        NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotificationCenter, NSString};

    use flui_platform::SystemNotice;

    use super::memory_pressure::{SUBSCRIBED, level_of};
    use crate::owner::apple::macos::LoopControl;
    use crate::owner::apple::macos::NoticeSources;
    use crate::owner::apple::macos::observation::Observation;
    use crate::owner::notices::NoticeChannel;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::owner::watch::Watch;

    /// The distributed notification the lock screen posts when the session
    /// locks.
    const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";
    /// The distributed notification the lock screen posts when the session
    /// unlocks.
    const SCREEN_UNLOCKED: &str = "com.apple.screenIsUnlocked";

    /// A memory-pressure dispatch source on the main queue.
    struct MemoryPressureSource {
        source: DispatchRetained<DispatchSource>,
    }

    impl MemoryPressureSource {
        fn new(notices: Weak<dyn NoticeChannel>) -> Self {
            let mask = SUBSCRIBED.iter().fold(0, |mask, state| mask | state);
            // SAFETY: the memory-pressure source type is a constant exported by
            // libdispatch; the handle is unused for it; the mask is a union of
            // the states it defines.
            let source = unsafe {
                DispatchSource::new(
                    (&raw const _dispatch_source_type_memorypressure).cast_mut(),
                    0,
                    mask,
                    Some(DispatchQueue::main()),
                )
            };
            // The handler reads the data of the source it belongs to, so it
            // holds the source: the cycle ends when `drop` cancels it, after
            // which libdispatch releases the handler.
            let in_handler = source.clone();
            let handler = RcBlock::new(move || {
                os_entry(
                    "the memory-pressure source",
                    AfterPanic::Answer(|| ()),
                    || {
                        if let Some(level) = level_of(in_handler.data())
                            && let Some(notices) = notices.upgrade()
                        {
                            notices.dispatch(SystemNotice::MemoryPressure(level));
                        }
                    },
                );
            });
            // SAFETY: the block is copied by libdispatch when set, and it is
            // `Fn()` as the parameter requires.
            unsafe {
                source.set_event_handler_with_block((&raw const *handler).cast_mut());
            }
            source.activate();
            Self { source }
        }
    }

    impl Drop for MemoryPressureSource {
        fn drop(&mut self) {
            // Cancelling is what ends the cycle between the source and its
            // handler: libdispatch releases a canceled source's handler, and
            // with it the clone of the source that the handler holds.
            self.source.cancel();
        }
    }

    /// The distributed notification centre delivering while the application
    /// is inactive, for as long as this value lives.
    ///
    /// The centre holds back what it is sent while the application is inactive
    /// and delivers it, merged, once the application is active again, so a
    /// lock that happened while another application was in front would be
    /// heard after the unlock. The setting belongs to the process's default
    /// centre; the previous one is put back on drop.
    struct DeliveryWhileInactive {
        center: Retained<NSDistributedNotificationCenter>,
        was_suspended: bool,
    }

    impl DeliveryWhileInactive {
        fn ask_for(center: &Retained<NSDistributedNotificationCenter>) -> Self {
            let was_suspended = center.suspended();
            center.setSuspended(false);
            Self {
                center: center.clone(),
                was_suspended,
            }
        }
    }

    impl Drop for DeliveryWhileInactive {
        fn drop(&mut self) {
            self.center.setSuspended(self.was_suspended);
        }
    }

    /// What listening holds: every source, removed when dropped.
    struct Listening {
        _observations: Vec<Observation>,
        _memory_pressure: MemoryPressureSource,
        _delivery: DeliveryWhileInactive,
    }

    /// The macOS sources of system notices, as the host's table builds them
    /// for the loop: they hand sleep, wake, lock and memory notices to the
    /// loop's channel and tell its control a session end is announced, once
    /// the loop starts them.
    pub(crate) struct SystemNoticeSources {
        notices: Weak<dyn NoticeChannel>,
        control: Weak<LoopControl>,
    }

    impl std::fmt::Debug for SystemNoticeSources {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SystemNoticeSources")
                .finish_non_exhaustive()
        }
    }

    impl SystemNoticeSources {
        /// The sources for the loop whose channel is `notices` and whose
        /// control is `control`; nothing listens until the loop starts them.
        pub(crate) fn new(notices: &Arc<dyn NoticeChannel>, control: &Arc<LoopControl>) -> Self {
            Self {
                notices: Arc::downgrade(notices),
                control: Arc::downgrade(control),
            }
        }
    }

    impl NoticeSources for SystemNoticeSources {
        fn listen(&self) -> Watch {
            let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
            let distributed = NSDistributedNotificationCenter::defaultCenter();
            let delivery = DeliveryWhileInactive::ask_for(&distributed);
            let distributed: Retained<NSNotificationCenter> = distributed.into_super();

            let forward = |kind: SystemNotice| {
                let notices = Weak::clone(&self.notices);
                move || {
                    if let Some(notices) = notices.upgrade() {
                        notices.dispatch(kind);
                    }
                }
            };
            let session_end = {
                let control = Weak::clone(&self.control);
                move || {
                    if let Some(control) = control.upgrade() {
                        control.session_end_announced();
                    }
                }
            };

            // SAFETY: the names are AppKit's immutable notification-name
            // constants. The workspace and distributed centres post them on the
            // main thread, where each handler forwards through weak handles.
            let observations = unsafe {
                vec![
                    Observation::new(
                        workspace.clone(),
                        NSWorkspaceWillSleepNotification,
                        forward(SystemNotice::Sleep),
                    ),
                    Observation::new(
                        workspace.clone(),
                        NSWorkspaceDidWakeNotification,
                        forward(SystemNotice::Wake),
                    ),
                    Observation::new(workspace, NSWorkspaceWillPowerOffNotification, session_end),
                    Observation::new(
                        distributed.clone(),
                        &NSString::from_str(SCREEN_LOCKED),
                        forward(SystemNotice::SessionLocked),
                    ),
                    Observation::new(
                        distributed,
                        &NSString::from_str(SCREEN_UNLOCKED),
                        forward(SystemNotice::SessionUnlocked),
                    ),
                ]
            };
            Watch::new(Listening {
                _observations: observations,
                _memory_pressure: MemoryPressureSource::new(Weak::clone(&self.notices)),
                _delivery: delivery,
            })
        }
    }
}

/// What a `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` source reports, as a
/// [`MemoryPressureLevel`](flui_foundation::memory::MemoryPressureLevel).
///
/// The source's data is a mask of the pressure states the system has entered
/// since the handler last ran.
#[cfg(any(target_os = "macos", test))]
#[forbid(unsafe_code)]
pub(crate) mod memory_pressure {
    use flui_foundation::memory::MemoryPressureLevel;

    /// `DISPATCH_MEMORYPRESSURE_NORMAL`: pressure ended.
    pub(crate) const NORMAL: usize = 0x01;
    /// `DISPATCH_MEMORYPRESSURE_WARN`: the system is low on memory.
    pub(crate) const WARN: usize = 0x02;
    /// `DISPATCH_MEMORYPRESSURE_CRITICAL`: the system is about to terminate
    /// processes.
    pub(crate) const CRITICAL: usize = 0x04;

    /// The states a source subscribes to. Pressure ending is subscribed to as
    /// well, so the handler runs for it too; it names no level (`level_of` is
    /// `None`), so the host is not told that pressure ended.
    pub(crate) const SUBSCRIBED: [usize; 3] = [NORMAL, WARN, CRITICAL];

    /// The level of pressure a source's data names; `None` for pressure ending
    /// or a mask with no state this crate knows.
    ///
    /// A mask can hold several states when the handler ran late; the worst
    /// wins.
    pub(crate) const fn level_of(flags: usize) -> Option<MemoryPressureLevel> {
        if flags & CRITICAL != 0 {
            Some(MemoryPressureLevel::Critical)
        } else if flags & WARN != 0 {
            Some(MemoryPressureLevel::Moderate)
        } else {
            None
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_subscribed_state_but_the_ending_is_a_level() {
            for state in SUBSCRIBED {
                assert_eq!(level_of(state).is_some(), state != NORMAL, "{state:#x}");
            }
        }

        #[test]
        fn a_source_mask_becomes_a_level() {
            // (name, mask, level)
            let rows = [
                ("critical", CRITICAL, Some(MemoryPressureLevel::Critical)),
                ("warn", WARN, Some(MemoryPressureLevel::Moderate)),
                (
                    "warn then critical before the handler ran",
                    WARN | CRITICAL,
                    Some(MemoryPressureLevel::Critical),
                ),
                (
                    "critical then back to normal",
                    CRITICAL | NORMAL,
                    Some(MemoryPressureLevel::Critical),
                ),
                (
                    "warn then back to normal",
                    WARN | NORMAL,
                    Some(MemoryPressureLevel::Moderate),
                ),
                ("pressure ended", NORMAL, None),
                ("nothing", 0, None),
                ("a state this crate does not know", 0x08, None),
            ];
            for (name, mask, expected) in rows {
                assert_eq!(level_of(mask), expected, "{name}");
            }
        }
    }
}
