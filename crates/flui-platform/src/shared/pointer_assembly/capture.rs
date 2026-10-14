//! Native recording capture (ADR-0215 §4), in debug builds with the
//! `native-capture` feature.
//!
//! [`capture`] installs a sink on the calling thread. Every
//! [`ContactRegistry`](super::ContactRegistry) created on that thread while
//! it is installed — a backend's window registry included — reports each
//! call that shapes its output: its clock and setup, each reading or
//! coalesced batch, device hot-plug and cancellation. That journal is a
//! native recording: replaying it through the same calls reproduces the
//! input the backend delivered. The sink writes it; the codec lives in this
//! crate's tests, so the backend links no serializer.
//!
//! A capture is identified by its session, not by an index: a registry
//! reports to the capture that was installed when it was created, for as
//! long as that capture's guard lives, even after another capture replaced
//! it for newer registries. Calls are reported before the registry acts on
//! them, so a sink that panics leaves the registry unchanged; the panic ends
//! that capture. Calls a sink causes from inside itself are not reported.

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::pointer::PointerKind;
use parking_lot::{Mutex, ReentrantMutex};

use super::{
    ClickSettings, CounterAnchor, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface,
};

/// One registry call, as a native recording holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Captured<'a> {
    /// The registry was created on a clock with these anchors.
    Started {
        /// The epoch instant.
        epoch: EventTime,
        /// System uptime at the epoch.
        uptime: Duration,
        /// The performance counter at the epoch, when the host has one.
        counter: Option<CounterAnchor>,
    },
    /// `contact` is given pointer identity one.
    ReservedContact(NativeContact),
    /// Pointer identities are minted from this one.
    FirstPointer(NonZeroU64),
    /// Device identities are minted from this one.
    FirstDevice(NonZeroU64),
    /// The OS click thresholds changed.
    ClickSettings(Option<ClickSettings>),
    /// One reading was decoded.
    Reading(&'a NativeReading),
    /// A batch was decoded: readings coalesced before `current`, oldest first,
    /// and the ones predicted after it.
    Batch {
        /// The coalesced readings.
        coalesced: &'a [NativeReading],
        /// The predicted readings.
        predicted: &'a [NativeReading],
        /// The reading the batch decodes as.
        current: &'a NativeReading,
    },
    /// A device was announced.
    AddDevice {
        /// Its label.
        device: NativeDevice,
        /// Its kind.
        kind: PointerKind,
        /// When.
        time: EventTime,
    },
    /// A device was retired.
    RemoveDevice {
        /// Its label.
        device: NativeDevice,
        /// When.
        time: EventTime,
    },
    /// Every contact on a surface ended.
    CancelSurface {
        /// The surface.
        surface: NativeSurface,
        /// When.
        time: EventTime,
        /// Why.
        reason: NativeCancel,
    },
    /// The contacts labelled `contact` on a surface ended.
    CancelContact {
        /// The surface.
        surface: NativeSurface,
        /// The label.
        contact: NativeContact,
        /// When.
        time: EventTime,
        /// Why.
        reason: NativeCancel,
    },
}

/// Which registry of a capture a call came from: registries are numbered in
/// the order they were created while the capture was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureSource(pub u32);

type Sink = Box<dyn FnMut(CaptureSource, Captured<'_>) + Send>;

/// One capture. A registry may move to another thread (the headless
/// backend's is shared), so its calls can arrive from any thread: the lock
/// serializes them in call order, and is reentrant so a call the sink causes
/// on its own thread finds the sink out and is skipped instead of
/// deadlocking.
struct Session {
    sink: ReentrantMutex<RefCell<Option<Sink>>>,
    next_source: Mutex<Option<u32>>,
}

thread_local! {
    /// The capture new registries on this thread report to. Exists only in
    /// debug builds with `native-capture`; no behavior reads it.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static INSTALLED: RefCell<Weak<Session>> = const { RefCell::new(Weak::new()) };
}

/// Keeps a capture installed; dropping it ends the capture for every
/// registry that reports to it.
#[must_use = "the capture ends when its guard is dropped"]
pub struct CaptureGuard {
    session: Arc<Session>,
}

impl std::fmt::Debug for CaptureGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureGuard").finish_non_exhaustive()
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let own = Arc::downgrade(&self.session);
        INSTALLED.with(|installed| {
            let mut installed = installed.borrow_mut();
            if installed.ptr_eq(&own) {
                *installed = Weak::new();
            }
        });
    }
}

/// Report the calls of every registry created on this thread from now on to
/// `sink`, until the guard is dropped. A later capture replaces this one for
/// registries created after it.
pub fn capture(sink: impl FnMut(CaptureSource, Captured<'_>) + Send + 'static) -> CaptureGuard {
    let session = Arc::new(Session {
        sink: ReentrantMutex::new(RefCell::new(Some(Box::new(sink)))),
        next_source: Mutex::new(Some(0)),
    });
    INSTALLED.with(|installed| *installed.borrow_mut() = Arc::downgrade(&session));
    CaptureGuard { session }
}

/// A registry's link to the capture it reports to, if any.
#[derive(Debug, Default)]
pub(super) struct Journal {
    session: Option<(Weak<Session>, CaptureSource)>,
}

impl Journal {
    /// The journal of a registry created now on `clock`, which it reports
    /// first: numbered within the installed capture, or reporting nowhere
    /// when none is installed or the capture has numbered every source it
    /// can.
    pub(super) fn open(clock: NativeClock) -> Self {
        let session = INSTALLED.with(|installed| installed.borrow().clone());
        let source = session.upgrade().and_then(|live| {
            let mut next = live.next_source.lock();
            let source = (*next)?;
            *next = source.checked_add(1);
            Some(CaptureSource(source))
        });
        let journal = Self {
            session: source.map(|source| (session, source)),
        };
        let (epoch, uptime, counter) = clock.anchors();
        journal.record(Captured::Started {
            epoch,
            uptime,
            counter,
        });
        journal
    }

    /// Report `call` to the capture, if it is still installed. The sink is
    /// moved out while it runs, so a call it causes is not reported, and a
    /// panic drops it.
    pub(super) fn record(&self, call: Captured<'_>) {
        let Some((session, source)) = &self.session else {
            return;
        };
        let Some(session) = session.upgrade() else {
            return;
        };
        let slot = session.sink.lock();
        let Some(mut sink) = slot.borrow_mut().take() else {
            return;
        };
        sink(*source, call);
        *slot.borrow_mut() = Some(sink);
    }
}
