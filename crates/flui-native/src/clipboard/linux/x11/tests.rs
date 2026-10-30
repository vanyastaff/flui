//! The X11 clipboard against a real X server: two clipboards, each on a
//! connection and a thread of its own, exchange the selections through it
//! as two applications would.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the
//! tests skip without it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_platform::capability::{
    Completion, CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope,
    PendingCompletions, Resolver,
};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::X11Clipboard;
use crate::host::{NativeClipboard, NativePrimarySelection};
use crate::owner::linux::x11::LoopClipboard as _;

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

/// A clipboard whose owner's loop is the case itself: it turns by calling
/// `deliver` while it waits, so the wake has nothing to ring.
fn clipboard() -> X11Clipboard {
    X11Clipboard::start(Box::new(|| {})).expect("the X server answers")
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
        self.read_with(
            |answer| NativeClipboard::read_text(clipboard, answer),
            || {},
        )
    }

    /// Read the primary selection through `reader`, turning the loops of
    /// `owners` while it waits.
    fn read_primary(&self, reader: &X11Clipboard, owners: &[&X11Clipboard]) -> Option<String> {
        self.read_with(
            |answer| NativePrimarySelection::read_text(reader, answer),
            || owners.iter().for_each(|owner| owner.deliver()),
        )
    }

    /// Start a read with `start` and wait for its answer, calling `turn`
    /// for the owners' loops meanwhile.
    fn read_with(
        &self,
        start: impl FnOnce(Resolver<Option<String>>),
        mut turn: impl FnMut(),
    ) -> Option<String> {
        let (mut completion, resolver): (Completion<Option<String>>, _) =
            self.source.pending::<Option<String>>();
        start(resolver);
        let deadline = Instant::now() + STEP;
        loop {
            turn();
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
    let first = clipboard();
    let second = clipboard();

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
    let owner = clipboard();
    let reader = clipboard();
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
    let reader = clipboard();
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
    let clipboard = clipboard();
    clipboard.write_text("before".to_owned());
    assert_eq!(answers.read(&clipboard).as_deref(), Some("before"));
    for round in 0..20 {
        let text = format!("after {round}");
        clipboard.write_text(text.clone());
        assert_eq!(answers.read(&clipboard), Some(text));
    }
}

/// A closure over `text` that counts how often it is asked.
fn counted(text: &str, asked: &Rc<Cell<u32>>) -> Rc<dyn Fn() -> Option<String>> {
    let text = text.to_owned();
    let asked = Rc::clone(asked);
    Rc::new(move || {
        asked.set(asked.get() + 1);
        Some(text.clone())
    })
}

/// The application's primary selection is asked for its text only when it
/// is read: by another client through the server, at the owner's turn, and
/// by the application itself at once.
#[test]
fn the_primary_selection_is_asked_only_when_read() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let owner = clipboard();
    let reader = clipboard();
    let asked = Rc::new(Cell::new(0));

    owner.offer(counted("selected", &asked));
    assert!(owner.owns());
    assert_eq!(asked.get(), 0, "an offer asks nothing");
    assert_eq!(
        answers.read_primary(&reader, &[&owner]).as_deref(),
        Some("selected")
    );
    assert_eq!(asked.get(), 1);
    assert_eq!(
        answers.read_primary(&owner, &[]).as_deref(),
        Some("selected")
    );
    assert_eq!(asked.get(), 2);
}

