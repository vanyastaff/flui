//! The process-wide lock every Win32 clipboard session in FLUI runs under.
//!
//! `OpenClipboard(NULL)` does **not** exclude other threads of the same
//! process: Win32 keys the "clipboard is open" check on the owner window,
//! and all `NULL`-owner openers count as the same owner. While thread A
//! holds the clipboard and scans the `CF_UNICODETEXT` handle through
//! `GlobalLock`, thread B's `OpenClipboard(NULL)` therefore succeeds, its
//! `EmptyClipboard` frees that handle, and A reads freed memory — the
//! process dies with `STATUS_HEAP_CORRUPTION` (`0xc0000374`). The winit
//! backend's `ArboardClipboard` opens with a `NULL` owner (inside `arboard`);
//! the Win32 backend's `WindowsClipboard` opens with its own owner window,
//! which Win32 does not let a `NULL` opener share, but a `NULL`-owner
//! `arboard` session would still let a `WindowsClipboard` session in beside
//! it. Either may exist in several instances on several threads, so a
//! per-instance lock cannot close the race; this one can.
//!
//! Not covered: third-party code in the same process that opens the
//! clipboard on its own thread bypasses this lock. It is shut out of a
//! `WindowsClipboard` session by the owner window, but not out of an
//! `ArboardClipboard` one (`docs/safety-review.md`).

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

/// Runs `session` — a call that opens and closes the clipboard itself, such
/// as one `arboard` operation — under the process-wide clipboard lock.
#[cfg(feature = "winit-backend")]
pub(crate) fn with_clipboard_session<R>(session: impl FnOnce() -> R) -> R {
    let _lock = acquire();
    session()
}

/// Serializes tests that assert a write → read round-trip through the one
/// system clipboard, so parallel test threads do not read each other's text.
/// Covers in-process runs (`cargo test`); under nextest, where every test is
/// its own process, the `system-clipboard` test group in
/// `.config/nextest.toml` does the same job.
#[cfg(test)]
pub(crate) fn round_trip_serial() -> MutexGuard<'static, ()> {
    static ROUND_TRIP: Mutex<()> = Mutex::new(());
    ROUND_TRIP.lock()
}

#[cfg(test)]
mod tests {
    use std::thread;

    use crate::{platforms::windows::WindowsClipboard, traits::Clipboard};

    /// Enough rounds that, without the lock, every run on a developer
    /// machine aborted the process; each round is one open..close session.
    const ROUNDS: usize = 500;

    /// Races a reader against a writer on two threads. Completing at all is
    /// the assertion: the failure mode is the process aborting.
    fn race_reader_against_writer(
        reader: impl Clipboard + 'static,
        writer: impl Clipboard + 'static,
    ) {
        let _serial = super::round_trip_serial();
        let writer = thread::spawn(move || {
            for round in 0..ROUNDS {
                writer.write_text(format!("round {round}: Привет 🌍"));
            }
        });
        let reader = thread::spawn(move || {
            for _ in 0..ROUNDS {
                let _ = reader.read_text();
            }
        });
        writer.join().expect("clipboard writer thread panicked");
        reader.join().expect("clipboard reader thread panicked");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "not reentrant")]
    fn a_nested_acquire_on_one_thread_panics_instead_of_deadlocking() {
        let _outer = super::acquire();
        let _inner = super::acquire();
    }

    #[test]
    fn releasing_the_lock_lets_the_same_thread_take_it_again() {
        drop(super::acquire());
        drop(super::acquire());
    }

    #[test]
    fn win32_reader_and_writer_on_two_threads_do_not_corrupt_the_heap() {
        race_reader_against_writer(WindowsClipboard::new(), WindowsClipboard::new());
    }

    #[cfg(feature = "winit-backend")]
    #[test]
    fn arboard_writer_and_win32_reader_on_two_threads_do_not_corrupt_the_heap() {
        let arboard = crate::platforms::winit::ArboardClipboard::new()
            .expect("arboard always reaches the Win32 clipboard");
        race_reader_against_writer(WindowsClipboard::new(), arboard);
    }

    #[cfg(feature = "winit-backend")]
    #[test]
    fn arboard_reader_and_writer_on_two_threads_do_not_corrupt_the_heap() {
        use crate::platforms::winit::ArboardClipboard;
        let open = || ArboardClipboard::new().expect("arboard always reaches the Win32 clipboard");
        race_reader_against_writer(open(), open());
    }
}
