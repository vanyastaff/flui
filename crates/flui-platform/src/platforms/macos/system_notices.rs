//! The macOS sources of system notices: the machine sleeping and waking, the
//! session locking and unlocking, memory pressure, and the announcement that
//! the user's session is about to end.
//!
//! Every source delivers on the main thread, which is the owner thread, and
//! only forwards: a notice goes to the platform's [`SystemNotices`] channel, a
//! session-end announcement to the loop control that answers the terminate
//! request. Each source is removed when [`SystemNoticeSources`] drops.

use std::ptr::NonNull;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use dispatch2::{
    _dispatch_source_type_memorypressure, DispatchObject, DispatchQueue, DispatchRetained,
    DispatchSource,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillPowerOffNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationName,
    NSObjectProtocol, NSString,
};

use flui_platform_api::SystemNotice;

use super::loop_control::LoopControl;
use crate::shared::{
    dispatch_memory_pressure::{SUBSCRIBED, level_of},
    panic_boundary::contain_owner_callback,
    system_notice::SystemNotices,
};

/// The distributed notification the lock screen posts when the session locks.
const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";
/// The distributed notification the lock screen posts when the session unlocks.
const SCREEN_UNLOCKED: &str = "com.apple.screenIsUnlocked";

/// One observer on a notification centre, removed on drop.
struct Observation {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    /// Cleared on drop, so a delivery already under way finds the source gone.
    active: Arc<AtomicBool>,
}

impl Observation {
    /// Run `handler` for each `name` that `center` posts, on the posting
    /// thread (the main thread for these centres).
    fn new(
        center: Retained<NSNotificationCenter>,
        name: &NSNotificationName,
        handler: impl Fn() + 'static,
    ) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let block = {
            let active = Arc::clone(&active);
            RcBlock::new(move |_: NonNull<NSNotification>| {
                contain_owner_callback(|| {
                    if active.load(Ordering::Acquire) {
                        handler();
                    }
                });
            })
        };
        // SAFETY: no object filter and no queue, so the block runs on the
        // posting thread, which is the main thread for the workspace and the
        // distributed centre alike. It ignores its argument and captures only
        // an atomic and the handler, which holds weak handles.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        Self {
            center,
            token,
            active,
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let token: &ProtocolObject<dyn NSObjectProtocol> = &self.token;
        // SAFETY: `token` is the exact observer this centre returned when it
        // was registered.
        unsafe {
            self.center.removeObserver(token.as_ref());
        }
    }
}

/// A memory-pressure dispatch source on the main queue.
struct MemoryPressureSource {
    source: DispatchRetained<DispatchSource>,
}

impl MemoryPressureSource {
    fn new(notices: Weak<SystemNotices>) -> Self {
        let mask = SUBSCRIBED.iter().fold(0, |mask, state| mask | state);
        // SAFETY: the memory-pressure source type is a constant exported by
        // libdispatch; the handle is unused for it; the mask is a union of the
        // states it defines.
        let source = unsafe {
            DispatchSource::new(
                (&raw const _dispatch_source_type_memorypressure).cast_mut(),
                0,
                mask,
                Some(DispatchQueue::main()),
            )
        };
        // The handler reads the data of the source it belongs to, so it holds
        // the source: the cycle ends when `drop` cancels it, after which
        // libdispatch releases the handler.
        let in_handler = source.clone();
        let handler = RcBlock::new(move || {
            contain_owner_callback(|| {
                if let Some(level) = level_of(in_handler.data())
                    && let Some(notices) = notices.upgrade()
                {
                    notices.dispatch(SystemNotice::MemoryPressure(level));
                }
            });
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
        // Cancelling is what ends the cycle between the source and its handler:
        // libdispatch releases a canceled source's handler, and with it the
        // clone of the source that the handler holds.
        self.source.cancel();
    }
}

/// The distributed notification centre delivering while the application is
/// inactive, for as long as this value lives.
///
/// The centre holds back what it is sent while the application is inactive and
/// delivers it, merged, once the application is active again, so a lock that
/// happened while another application was in front would be heard after the
/// unlock. The setting belongs to the process's default centre; the previous
/// one is put back on drop.
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

/// The platform's live sources of system notices.
pub(super) struct SystemNoticeSources {
    _observations: Vec<Observation>,
    _memory_pressure: MemoryPressureSource,
    _delivery: DeliveryWhileInactive,
}

impl SystemNoticeSources {
    /// Start every source. `notices` receives sleep, wake, lock and memory
    /// notices; `control` is told a session end is announced.
    pub(super) fn new(notices: &Arc<SystemNotices>, control: &Arc<LoopControl>) -> Self {
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        let delivery = DeliveryWhileInactive::ask_for(&distributed);
        let distributed: Retained<NSNotificationCenter> = distributed.into_super();

        let forward = |notices: &Arc<SystemNotices>, kind: SystemNotice| {
            let notices = Arc::downgrade(notices);
            move || {
                if let Some(notices) = notices.upgrade() {
                    notices.dispatch(kind);
                }
            }
        };
        let session_end = {
            let control = Arc::downgrade(control);
            move || {
                if let Some(control) = control.upgrade() {
                    control.session_end_announced();
                }
            }
        };

        // SAFETY: the names are AppKit's immutable notification-name constants.
        let observations = unsafe {
            vec![
                Observation::new(
                    workspace.clone(),
                    NSWorkspaceWillSleepNotification,
                    forward(notices, SystemNotice::Sleep),
                ),
                Observation::new(
                    workspace.clone(),
                    NSWorkspaceDidWakeNotification,
                    forward(notices, SystemNotice::Wake),
                ),
                Observation::new(workspace, NSWorkspaceWillPowerOffNotification, session_end),
                Observation::new(
                    distributed.clone(),
                    &NSString::from_str(SCREEN_LOCKED),
                    forward(notices, SystemNotice::SessionLocked),
                ),
                Observation::new(
                    distributed,
                    &NSString::from_str(SCREEN_UNLOCKED),
                    forward(notices, SystemNotice::SessionUnlocked),
                ),
            ]
        };
        Self {
            _observations: observations,
            _memory_pressure: MemoryPressureSource::new(Arc::downgrade(notices)),
            _delivery: delivery,
        }
    }
}
