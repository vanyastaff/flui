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
use flui_platform::{ClipboardError, ClipboardRead};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use super::X11Clipboard;
use crate::clipboard::linux::text::{CONCEALED_HINT, CONCEALED_HINT_VALUE};
use crate::host::NativeClipboard;
use crate::testing::clipboard::a_concealed_write_stays_out_of_history;

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
    fn read(&self, clipboard: &dyn NativeClipboard) -> Option<Arc<str>> {
        let (mut completion, resolver): (Completion<ClipboardRead>, _) =
            self.source.pending::<ClipboardRead>();
        clipboard.read_text(resolver);
        let deadline = Instant::now() + STEP;
        loop {
            let jobs = std::mem::take(&mut *self.queue.lock().expect("the answers"));
            let scope = OwnerScope::new(&self.pending);
            for job in jobs {
                job.run(&scope);
            }
            if let Some(outcome) = completion.poll() {
                return outcome
                    .expect("the read is answered, not abandoned")
                    .expect("the X11 read succeeds");
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
        assert_eq!(answers.read(&clipboard), Some(Arc::from(text)));
    }
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
/// read is refused at once rather than queue behind them; the reads
/// waiting go on waiting.
#[test]
fn a_read_past_the_waiting_limit_is_refused_at_once() {
    let Some(_server) = server() else {
        return;
    };
    let _owner = silent_owner();
    let answers = Answers::new();
    let reader = X11Clipboard::start().expect("the X server answers");
    let mut waiting: Vec<Completion<ClipboardRead>> = Vec::new();
    for _ in 0..super::selection::MAX_READERS {
        let (completion, resolver) = answers.source.pending::<ClipboardRead>();
        reader.read_text(resolver);
        waiting.push(completion);
    }
    let (mut past, resolver) = answers.source.pending::<ClipboardRead>();
    reader.read_text(resolver);
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
    assert_eq!(
        outcome.expect("the read is answered"),
        Err(ClipboardError::Unavailable)
    );
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
        let reader = X11Clipboard::start().expect("the X server answers");
        let (_first, answer) = source.pending::<ClipboardRead>();
        reader.read_text(answer);
        let (_second, answer) = source.pending::<ClipboardRead>();
        reader.read_text(answer);
        drop(reader);
        assert_eq!(
            posts.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "each waiting read is abandoned"
        );
    });
}

/// Ask the `CLIPBOARD` selection's owner for `target` as another client
/// would, and answer what it wrote: its type and bytes, or `None` when it
/// refused.
fn convert(target: &[u8]) -> Option<(xproto::Atom, Vec<u8>)> {
    let (requestor, screen) = RustConnection::connect(None).expect("the X server answers");
    let root = requestor.setup().roots[screen].root;
    let window = requestor.generate_id().expect("an id");
    requestor
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            xproto::WindowClass::INPUT_ONLY,
            x11rb::COPY_FROM_PARENT,
            &xproto::CreateWindowAux::new(),
        )
        .expect("the request is sent");
    let atom = |name: &[u8]| {
        requestor
            .intern_atom(false, name)
            .expect("the request is sent")
            .reply()
            .expect("the server answers")
            .atom
    };
    let (clipboard, target, property) = (atom(b"CLIPBOARD"), atom(target), atom(b"FLUI_TEST"));
    requestor
        .convert_selection(window, clipboard, target, property, x11rb::CURRENT_TIME)
        .expect("the request is sent");
    requestor.flush().expect("the request is sent");
    let deadline = Instant::now() + STEP;
    let answered = loop {
        match requestor.poll_for_event().expect("the connection holds") {
            Some(x11rb::protocol::Event::SelectionNotify(notify)) => {
                break notify.property != x11rb::NONE;
            }
            Some(_) => {}
            None => {
                assert!(Instant::now() < deadline, "the owner never answered");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    if !answered {
        return None;
    }
    let reply = requestor
        .get_property(true, window, property, xproto::AtomEnum::ANY, 0, 1 << 16)
        .expect("the request is sent")
        .reply()
        .expect("the server answers");
    Some((reply.type_, reply.value))
}

/// The atoms of a `TARGETS` answer.
fn targets() -> Vec<xproto::Atom> {
    let (_, bytes) = convert(b"TARGETS").expect("the owner answers TARGETS");
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|atom| u32::from_ne_bytes(*atom))
        .collect::<Vec<xproto::Atom>>()
}

/// The atom `name` names on the server.
fn atom(name: &str) -> xproto::Atom {
    let (connection, _) = RustConnection::connect(None).expect("the X server answers");
    connection
        .intern_atom(false, name.as_bytes())
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .atom
}

/// A concealed write pastes back, and the selection offers KDE's
/// password-manager hint beside its text, which a clipboard manager reads as
/// "keep it out of the history"; an ordinary write after it offers none.
#[test]
fn a_concealed_write_offers_the_password_manager_hint() {
    let Some(_server) = server() else {
        return;
    };
    let answers = Answers::new();
    let clipboard = X11Clipboard::start().expect("the X server answers");
    let hint = atom(CONCEALED_HINT);
    assert_eq!(
        a_concealed_write_stays_out_of_history(&clipboard, |clipboard| answers.read(clipboard)),
        Ok(())
    );
    assert!(targets().contains(&hint), "TARGETS offers the hint");
    assert_eq!(
        convert(CONCEALED_HINT.as_bytes()),
        Some((hint, CONCEALED_HINT_VALUE.to_vec()))
    );

    clipboard.write_text("plain".to_owned());
    assert_eq!(answers.read(&clipboard).as_deref(), Some("plain"));
    assert!(
        !targets().contains(&hint),
        "an ordinary write offers no hint"
    );
    assert_eq!(convert(CONCEALED_HINT.as_bytes()), None);
}
