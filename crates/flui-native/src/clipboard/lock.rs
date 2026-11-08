//! The process-wide lock every Win32 clipboard session in FLUI runs under.
//!
//! `OpenClipboard` does **not** exclude other threads of the same process
//! that open with the same owner: Win32 keys the "clipboard is open" check on
//! the owner window. Every `WindowsClipboard` opens with the one owner window
//! of the process, so while thread A holds the clipboard and scans the
//! `CF_UNICODETEXT` handle through `GlobalLock`, thread B's `OpenClipboard`
//! would succeed, its `EmptyClipboard` would free that handle, and A would
//! read freed memory — the process dies with `STATUS_HEAP_CORRUPTION`
//! (`0xc0000374`). Instances exist on several threads, so a per-instance lock
//! cannot close the race; this one can.
//!
//! Third-party code in the same process bypasses this lock. With a non-NULL
//! owner, Win32 excludes it from a `WindowsClipboard` session by the distinct
//! owner window; the logged NULL-owner fallback cannot provide that exclusion.

use parking_lot::{Mutex, MutexGuard};

static CLIPBOARD: Mutex<()> = Mutex::new(());

/// Proof that the caller holds the process-wide clipboard lock; releases it
/// on drop.
pub(crate) struct SessionLock {
    _guard: MutexGuard<'static, ()>,
    #[cfg(debug_assertions)]
    _reentrancy: reentrancy::Marker,
}

/// Blocks until no other FLUI clipboard session in this process is open.
/// Hold the returned lock from before `OpenClipboard` until after
/// `CloseClipboard`.
///
/// Not reentrant: acquiring again on a thread that already holds it would
/// deadlock silently, so debug builds panic on that instead.
pub(crate) fn acquire() -> SessionLock {
    #[cfg(debug_assertions)]
    let reentrancy = reentrancy::Marker::enter();
    SessionLock {
        _guard: CLIPBOARD.lock(),
        #[cfg(debug_assertions)]
        _reentrancy: reentrancy,
    }
}

/// Debug-build detection of a nested [`acquire`] on one thread.
#[cfg(debug_assertions)]
mod reentrancy {
    use std::cell::Cell;

    thread_local! {
        static SESSION_OPEN: Cell<bool> = const { Cell::new(false) };
    }

    /// Marks this thread as holding the clipboard lock until dropped.
    pub(super) struct Marker;

    impl Marker {
        /// Runs before the lock is taken, so a nested acquire panics
        /// instead of blocking on the lock its own thread holds.
        pub(super) fn enter() -> Self {
            let already_open = SESSION_OPEN.with(|open| open.replace(true));
            assert!(
                !already_open,
                "clipboard session lock acquired again on the thread that holds it:                  the lock is not reentrant, so this would deadlock"
            );
            Self
        }
    }

    impl Drop for Marker {
        fn drop(&mut self) {
            SESSION_OPEN.with(|open| open.set(false));
        }
    }
}
