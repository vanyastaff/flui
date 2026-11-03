//! The X11 text input against a stand-in XIM server on the test's X server.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the
//! tests that need an X server skip without it. The stand-in is the
//! transport's own server end: it registers under a name of the test's,
//! answers the handshake, and serves its input contexts by a script keyed
//! on the key's code: one key it commits for and swallows, one it sends
//! back, one it composes, one it never answers.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_platform::WindowId;
use flui_platform::text_store::{TextStore, TextStoreHost};
use flui_testing::text_store_kit::InMemoryTextStore;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use web_time::Instant;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, ConnectionExt as _, CreateWindowAux, FocusInEvent, NotifyDetail, NotifyMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::link::server::Listener;
use super::link::{Display, Received};
use super::wire::{AttrDecl, Message};
use super::{
    PREEDIT_CALLBACKS, PREEDIT_NOTHING, STATUS_NOTHING, XimReach, keysym_char, server_name,
    styles_of,
};
use crate::owner::linux::x11::{LoopText, WindowRef, WindowScale, X11Key};
use crate::text::TextInput;
use crate::text::linux::input_context::ANSWER_TIMEOUT;
use crate::text::linux::x11::bus::{Bus, Reach};
use crate::text::linux::x11::{WindowText, X11Ime};

/// The key the stand-in commits "あ" for, and swallows.
const COMMITS: u8 = 38;
/// The key it sends back, not taken.
const LETS_THROUGH: u8 = 56;
/// The key it composes "にほ" for, then redraws the second character.
const COMPOSES: u8 = 33;
/// The key it never answers.
const HOLDS: u8 = 43;
/// The key it inserts into the preedit for, past any preedit held.
const FLOODS: u8 = 44;

/// "あ" and "ん" as COMPOUND_TEXT: UTF-8 inside its escape.
const A: &[u8] = &[0x1b, 0x25, 0x47, 0xe3, 0x81, 0x82, 0x1b, 0x25, 0x40];
const NIHO: &[u8] = &[
    0x1b, 0x25, 0x47, 0xe3, 0x81, 0xab, 0xe3, 0x81, 0xbb, 0x1b, 0x25, 0x40,
];
const N: &[u8] = &[0x1b, 0x25, 0x47, 0xe3, 0x82, 0x93, 0x1b, 0x25, 0x40];

/// What the stand-in heard, as lines.
type Heard = Arc<Mutex<Vec<String>>>;

/// What a test has the stand-in do apart from its script.
enum Control {
    /// Go: destroy its communication window.
    Go,
    /// Send raw `_XIM_PROTOCOL` bytes, a message that reads as nothing.
    Raw([u8; 20]),
}

/// A name no other test's stand-in has.
fn unique_name() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "flui-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn has_display() -> bool {
    let present = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
    if !present {
        eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
    }
    present
}

/// Serve the client the listener accepts, by the script, until it goes or
/// the test says to.
async fn stand_in(listener: Listener, heard: Heard, mut control: mpsc::UnboundedReceiver<Control>) {
    let Some(mut link) = listener.accept().await else {
        heard.lock().push("no client".to_owned());
        return;
    };
    let mut made = 0u16;
    loop {
        let received = tokio::select! {
            control = control.recv() => {
                match control {
                    Some(Control::Go) | None => {
                        link.close();
                        return;
                    }
                    Some(Control::Raw(bytes)) => {
                        link.send_raw(b"_XIM_PROTOCOL", bytes);
                    }
                }
                continue;
            }
            received = link.receive() => received,
        };
        let bytes = match received {
            Received::Message(bytes) => bytes,
            Received::Other => continue,
            Received::Gone => return,
        };
        let Ok(message) = Message::decode(&bytes) else {
            heard.lock().push("unreadable".to_owned());
            continue;
        };
        for reply in script(&message, &mut made, &heard) {
            link.send(&reply.encode());
        }
    }
}

