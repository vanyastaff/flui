//! One observer on a notification centre, removed on drop.

use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol,
};

use crate::shared::panic_boundary::contain_owner_callback;

pub(super) struct Observation {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    /// Cleared on drop, so a delivery already under way finds the source gone.
    active: Arc<AtomicBool>,
}

impl Observation {
    /// Run `handler` for each `name` that `center` posts, on the posting
    /// thread: the main thread for the workspace, application and distributed
    /// centres, and possibly another for the default centre, which is why the
    /// handler must be `Send + Sync`.
    pub(super) fn new(
        center: Retained<NSNotificationCenter>,
        name: &NSNotificationName,
        handler: impl Fn() + Send + Sync + 'static,
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
        // posting thread; it ignores its argument and captures only an atomic
        // and the handler, both sound on any thread (`Send + Sync`).
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
