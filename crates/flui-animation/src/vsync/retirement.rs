//! Terminal clock withdrawal precedes cancellation and opaque retirement.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::{RegisteredChild, RegisteredController, Vsync, VsyncRegistration};
use crate::animation::{Retirement, Terminal};
use crate::controller::ValuePublication;
use flui_foundation::panic::RecoveryScope;

struct OutgoingRegistry {
    controllers: BTreeMap<u64, RegisteredController>,
    children: Vec<RegisteredChild>,
    requester: Option<Rc<dyn Fn()>>,
}

/// Cancellation delivery after a registry and its descendants are closed.
///
/// Preparation withdraws every seat and closes every kernel before any
/// callback. No registry borrow survives. Finish after the presentation's other
/// capabilities are withdrawn. Drop also completes accepted cancellation;
/// during unwind it preserves the incoming failure and retains opaque captures.
#[must_use = "finish cancellation after withdrawing presentation authority"]
pub struct VsyncRetirement {
    recovery: Option<Retirement>,
    publications: Vec<Terminal<ValuePublication>>,
    outgoing: Vec<Terminal<OutgoingRegistry>>,
}

impl std::fmt::Debug for VsyncRetirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VsyncRetirement")
            .finish_non_exhaustive()
    }
}

impl Vsync {
    /// Permanently close this registry and every attached descendant identity.
    ///
    /// Seats, parent links and frame hooks are withdrawn, and controller kernels
    /// refuse new runs before this returns. Saved handles cannot revive admission.
    /// Closing a shared descendant closes that identity for all its parents.
    /// Cancellation callbacks and outgoing capture retirement are deferred to the
    /// returned receipt; repeated preparation is an empty, idempotent retirement.
    pub fn prepare_close(&self) -> VsyncRetirement {
        let mut retirement = VsyncRetirement {
            recovery: None,
            publications: Vec::new(),
            outgoing: Vec::new(),
        };
        let mut recovery = Retirement::new();
        recovery.run(|| self.withdraw(&mut retirement));
        retirement.recovery = Some(recovery);
        retirement
    }

    fn withdraw(&self, retirement: &mut VsyncRetirement) {
        let (outgoing, parents) = {
            let mut inner = self.inner.borrow_mut();
            if inner.closed {
                return;
            }
            inner.closed = true;
            (
                Terminal::new(OutgoingRegistry {
                    controllers: std::mem::take(&mut inner.controllers),
                    children: std::mem::take(&mut inner.children),
                    requester: inner.request_frame.take(),
                }),
                std::mem::take(&mut inner.parents),
            )
        };
        // `self` and outgoing child custody keep the removed registries strong;
        // detaching these framework links cannot destroy a last user capture.
        for parent in parents {
            if let Some(owner) = parent.owner.upgrade() {
                Vsync { inner: owner }.detach_child(&parent);
            }
        }
        for (slot, registered) in &outgoing.controllers {
            registered
                .controller
                .remove_frame_route(&VsyncRegistration {
                    owner: Rc::downgrade(&self.inner),
                    slot: *slot,
                });
            if let Some(publication) = registered.controller.prepare_dispose() {
                retirement.publications.push(Terminal::new(publication));
            }
        }
        for child in &outgoing.children {
            child.child.withdraw(retirement);
        }
        retirement.outgoing.push(outgoing);
    }
}

impl VsyncRetirement {
    /// Deliver accepted cancellations and retire outgoing ownership.
    ///
    /// # Panics
    /// Resumes the first preparation, callback or capture retirement failure
    /// after delivering the healthy tail.
    pub fn finish(mut self) {
        let mut recovery = self
            .recovery
            .take()
            .expect("BUG: registry retirement owns recovery");
        self.deliver(&mut recovery.scope());
        recovery.finish();
    }

    /// Publish under the enclosing framework owner's first-failure custody.
    #[doc(hidden)]
    pub fn publish_with(mut self, recovery: &mut RecoveryScope<'_>) {
        let prepared = self
            .recovery
            .take()
            .expect("BUG: registry retirement owns recovery");
        recovery.run(|| prepared.finish());
        self.deliver(recovery);
    }

    fn deliver(&mut self, recovery: &mut RecoveryScope<'_>) {
        for publication in self.publications.drain(..) {
            recovery.run_with(|scope| publication.into_inner().publish(scope));
        }
        for outgoing in self.outgoing.drain(..) {
            // Separate opaque owners: one destructor failure cannot drop the
            // remaining aggregate before first-failure custody takes control.
            let OutgoingRegistry {
                controllers,
                children,
                requester,
            } = outgoing.into_inner();
            for (_, controller) in controllers {
                recovery.retire(Terminal::new(controller));
            }
            for child in children {
                recovery.retire(Terminal::new(child));
            }
            recovery.retire(Terminal::new(requester));
        }
    }
}

impl Drop for VsyncRetirement {
    fn drop(&mut self) {
        let Some(mut recovery) = self.recovery.take() else {
            return;
        };
        if std::thread::panicking() {
            // An unwinding failure owns priority over a preparation failure.
            let outgoing = std::mem::replace(&mut recovery, Retirement::new());
            drop(outgoing);
        }
        self.deliver(&mut recovery.scope());
        recovery.finish();
    }
}
