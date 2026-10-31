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

/// `owner` offers `text` as the primary selection, and the case waits until
/// the server names an owner: the offer is the application's at once, but its
/// thread takes the selection at the server's next stamp, and a read sent
/// before reaches whoever owned it then. Whoever owned it is let go first.
fn offer_primary(owner: &X11Clipboard, text: Rc<dyn Fn() -> Option<String>>) {
    let (connection, _) = RustConnection::connect(None).expect("the X server answers");
    let primary: x11rb::protocol::xproto::Atom = x11rb::protocol::xproto::AtomEnum::PRIMARY.into();
    connection
        .set_selection_owner(x11rb::NONE, primary, x11rb::CURRENT_TIME)
        .expect("the request is sent");
    connection
        .get_input_focus()
        .expect("the request is sent")
        .reply()
        .expect("the server answers");
    owner.offer(text);
    let deadline = Instant::now() + STEP;
    while connection
        .get_selection_owner(primary)
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .owner
        == x11rb::NONE
    {
        assert!(
            Instant::now() < deadline,
            "the offer never took the primary selection"
        );
        std::thread::sleep(Duration::from_millis(2));
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

    offer_primary(&owner, counted("selected", &asked));
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

    offer_primary(&first, first_text);
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

    offer_primary(&owner, counted("selected", &asked));
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

/// An offer withdrawn before it owns the primary selection lets go of the
/// older offer it replaced, which owned it: no client goes on asking the
/// application for text it no longer offers.
#[test]
fn withdrawing_a_replacement_lets_go_of_the_offer_it_replaced() {
    let Some(server) = server() else {
        return;
    };
    let server = server.as_ref().expect("the server's connection is held");
    let answers = Answers::new();
    let owner = clipboard();
    let reader = clipboard();
    let asked = Rc::new(Cell::new(0));

    offer_primary(&owner, counted("first", &asked));
    assert_eq!(
        answers.read_primary(&reader, &[&owner]).as_deref(),
        Some("first")
    );
    owner.offer(counted("second", &asked));
    owner.withdraw();
    let primary = server
        .intern_atom(false, b"PRIMARY")
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .atom;
    let deadline = Instant::now() + STEP;
    loop {
        let held = server
            .get_selection_owner(primary)
            .expect("the request is sent")
            .reply()
            .expect("the server answers")
            .owner;
        if held == x11rb::NONE {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the replaced offer still owns the primary selection"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
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
    offer_primary(&owner, Rc::new(|| panic!("the selection failed")));

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

/// The application's own read of its selection, whose closure fails, keeps
/// that failure though the owner's queue fails too as the read is answered:
/// the answer is posted under containment of its own, not dropped during the
/// unwind, where a second panic would abort.
#[test]
fn an_own_read_whose_selection_fails_keeps_the_first_failure() {
    let Some(_server) = server() else {
        return;
    };
    in_child(
        "an_own_read_whose_selection_fails_keeps_the_first_failure",
        || {
            let posts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counted = Arc::clone(&posts);
            let poster = OwnerPoster::new(move |_| {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                panic!("the owner's queue failed")
            });
            let source = CompletionSource::new(poster, PendingCompletions::new());
            let owner = clipboard();
            owner.offer(Rc::new(|| panic!("the selection failed")));
            let (_completion, answer) = source.pending::<Option<String>>();
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                NativePrimarySelection::read_text(&owner, answer);
            }))
            .expect_err("the read fails");
            assert_eq!(
                flui_foundation::panic::payload_text(&*failure),
                Some("the selection failed")
            );
            assert_eq!(
                posts.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "the read is answered once"
            );
        },
    );
}

/// While a requestor holds every `INCR` transfer, its further requests for
/// the primary selection are refused without asking the application's
/// closure: holding the writers and asking again costs the application no
/// work.
#[test]
fn requests_while_every_transfer_is_held_do_not_ask_the_selection() {
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    let Some(_server) = server() else {
        return;
    };
    let owner = clipboard();
    let asked = Rc::new(Cell::new(0));
    // Longer than one request: each answer is an `INCR` transfer.
    offer_primary(&owner, counted(&"x".repeat(1 << 20), &asked));

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
            WindowClass::INPUT_ONLY,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .expect("the request is sent");
    let intern = |name: &str| {
        requestor
            .intern_atom(false, name.as_bytes())
            .expect("the request is sent")
            .reply()
            .expect("the server answers")
            .atom
    };
    let utf8 = intern("UTF8_STRING");
    let incr = intern("INCR");
    // Ask for the text into `property` and wait for the answer's property,
    // turning the owner's loop meanwhile.
    let request = |property: u32| -> u32 {
        requestor
            .convert_selection(
                window,
                x11rb::protocol::xproto::AtomEnum::PRIMARY.into(),
                utf8,
                property,
                x11rb::CURRENT_TIME,
            )
            .expect("the request is sent");
        requestor.flush().expect("the request is sent");
        let deadline = Instant::now() + STEP;
        loop {
            owner.deliver();
            while let Some(event) = requestor.poll_for_event().expect("the connection holds") {
                if let Event::SelectionNotify(notify) = event {
                    return notify.property;
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the answer"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };

    for held in 0..super::selection::MAX_WRITERS {
        let property = intern(&format!("FLUI_HELD_{held}"));
        assert_eq!(request(property), property, "transfer {held} starts");
        let announced = requestor
            .get_property(false, window, property, incr, 0, 1)
            .expect("the request is sent")
            .reply()
            .expect("the server answers");
        assert_eq!(announced.type_, incr, "transfer {held} goes INCR");
        // The property is never deleted: the transfer stays held.
    }
    let writers = u32::try_from(super::selection::MAX_WRITERS).expect("a few writers");
    assert_eq!(asked.get(), writers);

    for refused in 0..3 {
        let property = intern(&format!("FLUI_REFUSED_{refused}"));
        assert_eq!(
            request(property),
            x11rb::NONE,
            "request {refused} is refused"
        );
    }
    assert_eq!(asked.get(), writers, "the refused requests ask nothing");
}
