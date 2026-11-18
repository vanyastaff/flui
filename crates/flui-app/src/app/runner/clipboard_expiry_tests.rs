//! An expiring clipboard entry through the desktop runner (ADR-0363): an
//! application run on the headless platform writes through the built-in
//! clipboard provider, and the runner clears the entry at the turn its turn
//! deadline gives, when the session locks, and at the loop's end. The run's
//! clipboard expiries are timed on a clock the test turns.

use std::rc::Rc;
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_interaction::{ClipboardCapability, ClipboardHandle};
use flui_platform::{ClipboardExpiry, ClipboardWrite, SystemNotice};
use flui_runtime::capability::HostCapabilities;

use crate::app::Application;
use crate::app::runner::host::APP_RUNTIME;
use crate::testing::{HeadlessRun, HeadlessRunHandle};

const SECRET: &str = "correct horse battery staple";

/// A run with its main window, its clipboard expiries timed on a manual clock.
fn start() -> (HeadlessRunHandle, ManualClock) {
    let run = HeadlessRun::start(Application::new(), flui_widgets::SizedBox::shrink());
    let clock = ManualClock::new();
    let expiries = APP_RUNTIME
        .with(|slot| slot.borrow().clipboard_expiries())
        .expect("a running loop holds its clipboard");
    expiries.set_clock(Rc::new(clock.clone()));
    (run, clock)
}

/// The clipboard as a widget acquires it, from the run's registry.
fn clipboard() -> (ClipboardHandle, HostCapabilities) {
    let registry = APP_RUNTIME
        .with(|slot| slot.borrow().capabilities.clone())
        .expect("a running loop holds its registry");
    let host = HostCapabilities::new(registry, || {});
    let handle = host
        .acquire::<ClipboardCapability>()
        .expect("the built-in clipboard is provided");
    (handle, host)
}

/// What the clipboard holds, read through `handle` and settled as an owner
/// turn settles it.
fn read(handle: &ClipboardHandle, host: &HostCapabilities) -> Option<String> {
    let mut completion = handle.read_text();
    // The shared read settles in two hops: the host's answer, then each
    // reader's.
    let answered = (0..4).find_map(|_| {
        host.run_posted().resume();
        completion.poll()
    });
    answered
        .expect("the in-memory clipboard answers at once")
        .expect("the read is answered, not abandoned")
        .expect("the in-memory clipboard reads")
        .as_deref()
        .map(str::to_owned)
}

fn expiring() -> ClipboardWrite {
    ClipboardWrite::text(SECRET)
        .expires_after(ClipboardExpiry::try_new(Duration::from_secs(30)).expect("a valid expiry"))
}

/// An expiring write through the built-in provider names its time as the
/// loop's turn deadline, and the owner's turn at that time clears the entry.
#[test]
fn an_expiring_write_clears_at_its_turn_deadline() {
    let (run, clock) = start();
    let (handle, host) = clipboard();
    let written_at = clock.now();
    drop(handle.write(expiring()));
    host.run_posted().resume();
    assert_eq!(
        run.turn_deadline(),
        Some(written_at + Duration::from_secs(30)),
        "the write's time is the loop's turn deadline"
    );
    clock.advance(Duration::from_secs(29));
    run.run_owner_turn_at(clock.now());
    assert_eq!(read(&handle, &host).as_deref(), Some(SECRET));
    clock.advance(Duration::from_secs(1));
    run.run_owner_turn_at(clock.now());
    host.run_posted().resume();
    assert_eq!(run.turn_deadline(), None, "a served clear is owed no more");
    assert_eq!(read(&handle, &host), None, "cleared at its time");
    run.join().expect("the run ends");
}

/// A session lock clears an expiring entry at once, and leaves an ordinary
/// one alone.
#[test]
fn a_session_lock_clears_an_expiring_entry() {
    let (run, _clock) = start();
    let (handle, host) = clipboard();
    drop(handle.write(expiring()));
    host.run_posted().resume();
    run.notice(SystemNotice::SessionLocked);
    run.run_owner_turn();
    host.run_posted().resume();
    assert_eq!(read(&handle, &host), None, "cleared at the lock");

    drop(handle.write(ClipboardWrite::text("ordinary")));
    host.run_posted().resume();
    run.notice(SystemNotice::Sleep);
    run.run_owner_turn();
    host.run_posted().resume();
    assert_eq!(
        read(&handle, &host).as_deref(),
        Some("ordinary"),
        "an ordinary entry stays"
    );
    run.join().expect("the run ends");
}

/// The loop's end clears an expiring entry still on the clipboard.
#[test]
fn the_loop_end_clears_an_expiring_entry() {
    let (run, _clock) = start();
    let (handle, host) = clipboard();
    drop(handle.write(expiring()));
    host.run_posted().resume();
    assert_eq!(read(&handle, &host).as_deref(), Some(SECRET));
    run.join().expect("the run ends");
    assert_eq!(read(&handle, &host), None, "cleared at the loop's end");
}
