//! The process-wide lock every Win32 clipboard session in FLUI runs under.
//!
//! `OpenClipboard(NULL)` does **not** exclude other threads of the same
//! process: Win32 keys the "clipboard is open" check on the owner window,
//! and all `NULL`-owner openers count as the same owner. While thread A
//! holds the clipboard and scans the `CF_UNICODETEXT` handle through
//! `GlobalLock`, thread B's `OpenClipboard(NULL)` therefore succeeds, its
//! `EmptyClipboard` frees that handle, and A reads freed memory — the
//! process dies with `STATUS_HEAP_CORRUPTION` (`0xc0000374`). Both FLUI
//! backends that can run on Windows open with a `NULL` owner: the Win32
//! backend's `WindowsClipboard` and, through `arboard`, the winit backend's
//! `ArboardClipboard`. Either may exist in several instances on several
//! threads, so a per-instance lock cannot close the race; this one can.
//!
//! Not covered: third-party code in the same process that opens the
//! clipboard on its own thread bypasses this lock (`docs/safety-review.md`).

use parking_lot::{Mutex, MutexGuard};

static CLIPBOARD: Mutex<()> = Mutex::new(());

/// Proof that the caller holds the process-wide clipboard lock.
pub(crate) type SessionLock = MutexGuard<'static, ()>;

/// Blocks until no other FLUI clipboard session in this process is open.
/// Hold the returned lock from before `OpenClipboard` until after
/// `CloseClipboard`. Not reentrant.
pub(crate) fn acquire() -> SessionLock {
    CLIPBOARD.lock()
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
