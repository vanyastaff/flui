//! The X11 clipboard against a real X server: two clipboards, each on a
//! connection and a thread of its own, exchange the selection through it as
//! two applications would.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the
//! tests skip without it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_platform::capability::{
    Completion, CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope,
    PendingCompletions,
};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::X11Clipboard;
use crate::host::NativeClipboard as _;

/// How long a read may take before the case fails.
const STEP: Duration = Duration::from_secs(10);

/// The selection is the server's, one for every client: the cases take
/// turns at it. The connection held here keeps the server from resetting
/// between cases, as Xvfb does once its last client leaves.
static SERVER: Mutex<Option<RustConnection>> = Mutex::new(None);

/// The server's selection for this case alone, or `None` with no X server
/// to run against.
fn server() -> Option<std::sync::MutexGuard<'static, Option<RustConnection>>> {
    let present = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
    if !present {
        eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
        return None;
    }
    let mut server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if server.is_none() {
        let (connection, _) = RustConnection::connect(None).expect("the X server answers");
        *server = Some(connection);
    }
    Some(server)
}

/// The completions a read answers through, settled by hand as an
/// application's runtime settles them.
struct Answers {
    queue: Arc<Mutex<Vec<OwnerJob>>>,
    pending: PendingCompletions,
    source: CompletionSource,
}

impl Answers {
    fn new() -> Self {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&queue);
        let poster = OwnerPoster::new(move |job| {
            sink.lock().map_err(|_| OwnerRetired)?.push(job);
            Ok(())
        });
        let pending = PendingCompletions::new();
        Self {
            queue,
            source: CompletionSource::new(poster, pending.clone()),
            pending,
        }
    }

    /// Read `clipboard`, waiting for its answer.
    fn read(&self, clipboard: &X11Clipboard) -> Option<String> {
        let (mut completion, resolver): (Completion<Option<String>>, _) =
            self.source.pending::<Option<String>>();
        clipboard.read_text(resolver);
        let deadline = Instant::now() + STEP;
        loop {
            let jobs = std::mem::take(&mut *self.queue.lock().expect("the answers"));
            let scope = OwnerScope::new(&self.pending);
            for job in jobs {
                job.run(&scope);
            }
            if let Some(outcome) = completion.poll() {
                return outcome.expect("the read is answered, not abandoned");
            }
            assert!(Instant::now() < deadline, "timed out waiting for the read");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// One clipboard writes and reads its own text back; a second reads that
/// text through the server and writes its own, which the first then reads.
#[test]
fn the_clipboard_crosses_between_connections() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let first = X11Clipboard::start().expect("the X server answers");
    let second = X11Clipboard::start().expect("the X server answers");

    first.write_text("from the first".to_owned());
    assert_eq!(answers.read(&first).as_deref(), Some("from the first"));
    assert_eq!(answers.read(&second).as_deref(), Some("from the first"));

    second.write_text("от второго".to_owned());
    // The second owns it once its write took: until then a read of the first
    // may still answer its own text.
    let deadline = Instant::now() + STEP;
    while answers.read(&first).as_deref() != Some("от второго") {
        assert!(
            Instant::now() < deadline,
            "the first never read the second's text"
        );
    }
}

/// Text larger than one request crosses whole, in `INCR` chunks.
#[test]
fn a_large_selection_crosses_in_chunks() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let owner = X11Clipboard::start().expect("the X server answers");
    let reader = X11Clipboard::start().expect("the X server answers");
    let text: String = (0..300_000_u32)
        .map(|at| char::from(b'a' + u8::try_from(at % 26).expect("BUG: below 26")))
        .chain("конец".chars())
        .collect();

    owner.write_text(text.clone());
    assert_eq!(answers.read(&owner).as_deref(), Some(text.as_str()));
    assert_eq!(answers.read(&reader).as_deref(), Some(text.as_str()));
}

/// A selection nobody owns reads as nothing, and the read does not wait
/// for its timeout.
#[test]
fn a_selection_nobody_owns_reads_as_nothing() {
    let Some(_server) = server() else {
        return;
    };
    let (connection, _) = RustConnection::connect(None).expect("the X server answers");
    let clipboard_atom = connection
        .intern_atom(false, b"CLIPBOARD")
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .atom;
    // Whoever owns it now lets go: no window owns it.
    connection
        .set_selection_owner(x11rb::NONE, clipboard_atom, x11rb::CURRENT_TIME)
        .expect("the request is sent");
    connection.flush().expect("the request is sent");
    connection
        .get_selection_owner(clipboard_atom)
        .expect("the request is sent")
        .reply()
        .expect("the server answers");

    let answers = Answers::new();
    let reader = X11Clipboard::start().expect("the X server answers");
    let started = Instant::now();
    assert_eq!(answers.read(&reader), None);
    assert!(
        started.elapsed() < super::selection::TRANSFER_TIMEOUT,
        "a refused conversion answers at once"
    );
}

/// A read sent right after a write answers the written text, not the one
/// the clipboard owned before, though both reach its thread together.
#[test]
fn a_read_after_a_write_answers_the_write() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let clipboard = X11Clipboard::start().expect("the X server answers");
    clipboard.write_text("before".to_owned());
    assert_eq!(answers.read(&clipboard).as_deref(), Some("before"));
    for round in 0..20 {
        let text = format!("after {round}");
        clipboard.write_text(text.clone());
        assert_eq!(answers.read(&clipboard), Some(text));
    }
}

/// Set in the child process a teardown case runs in.
const CHILD: &str = "FLUI_X11_CLIPBOARD_TEARDOWN_CHILD";

/// Run `case` in a child of this test binary: a second panic during an
/// unwind aborts the process, which only a child can report.
fn in_child(test: &str, case: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        case();
        return;
    }
    let path = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, path)| path);
    let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", &format!("{path}::{test}"), "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .expect("the child runs");
    assert!(
        output.status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Reads still waiting when the clipboard drops are each abandoned, though
/// the abandonment of the first fails: the thread contains each.
#[test]
fn reads_waiting_at_the_end_are_each_abandoned() {
    let Some(_server) = server() else {
        return;
    };
    in_child("reads_waiting_at_the_end_are_each_abandoned", || {
        // An owner that never answers, so the reads wait.
        let (owner, screen) = RustConnection::connect(None).expect("the X server answers");
        let root = owner.setup().roots[screen].root;
        let window = owner.generate_id().expect("an id");
        owner
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                x11rb::protocol::xproto::WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &x11rb::protocol::xproto::CreateWindowAux::new(),
            )
            .expect("the request is sent");
        let clipboard_atom = owner
            .intern_atom(false, b"CLIPBOARD")
            .expect("the request is sent")
            .reply()
            .expect("the server answers")
            .atom;
        owner
            .set_selection_owner(window, clipboard_atom, x11rb::CURRENT_TIME)
            .expect("the request is sent");
        owner
            .get_selection_owner(clipboard_atom)
            .expect("the request is sent")
            .reply()
            .expect("the server answers");

        let posts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&posts);
        let poster = OwnerPoster::new(move |_| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            panic!("the owner's queue failed")
        });
        let source = CompletionSource::new(poster, PendingCompletions::new());
        let reader = X11Clipboard::start().expect("the X server answers");
        let (_first, answer) = source.pending::<Option<String>>();
        reader.read_text(answer);
        let (_second, answer) = source.pending::<Option<String>>();
        reader.read_text(answer);
        drop(reader);
        assert_eq!(
            posts.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "each waiting read is abandoned"
        );
    });
}