/// Another client's selection takes the primary selection from the
/// application's offer, whose closure is dropped at the owner's next turn.
#[test]
fn another_selection_takes_the_primary_from_the_offer() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let first = clipboard();
    let second = clipboard();
    let asked = Rc::new(Cell::new(0));
    let first_text = counted("from the first", &asked);
    let held = Rc::downgrade(&first_text);

    first.offer(first_text);
    assert_eq!(
        answers.read_primary(&second, &[&first]).as_deref(),
        Some("from the first")
    );
    second.offer(counted("from the second", &asked));
    let deadline = Instant::now() + STEP;
    while first.owns() {
        assert!(
            Instant::now() < deadline,
            "the first never heard its selection was taken"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    first.deliver();
    assert_eq!(held.strong_count(), 0, "the turn drops the taken closure");
    assert_eq!(
        answers.read_primary(&first, &[&second]).as_deref(),
        Some("from the second")
    );
}

/// A withdrawn offer leaves no primary selection: a read answers nothing,
/// and does not wait for its timeout.
#[test]
fn a_withdrawn_offer_reads_as_nothing() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let owner = clipboard();
    let reader = clipboard();
    let asked = Rc::new(Cell::new(0));

    owner.offer(counted("selected", &asked));
    assert_eq!(
        answers.read_primary(&reader, &[&owner]).as_deref(),
        Some("selected")
    );
    owner.withdraw();
    assert!(!owner.owns());
    let started = Instant::now();
    assert_eq!(answers.read_primary(&reader, &[&owner]), None);
    assert!(started.elapsed() < super::selection::TRANSFER_TIMEOUT);
    assert_eq!(asked.get(), 1, "a withdrawn offer is not asked");
}

/// A closure that fails while another client reads the selection answers
/// that client with nothing at once, and its failure goes on from the turn.
#[test]
fn a_failing_selection_answers_its_reader_with_nothing() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let owner = clipboard();
    let reader = clipboard();
    owner.offer(Rc::new(|| panic!("the selection failed")));

    let failure = std::cell::RefCell::new(None);
    let started = Instant::now();
    let read = answers.read_with(
        |answer| NativePrimarySelection::read_text(&reader, answer),
        || {
            let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.deliver()));
            if let Err(payload) = turn {
                failure.borrow_mut().get_or_insert(payload);
            }
        },
    );
    assert_eq!(read, None);
    assert!(
        started.elapsed() < super::selection::TRANSFER_TIMEOUT,
        "the reader is answered as the closure fails, not at the timeout"
    );
    let failure = failure.into_inner().expect("the turn failed");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("the selection failed")
    );
}

/// A client that owns `CLIPBOARD` and never answers, so reads of it wait;
/// it owns the selection while the connection lives.
fn silent_owner() -> RustConnection {
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
    owner
}

/// Past [`MAX_READERS`](super::selection::MAX_READERS) reads waiting, a
/// read answers nothing at once rather than wait for another copy of the
/// text; the reads waiting go on waiting.
#[test]
fn a_read_past_the_waiting_limit_answers_nothing_at_once() {
    let Some(_server) = server() else {
        return;
    };
    let _owner = silent_owner();
    let answers = Answers::new();
    let reader = clipboard();
    let mut waiting: Vec<Completion<Option<String>>> = Vec::new();
    for _ in 0..super::selection::MAX_READERS {
        let (completion, resolver) = answers.source.pending::<Option<String>>();
        NativeClipboard::read_text(&reader, resolver);
        waiting.push(completion);
    }
    let (mut past, resolver) = answers.source.pending::<Option<String>>();
    NativeClipboard::read_text(&reader, resolver);
    let deadline = Instant::now() + super::selection::TRANSFER_TIMEOUT / 2;
    let outcome = loop {
        let jobs = std::mem::take(&mut *answers.queue.lock().expect("the answers"));
        let scope = OwnerScope::new(&answers.pending);
        for job in jobs {
            job.run(&scope);
        }
        if let Some(outcome) = past.poll() {
            break outcome;
        }
        assert!(
            Instant::now() < deadline,
            "the read past the limit waited with the others"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(outcome.expect("the read is answered"), None);
    for completion in &mut waiting {
        assert!(completion.poll().is_none(), "a waiting read was answered");
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
        let _owner = silent_owner();
        let posts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&posts);
        let poster = OwnerPoster::new(move |_| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            panic!("the owner's queue failed")
        });
        let source = CompletionSource::new(poster, PendingCompletions::new());
        let reader = clipboard();
        let (_first, answer) = source.pending::<Option<String>>();
        NativeClipboard::read_text(&reader, answer);
        let (_second, answer) = source.pending::<Option<String>>();
        NativeClipboard::read_text(&reader, answer);
        drop(reader);
        assert_eq!(
            posts.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "each waiting read is abandoned"
        );
    });
}