/// The stand-in's answers to `message`.
fn script(message: &Message, made: &mut u16, heard: &Heard) -> Vec<Message> {
    let decl = |id: u16, name: &[u8]| AttrDecl {
        id,
        kind: 0,
        name: name.to_vec(),
    };
    match message {
        Message::Connect => vec![Message::ConnectReply],
        Message::Open { .. } => vec![Message::OpenReply {
            im: 1,
            im_attrs: vec![decl(0, b"queryInputStyle")],
            ic_attrs: vec![
                decl(0, b"inputStyle"),
                decl(1, b"clientWindow"),
                decl(2, b"focusWindow"),
                decl(3, b"preeditAttributes"),
                decl(4, b"spotLocation"),
            ],
        }],
        Message::EncodingNegotiation { im } => {
            vec![Message::EncodingNegotiationReply { im: *im, index: 0 }]
        }
        Message::GetImValues { im, ids } => {
            let mut styles = Vec::new();
            styles.extend_from_slice(&2u16.to_ne_bytes());
            styles.extend_from_slice(&[0, 0]);
            styles.extend_from_slice(&(PREEDIT_NOTHING | STATUS_NOTHING).to_ne_bytes());
            styles.extend_from_slice(&(PREEDIT_CALLBACKS | STATUS_NOTHING).to_ne_bytes());
            vec![Message::GetImValuesReply {
                im: *im,
                values: ids
                    .iter()
                    .map(|id| super::wire::AttrValue {
                        id: *id,
                        value: styles.clone(),
                    })
                    .collect(),
            }]
        }
        Message::CreateIc { im, values } => {
            *made += 1;
            let style = values
                .iter()
                .find(|value| value.id == 0)
                .and_then(|value| value.value.get(..4))
                .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            heard
                .lock()
                .push(format!("create {made} style {style:x?}", made = *made));
            vec![
                Message::CreateIcReply { im: *im, ic: *made },
                Message::SetEventMask {
                    im: *im,
                    ic: *made,
                    forward: 0x3,
                    synchronous: 0,
                },
            ]
        }
        Message::SetIcFocus { ic, .. } => {
            heard.lock().push(format!("focus in {ic}"));
            Vec::new()
        }
        Message::UnsetIcFocus { ic, .. } => {
            heard.lock().push(format!("focus out {ic}"));
            Vec::new()
        }
        Message::DestroyIc { im, ic } => {
            heard.lock().push(format!("destroy {ic}"));
            vec![Message::DestroyIcReply { im: *im, ic: *ic }]
        }
        Message::ResetIc { im, ic } => {
            heard.lock().push(format!("reset {ic}"));
            vec![Message::ResetIcReply {
                im: *im,
                ic: *ic,
                text: Vec::new(),
            }]
        }
        Message::SetIcValues { im, ic, values } => {
            for value in values {
                // preeditAttributes holding spotLocation: id, length, x, y.
                if value.id == 3 && value.value.len() >= 8 {
                    let x = i16::from_ne_bytes([value.value[4], value.value[5]]);
                    let y = i16::from_ne_bytes([value.value[6], value.value[7]]);
                    heard.lock().push(format!("spot {x} {y}"));
                }
            }
            vec![Message::SetIcValuesReply { im: *im, ic: *ic }]
        }
        Message::ForwardEvent { im, ic, event, .. } => {
            let (im, ic) = (*im, *ic);
            let release = event[0] == super::wire::KEY_RELEASE;
            heard.lock().push(format!(
                "key {} {}",
                event[1],
                if release { "up" } else { "down" }
            ));
            if release {
                return Vec::new();
            }
            match event[1] {
                COMMITS => vec![Message::Commit {
                    im,
                    ic,
                    synchronous: false,
                    keysym: None,
                    text: Some(A.to_vec()),
                }],
                LETS_THROUGH => vec![Message::ForwardEvent {
                    im,
                    ic,
                    flag: 0,
                    serial: 0,
                    event: *event,
                }],
                COMPOSES => vec![
                    Message::PreeditStart { im, ic },
                    Message::PreeditDraw {
                        im,
                        ic,
                        caret: 2,
                        first: 0,
                        length: 0,
                        status: 0,
                        text: NIHO.to_vec(),
                    },
                    Message::PreeditDraw {
                        im,
                        ic,
                        caret: 2,
                        first: 1,
                        length: 1,
                        status: 0,
                        text: N.to_vec(),
                    },
                ],
                FLOODS => (0..3)
                    .map(|_| Message::PreeditDraw {
                        im,
                        ic,
                        caret: 0,
                        first: 0,
                        length: 0,
                        status: 0,
                        text: vec![b'x'; 2000],
                    })
                    .collect(),
                HOLDS => {
                    heard.lock().push("holding".to_owned());
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        Message::Sync { im, ic } => {
            let holding = heard.lock().last().is_some_and(|line| line == "holding");
            if holding {
                Vec::new()
            } else {
                vec![Message::SyncReply { im: *im, ic: *ic }]
            }
        }
        Message::PreeditStartReply { .. } | Message::SyncReply { .. } => Vec::new(),
        other => {
            heard.lock().push(format!("other {other:?}"));
            Vec::new()
        }
    }
}

/// The keys delivered to the window, each with the field's text as it went.
type Delivered = Rc<RefCell<Vec<(u8, String)>>>;

/// The text input with one window on the test's X server, its field
/// `store`, served by a stand-in XIM server.
struct Rig {
    runtime: tokio::runtime::Runtime,
    conn: RustConnection,
    ime: X11Ime,
    window: WindowRef,
    text: Arc<WindowText>,
    store: Rc<InMemoryTextStore>,
    heard: Heard,
    delivered: Delivered,
    control: mpsc::UnboundedSender<Control>,
}

impl Rig {
    fn start() -> Option<Self> {
        if !has_display() {
            return None;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let (conn, screen) = x11rb::connect(None).expect("the test's X server");
        let root = conn.setup().roots[screen].root;
        let xid = conn.generate_id().expect("an id");
        conn.create_window(
            0,
            xid,
            root,
            0,
            0,
            200,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .expect("a window");
        conn.flush().expect("the window made");
        let name = unique_name();
        let heard = Heard::default();
        let (control, controls) = mpsc::unbounded_channel();
        let listener = runtime.block_on(async {
            Listener::start(Display::connect(None).expect("a connection"), &name)
                .expect("the stand-in registers")
        });
        runtime.spawn(stand_in(listener, Arc::clone(&heard), controls));
        let reach = Reach::Xim(XimReach {
            display: None,
            name,
        });
        let ime = X11Ime::with_bus(None, Bus::start(runtime.handle(), reach, Box::new(|| {})));
        let window = WindowRef {
            xid,
            id: WindowId::try_from_raw(1).expect("a nonzero window id"),
        };
        let text = Arc::new(WindowText::new());
        ime.window_opened(window, WindowScale::new(1.0), Some(Arc::clone(&text) as _));
        let store = InMemoryTextStore::new("");
        text.client()
            .expect("the window's client")
            .focus_store(Some(Rc::clone(&store) as Rc<dyn TextStore>));
        let rig = Self {
            runtime,
            conn,
            ime,
            window,
            text,
            store,
            heard,
            delivered: Delivered::default(),
            control,
        };
        rig.ime.event(&Event::FocusIn(FocusInEvent {
            response_type: xproto::FOCUS_IN_EVENT,
            detail: NotifyDetail::NONLINEAR,
            sequence: 0,
            event: rig.window.xid,
            mode: NotifyMode::NORMAL,
        }));
        rig.settle("the server hears the focus", || rig.heard_any("focus in"));
        Some(rig)
    }

    /// Run the owner's turns until `done`, failing after five seconds.
    fn settle(&self, what: &str, done: impl Fn() -> bool) {
        self.settle_at(None, what, done);
    }

    /// Run the owner's turns as at `now`, which no held key's deadline is
    /// past: only the server's answers let a key go.
    fn settle_answered(&self, now: Instant, what: &str, done: impl Fn() -> bool) {
        self.settle_at(Some(now), what, done);
    }

    fn settle_at(&self, now: Option<Instant>, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.ime.deliver(now.unwrap_or_else(Instant::now));
            if done() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out: {what}; heard {:?}",
                self.heard.lock()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn heard_any(&self, prefix: &str) -> bool {
        self.heard
            .lock()
            .iter()
            .any(|heard| heard.starts_with(prefix))
    }

    /// A press of the key `keycode`, delivered to the window with the
    /// field's text as it goes.
    fn press(&self, keycode: u8) {
        let delivered = Rc::clone(&self.delivered);
        let store = Rc::clone(&self.store);
        self.ime.key(X11Key {
            window: self.window,
            keysym: u32::from(b'a'),
            keycode,
            state: 0,
            release: false,
            time: 0,
            deliver: Box::new(move || delivered.borrow_mut().push((keycode, store.text()))),
        });
    }

    fn delivered(&self) -> Vec<(u8, String)> {
        self.delivered.borrow().clone()
    }

    fn control(&self, control: Control) {
        self.control.send(control).expect("the stand-in runs");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.ime.release();
        self.text.detach();
        let _ = self.conn.destroy_window(self.window.xid);
        let _ = self.conn.flush();
        let _ = &self.runtime;
    }
}

#[test]
fn a_context_is_made_in_the_style_that_draws_the_preedit_in_the_field() {
    let Some(rig) = Rig::start() else {
        return;
    };
    assert!(
        rig.heard_any("create 1 style Some(402)"),
        "on-the-spot preedit, no status: {:?}",
        rig.heard.lock()
    );
}

#[test]
fn a_commit_reaches_the_field_and_a_key_sent_back_is_typed_after_it() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let pressed = Instant::now();
    rig.press(COMMITS);
    rig.press(LETS_THROUGH);
    rig.settle_answered(pressed, "the key sent back", || !rig.delivered().is_empty());
    assert_eq!(
        rig.delivered(),
        [(LETS_THROUGH, "あ".to_owned())],
        "the key the server took is not typed, and the one after finds the commit"
    );
    assert_eq!(rig.store.text(), "あ");
}

#[test]
fn a_preedit_the_server_draws_composes_in_the_field() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press(COMPOSES);
    rig.settle("the preedit redrawn", || rig.store.text() == "にん");
    assert!(rig.store.composition().is_some());
    assert!(rig.delivered().is_empty(), "the key composed is not typed");
}

#[test]
fn a_key_the_server_holds_goes_as_typed_at_the_deadline() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let pressed = Instant::now();
    rig.press(HOLDS);
    rig.settle("the held key", || rig.delivered().len() == 1);
    assert!(pressed.elapsed() >= ANSWER_TIMEOUT);
    assert_eq!(rig.delivered(), [(HOLDS, String::new())]);
}

#[test]
fn the_caret_reaches_the_server_as_the_spot() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.settle("the spot", || rig.heard_any("spot "));
}

/// A server that goes, or that sends what no message reads as, ends the
/// link without a panic: its context is lost, and a key typed then goes as
/// typed at once rather than at a deadline.
#[test]
fn a_server_gone_or_misbehaving_leaves_keys_typed() {
    let mut header = [0u8; 20];
    // XIM_COMMIT claiming a body far past the twenty bytes sent.
    header[0] = 63;
    header[2..4].copy_from_slice(&u16::MAX.to_ne_bytes());
    for (case, control) in [("gone", Control::Go), ("unreadable", Control::Raw(header))] {
        let Some(rig) = Rig::start() else {
            return;
        };
        rig.control(control);
        rig.settle(case, || {
            rig.press(COMMITS);
            !rig.delivered().is_empty()
        });
        let typed = rig.delivered().len();
        rig.press(COMMITS);
        rig.ime.deliver(Instant::now());
        assert_eq!(rig.delivered().len(), typed + 1, "{case}: typed at once");
        assert_eq!(rig.store.text(), "", "{case}: nothing committed");
    }
}

#[test]
fn xmodifiers_name_the_server() {
    for (modifiers, name) in [
        ("@im=fcitx", Some("fcitx")),
        ("@im=ibus", Some("ibus")),
        ("@im=kinput2@other=x", Some("kinput2")),
        ("@im=none", None),
        ("@im=", None),
        ("", None),
    ] {
        assert_eq!(server_name(modifiers), name, "{modifiers:?}");
    }
}

#[test]
fn styles_listed_by_the_server_are_read_within_their_value() {
    let mut value = Vec::new();
    value.extend_from_slice(&3u16.to_ne_bytes());
    value.extend_from_slice(&[0, 0]);
    value.extend_from_slice(&0x402u32.to_ne_bytes());
    // A count past the styles there: only those there are read.
    assert_eq!(styles_of(&value), [0x402]);
    assert_eq!(styles_of(&[1]), Vec::<u32>::new());
}

#[test]
fn a_keysym_committed_alone_is_its_character() {
    assert_eq!(keysym_char(0x61), Some('a'));
    assert_eq!(keysym_char(0x0100_3042), Some('あ'));
    assert_eq!(keysym_char(0xff0d), None, "Return commits nothing");
}

/// A server that only ever inserts into the preedit has its context
/// retired once the preedit is past the longest held: a context is made
/// again.
#[test]
fn a_preedit_past_the_longest_held_retires_its_context() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press(FLOODS);
    rig.settle("the context made again", || rig.heard_any("create 2"));
    assert!(rig.heard_any("destroy 1"), "{:?}", rig.heard.lock());
}
