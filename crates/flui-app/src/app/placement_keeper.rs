//! Where a window was, kept in the host storage and restored on the next run
//! (ADR-0237 decision 6).
//!
//! [`recall`] reads the stored [`WindowPlacement`] before a window opens and
//! [`restore_into`] fits it to the displays of this run, so the first reveal is
//! already where the window belongs. A [`PlacementKeeper`] then follows the
//! window's published state and writes the placement back: at most once every
//! [`WRITE_INTERVAL`] while it changes, and at once when the window closes or
//! the session ends. A change inside the interval is held and written by the
//! next of those, or by the first change after the interval.

use std::cell::RefCell;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use flui_platform::WindowOptions;
use flui_platform_api::{
    DisplaySnapshot, Storage, StorageName, WindowCapabilities, WindowMode, WindowPlacement,
    WindowState, WriteMode,
};

/// The shortest time between two writes of a window that keeps changing.
pub(crate) const WRITE_INTERVAL: Duration = Duration::from_secs(2);

/// How long a closing window or an ending session waits for the final write to
/// land before giving up on it.
pub(crate) const FLUSH_BUDGET: Duration = Duration::from_secs(4);

/// A placement is a few hundred bytes; a stored value past this is not one.
const READ_LIMIT: u64 = 4096;

/// The placement stored under `name`, if there is one this build can read.
///
/// Blocks the calling thread on the storage read, once, before the first
/// window opens. A missing value is the normal first run; a value that cannot
/// be read or decoded is reported and the window opens from its configuration.
pub(crate) fn recall(storage: &dyn Storage, name: &StorageName) -> Option<WindowPlacement> {
    let stored = match pollster::block_on(storage.read(name, READ_LIMIT)) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, storage = name.as_str(), "the stored window placement cannot be read");
            return None;
        }
    };
    match WindowPlacement::decode(&stored.bytes?) {
        Ok(placement) => Some(placement),
        Err(error) => {
            tracing::warn!(%error, storage = name.as_str(), "the stored window placement is not usable");
            None
        }
    }
}

/// Open `options` where `placement` was, fitted to `displays`.
///
/// Size, position and mode come from the placement; everything else stays the
/// configuration's. The window's capabilities are not known before it exists,
/// so every one is assumed: a backend that cannot place or maximize a window
/// ignores that part of the options, as `WindowOptions` documents.
pub(crate) fn restore_into(
    options: &mut WindowOptions,
    placement: &WindowPlacement,
    displays: &DisplaySnapshot,
) {
    let resolved = placement.resolve(displays, &WindowCapabilities::all());
    options.size = resolved.size;
    options.position = resolved.position;
    options.mode = resolved.mode;
}

/// Follows one window's published state and writes its placement to storage.
pub(crate) struct PlacementKeeper {
    storage: Arc<dyn Storage>,
    name: StorageName,
    now: Box<dyn Fn() -> Instant>,
    displays: Box<dyn Fn() -> DisplaySnapshot>,
    book: RefCell<Book>,
    /// The window-state subscription that feeds [`Self::observe`], held for as
    /// long as the keeper lives.
    subscription: RefCell<Option<flui_view::WindowSubscription>>,
}

#[derive(Default)]
struct Book {
    /// The placement last captured; `None` until the window reports one.
    latest: Option<Vec<u8>>,
    /// The bytes last handed to storage, or read from it.
    written: Option<Vec<u8>>,
    last_write: Option<Instant>,
    /// Whether the window was maximized before it was last minimized, so a
    /// minimized window comes back maximized.
    maximized_before: bool,
}

impl PlacementKeeper {
    /// A keeper writing to `storage` under `name`, which already holds
    /// `recalled` when the window opened from it.
    pub(crate) fn new(
        storage: Arc<dyn Storage>,
        name: StorageName,
        recalled: Option<&WindowPlacement>,
        now: impl Fn() -> Instant + 'static,
        displays: impl Fn() -> DisplaySnapshot + 'static,
    ) -> Self {
        let book = Book {
            written: recalled.map(WindowPlacement::encode),
            ..Book::default()
        };
        Self {
            storage,
            name,
            now: Box::new(now),
            displays: Box::new(displays),
            book: RefCell::new(book),
            subscription: RefCell::new(None),
        }
    }

    /// Keep `subscription` alive as long as the keeper.
    pub(crate) fn hold(&self, subscription: flui_view::WindowSubscription) {
        *self.subscription.borrow_mut() = Some(subscription);
    }

    /// Take in the window's newest state, writing it when [`WRITE_INTERVAL`]
    /// has passed since the last write.
    pub(crate) fn observe(&self, state: &WindowState) {
        let mut book = self.book.borrow_mut();
        match state.mode {
            WindowMode::Maximized => book.maximized_before = true,
            WindowMode::Normal | WindowMode::Fullscreen => book.maximized_before = false,
            _ => {}
        }
        let Some(placement) =
            WindowPlacement::capture(state, &(self.displays)(), book.maximized_before)
        else {
            return;
        };
        book.latest = Some(placement.encode());
        let now = (self.now)();
        let due = book
            .last_write
            .is_none_or(|at| now.saturating_duration_since(at) >= WRITE_INTERVAL);
        if due {
            self.write(&mut book, now).detach();
        }
    }

    /// Write the newest placement now, if it differs from the last one written,
    /// and wait up to [`FLUSH_BUDGET`] for storage to take it.
    pub(crate) fn flush(&self) {
        let pending = {
            let mut book = self.book.borrow_mut();
            let now = (self.now)();
            self.write(&mut book, now)
        };
        pending.wait(FLUSH_BUDGET);
    }

    /// Hand the newest placement to storage when it is new.
    fn write(&self, book: &mut Book, now: Instant) -> Write {
        let Some(bytes) = book.latest.clone() else {
            return Write(None);
        };
        if book.written.as_ref() == Some(&bytes) {
            return Write(None);
        }
        book.written = Some(bytes.clone());
        book.last_write = Some(now);
        Write(Some(self.storage.publish(
            &self.name,
            bytes,
            WriteMode::Replace,
        )))
    }
}

/// A write storage has accepted and may not have finished.
struct Write(Option<flui_platform_api::StorageFuture<flui_platform_api::StoredVersion>>);

impl Write {
    /// Let storage finish the write on its own; the next write supersedes it.
    fn detach(self) {
        drop(self);
    }

    /// Wait for the write for at most `budget`, and report a failure.
    fn wait(self, budget: Duration) {
        let Some(mut future) = self.0 else {
            return;
        };
        let deadline = Instant::now() + budget;
        let waker = Waker::from(Arc::new(Unpark(thread::current())));
        let mut cx = Context::from_waker(&waker);
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(Ok(_)) => return,
                Poll::Ready(Err(error)) => {
                    tracing::warn!(%error, "the window placement was not stored");
                    return;
                }
                Poll::Pending => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        tracing::warn!("the window placement was not stored in time");
                        return;
                    }
                    thread::park_timeout(left);
                }
            }
        }
    }
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

#[cfg(test)]
#[path = "placement_keeper_tests.rs"]
mod tests;
