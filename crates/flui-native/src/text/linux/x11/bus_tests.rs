//! The X11 text input against a stand-in IBus on a private bus.
//!
//! The test starts a `dbus-daemon` of its own (skipped when none is
//! installed), where a stand-in owns `org.freedesktop.IBus` and serves its
//! input contexts. A window of the text input is focused by a synthetic
//! `FocusIn`, its field is an in-memory store, and keys are handed to it as
//! the input hands them: what the stand-in takes, commits and composes
//! reaches the store, and what it lets through reaches the key's own
//! delivery, in the order the stand-in answered.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use flui_platform::WindowId;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{Selection, TextStore, TextStoreHost};
use flui_testing::text_store_kit::InMemoryTextStore;
use parking_lot::Mutex;
use web_time::Instant;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, FocusInEvent, NotifyDetail, NotifyMode};
use zbus::object_server::SignalEmitter;
use zbus::{ObjectServer, OwnedObjectPath, Variant, interface};

use super::bus::{Bus, CREATE_TIMEOUT, MAX_HEARD, Reach};
use super::{WindowText, X11Ime};
use crate::owner::linux::x11::{LoopText, WindowRef, WindowScale, X11Key};
use crate::text::TextInput;
use crate::text::linux::input_context::{ANSWER_TIMEOUT, Daemon, MAX_HELD_KEYS};

/// A private session bus, stopped when dropped.
struct PrivateBus {
    daemon: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut address = String::new();
        BufReader::new(daemon.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_owned();
        (!address.is_empty()).then_some(Self { daemon, address })
    }

    fn stop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.stop();
    }
}

/// What the stand-in heard, in order.
type Heard = Arc<Mutex<Vec<String>>>;

/// The stand-in's `org.freedesktop.IBus`: it hands out input contexts.
struct StandInBus {
    heard: Heard,
    made: AtomicU32,
    /// How long making a context takes, in milliseconds.
    delay: Arc<AtomicU64>,
    /// How long answering a focus takes, in milliseconds.
    focus_delay: Arc<AtomicU64>,
}

#[interface(name = "org.freedesktop.IBus")]
impl StandInBus {
    async fn create_input_context(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        client: String,
    ) -> OwnedObjectPath {
        self.heard.lock().push(format!("context for {client}"));
        let delay = self.delay.load(Ordering::SeqCst);
        if delay > 0 {
            async_io::Timer::after(Duration::from_millis(delay)).await;
        }
        let number = self.made.fetch_add(1, Ordering::SeqCst);
        let path = format!("/org/freedesktop/IBus/InputContext_{number}");
        server
            .at(
                path.as_str(),
                StandInContext {
                    heard: Arc::clone(&self.heard),
                    content: (0, 0),
                    focus_delay: Arc::clone(&self.focus_delay),
                },
            )
            .await
            .expect("the stand-in serves its context");
        server
            .at(
                path.as_str(),
                StandInService {
                    heard: Arc::clone(&self.heard),
                },
            )
            .await
            .expect("the stand-in serves its context's service");
        OwnedObjectPath::try_from(path).expect("a valid path")
    }
}

/// The stand-in's `org.freedesktop.IBus.Service` on each context.
struct StandInService {
    heard: Heard,
}

#[interface(name = "org.freedesktop.IBus.Service")]
impl StandInService {
    fn destroy(&self) {
        self.heard.lock().push("destroy".to_owned());
    }
}

/// An IBusText of `text`, as the daemon sends it.
fn ibus_text(text: &str) -> Variant<'static> {
    Variant::from((
        "IBusText",
        HashMap::<String, Variant<'static>>::new(),
        text.to_owned(),
        Variant::from((
            "IBusAttrList",
            HashMap::<String, Variant<'static>>::new(),
            Vec::<Variant<'static>>::new(),
        )),
    ))
}

/// The keysyms the stand-in's engine answers for.
const COMMITS: u32 = 'a' as u32;
const SLOW: u32 = 'c' as u32;
const DELETES: u32 = 'x' as u32;
const ASKS: u32 = 'r' as u32;
const FLOODS: u32 = 'm' as u32;
const COMPOSES: u32 = 'p' as u32;
const LATE: u32 = 'l' as u32;

/// One input context of the stand-in: an engine that commits "あ" for `a`,
/// lets `b` through, answers `c` after a second, composes "にほ" for `p`,
/// commits "遅" for `l` after 400 ms, deletes three characters before the
/// cursor and one after for `x`, asks for the field for `r`, and for `m`
/// commits "x" more times than the owner queues before it answers.
struct StandInContext {
    heard: Heard,
    content: (u32, u32),
    /// How long answering a focus takes, in milliseconds.
    focus_delay: Arc<AtomicU64>,
}

#[interface(name = "org.freedesktop.IBus.InputContext")]
impl StandInContext {
    async fn process_key_event(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        keyval: u32,
        keycode: u32,
        state: u32,
    ) -> bool {
        self.heard
            .lock()
            .push(format!("key {keyval} {keycode} {state}"));
        let release = state & (1 << 30) != 0;
        match keyval {
            COMMITS if !release => {
                Self::commit_text(&emitter, ibus_text("あ"))
                    .await
                    .expect("the stand-in commits");
                true
            }
            COMPOSES if !release => {
                Self::update_preedit_text(&emitter, ibus_text("にほ"), 1, true)
                    .await
                    .expect("the stand-in composes");
                true
            }
            SLOW => {
                // Handlers run on zbus's own executor.
                async_io::Timer::after(Duration::from_secs(1)).await;
                true
            }
            LATE if !release => {
                async_io::Timer::after(Duration::from_millis(400)).await;
                Self::commit_text(&emitter, ibus_text("遅"))
                    .await
                    .expect("the stand-in commits late");
                self.heard.lock().push("late commit".to_owned());
                true
            }
            DELETES if !release => {
                Self::delete_surrounding_text(&emitter, -3, 4)
                    .await
                    .expect("the stand-in deletes");
                true
            }
            ASKS if !release => {
                Self::require_surrounding_text(&emitter)
                    .await
                    .expect("the stand-in asks for the field");
                true
            }
            FLOODS if !release => {
                for _ in 0..=MAX_HEARD {
                    Self::commit_text(&emitter, ibus_text("x"))
                        .await
                        .expect("the stand-in commits");
                }
                self.heard.lock().push("flooded".to_owned());
                async_io::Timer::after(Duration::from_millis(150)).await;
                self.heard.lock().push("answered".to_owned());
                true
            }
            COMMITS | COMPOSES | DELETES | ASKS | FLOODS => true,
            _ => false,
        }
    }

    async fn focus_in(&self) {
        self.heard.lock().push("focus in".to_owned());
        let delay = self.focus_delay.load(Ordering::SeqCst);
        if delay > 0 {
            async_io::Timer::after(Duration::from_millis(delay)).await;
        }
    }

    fn focus_out(&self) {
        self.heard.lock().push("focus out".to_owned());
    }

    fn reset(&self) {
        self.heard.lock().push("reset".to_owned());
    }

    fn set_capabilities(&self, capabilities: u32) {
        self.heard
            .lock()
            .push(format!("capabilities {capabilities}"));
    }

    fn set_surrounding_text(&self, text: Variant<'_>, cursor: u32, anchor: u32) {
        let text = super::bus::ibus_text_of(&text).unwrap_or_default();
        self.heard
            .lock()
            .push(format!("surrounding {text:?} {cursor} {anchor}"));
    }

    fn set_cursor_location(&self, x: i32, y: i32, width: i32, height: i32) {
        self.heard
            .lock()
            .push(format!("cursor {x} {y} {width} {height}"));
    }

    #[zbus(property)]
    fn content_type(&self) -> (u32, u32) {
        self.content
    }

    #[zbus(property)]
    fn set_content_type(&mut self, content: (u32, u32)) {
        self.content = content;
        self.heard
            .lock()
            .push(format!("content {} {}", content.0, content.1));
    }

    #[zbus(signal)]
    async fn commit_text(emitter: &SignalEmitter<'_>, text: Variant<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn update_preedit_text(
        emitter: &SignalEmitter<'_>,
        text: Variant<'_>,
        cursor: u32,
        visible: bool,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn delete_surrounding_text(
        emitter: &SignalEmitter<'_>,
        offset: i32,
        chars: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn require_surrounding_text(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// The keys delivered to the window, each with the field's text as it went.
type Delivered = Rc<RefCell<Vec<(char, String)>>>;

/// The text input with one window, its field `store`, served by a stand-in
/// IBus on a private bus.
struct Rig {
    bus: PrivateBus,
    server: zbus::Connection,
    runtime: tokio::runtime::Runtime,
    ime: X11Ime,
    window: WindowRef,
    text: Arc<WindowText>,
    store: Rc<InMemoryTextStore>,
    heard: Heard,
    delivered: Delivered,
    /// How long the stand-in takes to make a context, in milliseconds.
    create_delay: Arc<AtomicU64>,
    /// How long the stand-in takes to answer a focus, in milliseconds.
    focus_delay: Arc<AtomicU64>,
}

impl Rig {
    fn start() -> Option<Self> {
        let bus = PrivateBus::start().or_else(|| {
            eprintln!("skipped: no dbus-daemon to host a stand-in for IBus");
            None
        })?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let heard = Heard::default();
        let create_delay = Arc::new(AtomicU64::new(0));
        let focus_delay = Arc::new(AtomicU64::new(0));
        let server = runtime
            .block_on(
                zbus::connection::Builder::address(bus.address.as_str())
                    .name("org.freedesktop.IBus")
                    .serve_at(
                        "/org/freedesktop/IBus",
                        StandInBus {
                            heard: Arc::clone(&heard),
                            made: AtomicU32::new(0),
                            delay: Arc::clone(&create_delay),
                            focus_delay: Arc::clone(&focus_delay),
                        },
                    )
                    .build(),
            )
            .expect("the stand-in owns IBus's name");
        let reach = Reach::Address {
            daemon: Daemon::IBus,
            address: bus.address.clone(),
        };
        let ime = X11Ime::with_bus(None, Bus::start(runtime.handle(), reach, Box::new(|| {})));
        let window = WindowRef {
            xid: 7,
            id: WindowId(1),
        };
        let text = Arc::new(WindowText::new());
        ime.window_opened(window, WindowScale::new(1.0), Some(Arc::clone(&text) as _));
        let store = InMemoryTextStore::new("");
        text.client()
            .expect("the window's client")
            .focus_store(Some(Rc::clone(&store) as Rc<dyn TextStore>));
        let rig = Self {
            bus,
            server,
            runtime,
            ime,
            window,
            text,
            store,
            heard,
            delivered: Delivered::default(),
            create_delay,
            focus_delay,
        };
        rig.ime.event(&focus(FocusInEvent {
            response_type: xproto::FOCUS_IN_EVENT,
            detail: NotifyDetail::NONLINEAR,
            sequence: 0,
            event: rig.window.xid,
            mode: NotifyMode::NORMAL,
        }));
        rig.settle("the input method hears the focus", || {
            rig.heard_any("focus in")
        });
        Some(rig)
    }

    /// Run the owner's turns until `done`, failing after five seconds.
    fn settle(&self, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.ime.deliver(Instant::now());
            if done() {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// How many things the stand-in heard start with `prefix`.
    fn heard_count(&self, prefix: &str) -> usize {
        self.heard
            .lock()
            .iter()
            .filter(|heard| heard.starts_with(prefix))
            .count()
    }

    /// The window takes the keyboard's focus again.
    fn focus_in(&self) {
        self.ime.event(&focus(FocusInEvent {
            response_type: xproto::FOCUS_IN_EVENT,
            detail: NotifyDetail::NONLINEAR,
            sequence: 0,
            event: self.window.xid,
            mode: NotifyMode::NORMAL,
        }));
    }

    fn heard_any(&self, prefix: &str) -> bool {
        self.heard
            .lock()
            .iter()
            .any(|heard| heard.starts_with(prefix))
    }

    /// A press of `key`, delivered to the window with the field's text as it
    /// goes.
    fn press(&self, key: char) {
        self.ime.key(self.key(key, false));
    }

    fn key(&self, key: char, release: bool) -> X11Key {
        let delivered = Rc::clone(&self.delivered);
        let store = Rc::clone(&self.store);
        X11Key {
            window: self.window,
            keysym: u32::from(key),
            keycode: 38,
            state: 0,
            release,
            time: 0,
            deliver: Box::new(move || delivered.borrow_mut().push((key, store.text()))),
        }
    }

    fn delivered(&self) -> Vec<(char, String)> {
        self.delivered.borrow().clone()
    }

    fn focus_out(&self) {
        self.ime.event(&Event::FocusOut(FocusInEvent {
            response_type: xproto::FOCUS_OUT_EVENT,
            detail: NotifyDetail::NONLINEAR,
            sequence: 0,
            event: self.window.xid,
            mode: NotifyMode::NORMAL,
        }));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.ime.release();
        self.text.detach();
    }
}

fn focus(event: FocusInEvent) -> Event {
    Event::FocusIn(event)
}

#[test]
fn a_focused_field_is_stated_to_the_input_method() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.settle("the field's state", || rig.heard_any("surrounding"));
    let heard = rig.heard.lock().clone();
    let at = |prefix: &str| heard.iter().position(|heard| heard.starts_with(prefix));
    assert!(
        at("capabilities 41").is_some(),
        "preedit, focus and surrounding text: {heard:?}"
    );
    assert!(at("focus in") < at("surrounding"), "{heard:?}");
    assert!(at("content 0 ").is_some(), "free text: {heard:?}");
}

#[test]
fn a_commit_reaches_the_field_before_the_key_let_through_after_it() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('a');
    rig.press('b');
    rig.settle("the key let through", || !rig.delivered().is_empty());
    assert_eq!(
        rig.delivered(),
        [('b', "あ".to_owned())],
        "the taken key is not delivered, and the one after it finds the commit"
    );
    assert_eq!(rig.store.text(), "あ");
    rig.settle("the state after the commit", || {
        rig.heard_any("surrounding \"あ\" 1 1")
    });
}

#[test]
fn a_preedit_composes_in_the_field_and_leaves_with_the_focus() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    assert_eq!(rig.store.text(), "にほ");
    rig.focus_out();
    assert_eq!(rig.store.text(), "", "the preedit leaves with the focus");
    assert!(rig.store.composition().is_none());
    rig.settle("the input method hears the focus go", || {
        rig.heard_any("focus out")
    });
}

#[test]
fn a_key_unanswered_in_time_goes_as_typed_and_keeps_its_place() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let pressed = Instant::now();
    rig.press('c');
    rig.press('b');
    assert!(
        rig.delivered().is_empty(),
        "the second key waits behind the first"
    );
    rig.settle("the overdue key", || rig.delivered().len() == 2);
    assert!(
        pressed.elapsed() < Duration::from_secs(1),
        "delivered at the deadline, not at the late answer"
    );
    assert!(pressed.elapsed() >= ANSWER_TIMEOUT);
    assert_eq!(
        rig.delivered(),
        [('c', String::new()), ('b', String::new())]
    );
    // The late answer that `c` was taken finds no key waiting.
    std::thread::sleep(Duration::from_millis(1100));
    rig.ime.deliver(Instant::now());
    rig.press('b');
    rig.settle("the next key", || rig.delivered().len() == 3);
}

#[test]
fn keys_held_for_a_window_go_as_it_loses_the_focus() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('c');
    rig.press('b');
    rig.focus_out();
    assert_eq!(
        rig.delivered(),
        [('c', String::new()), ('b', String::new())],
        "delivered at once, in order"
    );
}

#[test]
fn a_key_after_the_input_method_went_goes_as_typed() {
    let Some(mut rig) = Rig::start() else {
        return;
    };
    rig.bus.stop();
    // Once the context is lost, a key is not asked: it goes to the window as
    // it comes, behind nothing.
    rig.settle("a key goes as it comes", || {
        let before = rig.delivered().len();
        rig.press('a');
        rig.delivered().len() == before + 1
    });
    assert!(
        rig.delivered().iter().all(|(key, _)| *key == 'a'),
        "every key the input method would have taken reached the window"
    );
}

#[test]
fn a_panic_delivering_a_key_leaves_the_next_delivered() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let delivered = Rc::clone(&rig.delivered);
    rig.press('c');
    rig.ime.key(X11Key {
        deliver: Box::new(move || {
            delivered.borrow_mut().push(('!', String::new()));
            panic!("a handler fails on a key");
        }),
        ..rig.key('b', false)
    });
    rig.press('b');
    let deadline = Instant::now() + Duration::from_secs(5);
    let caught = loop {
        let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rig.ime.deliver(Instant::now());
        }));
        if turn.is_err() {
            break true;
        }
        assert!(Instant::now() < deadline, "the overdue keys never went");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(caught, "the handler's panic is raised again");
    assert_eq!(
        rig.delivered()
            .iter()
            .map(|(key, _)| *key)
            .collect::<String>(),
        "c!b",
        "the key after the one that panicked is delivered too"
    );
    rig.press('b');
    rig.settle("the next key", || rig.delivered().len() == 4);
}

#[test]
fn a_window_that_closes_delivers_its_keys_and_hears_nothing_more() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('c');
    rig.ime.window_closed(rig.window);
    assert_eq!(rig.delivered(), [('c', String::new())]);
    rig.press('a');
    assert_eq!(
        rig.delivered().len(),
        2,
        "a key of no window it knows goes as typed"
    );
}

/// What the input method sent for a field reaches no field focused after
/// it: the focus asked of it fences what it sent before, and what it sends
/// once it answered reaches the new field.
#[test]
fn an_edit_sent_for_the_field_before_reaches_not_the_next() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('a');
    // The commit is heard, and waits for the owner's turn, as the
    // application moves the focus to another field.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !rig.heard_any("key 97") {
        assert!(
            Instant::now() < deadline,
            "the stand-in never heard the key"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(100));
    let next = InMemoryTextStore::new("");
    rig.text
        .client()
        .expect("the window's client")
        .focus_store(Some(Rc::clone(&next) as Rc<dyn TextStore>));
    rig.settle("the input method hears the next field", || {
        rig.heard_count("focus in") == 2
    });
    let settled = Instant::now() + Duration::from_millis(200);
    rig.settle("the turns after", || Instant::now() >= settled);
    assert_eq!(
        next.text(),
        "",
        "the earlier field's commit reaches no field"
    );
    assert_eq!(rig.store.text(), "");
    rig.ime.key(X11Key {
        window: rig.window,
        keysym: u32::from('a'),
        keycode: 38,
        state: 0,
        release: false,
        time: 0,
        deliver: Box::new(|| {}),
    });
    rig.settle("the next field's commit", || next.text() == "あ");
}

/// A preedit the window's focus loss took out of the field is not composed
/// again by the next batch, which states no preedit of its own.
#[test]
fn a_preedit_ended_with_the_focus_is_not_composed_again() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    rig.focus_out();
    assert_eq!(rig.store.text(), "");
    rig.focus_in();
    rig.settle("the input method hears the focus back", || {
        rig.heard_count("focus in") == 2
    });
    rig.press('a');
    rig.settle("the commit", || !rig.store.text().is_empty());
    assert_eq!(rig.store.text(), "あ", "the commit alone, no preedit");
    assert!(rig.store.composition().is_none());
}

/// A call the input method leaves unanswered is given up on: what the task
/// keeps for it goes, before the input method answers at all.
#[test]
fn a_call_unanswered_in_time_is_given_up_on() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let awaited = || {
        rig.ime
            .bus
            .inbox()
            .awaited
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    let pressed = Instant::now();
    for _ in 0..3 {
        rig.press('c');
    }
    rig.settle("the calls are sent", || awaited() == 3);
    rig.settle("the calls are given up on", || awaited() == 0);
    assert!(
        pressed.elapsed() < Duration::from_millis(900),
        "given up on before the input method answers, a second after: {:?}",
        pressed.elapsed()
    );
}

/// A window closed while the bus task is behind still has its context
/// destroyed: the close is not dropped with the requests the task could not
/// take.
#[test]
fn a_window_closed_while_the_bus_is_behind_still_destroys_its_context() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let context = rig
        .ime
        .state
        .with(|ime| ime.by_id(rig.window.id).map(|held| held.wire.context.get()))
        .flatten()
        .expect("the window's context");
    let mut full = false;
    for _ in 0..1_000_000 {
        if !rig.ime.bus.send(context, super::bus::Request::FocusOut) {
            full = true;
            break;
        }
    }
    assert!(full, "the bus task fell behind");
    rig.ime.window_closed(rig.window);
    rig.settle("the context is destroyed", || rig.heard_any("destroy"));
}

/// The input method going mid-composition ends the composition in the
/// field, which then takes keys as typed.
#[test]
fn a_composition_ends_as_the_input_method_goes() {
    let Some(mut rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    rig.bus.stop();
    rig.settle("the composition ends", || rig.store.composition().is_none());
    rig.press('b');
    rig.settle("the key as typed", || !rig.delivered().is_empty());
}

/// A key the input method answers past its deadline goes as typed, and what
/// it commits for that key late reaches no field: the window's context is
/// replaced, and the field is stated to the new one.
#[test]
fn an_edit_for_a_key_already_typed_reaches_no_field() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('l');
    rig.settle("the key as typed", || !rig.delivered().is_empty());
    rig.settle("the late commit", || rig.heard_any("late commit"));
    rig.settle("the field stated to a new context", || {
        rig.heard_count("context for") == 2 && rig.heard_any("destroy")
    });
    let settled = Instant::now() + Duration::from_millis(200);
    rig.settle("the turns after", || Instant::now() >= settled);
    assert_eq!(rig.store.text(), "", "the late commit types nothing");
    rig.press('a');
    rig.settle("the new context's commit", || rig.store.text() == "あ");
}

/// A composition the application completes is forgotten by the input method
/// too: its engine is reset.
#[test]
fn a_completed_composition_resets_the_engine() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    let store: Rc<dyn TextStore> = Rc::clone(&rig.store) as _;
    let _ = rig
        .text
        .client()
        .expect("the window's client")
        .complete_composition(&store);
    assert_eq!(rig.store.text(), "にほ", "the preedit stays as text");
    rig.settle("the engine is reset", || rig.heard_any("reset"));
}

/// A key held mid-composition goes as typed to a field whose composition
/// already ended, which a field still composing would not take: whether the
/// input method left it unanswered or the window lost the focus.
#[test]
fn a_key_held_mid_composition_reaches_a_field_no_longer_composing() {
    for focus_lost in [false, true] {
        let Some(rig) = Rig::start() else {
            return;
        };
        rig.press('p');
        rig.settle("the preedit", || rig.store.composition().is_some());
        let composing = Rc::new(RefCell::new(Vec::new()));
        let mut key = rig.key('c', false);
        key.deliver = Box::new({
            let composing = Rc::clone(&composing);
            let store = Rc::clone(&rig.store);
            move || composing.borrow_mut().push(store.composition().is_some())
        });
        rig.ime.key(key);
        if focus_lost {
            rig.focus_out();
        }
        rig.settle("the key as typed", || !composing.borrow().is_empty());
        assert_eq!(
            *composing.borrow(),
            [false],
            "delivered after the composition ended (focus lost: {focus_lost})"
        );
    }
}

/// An input method that leaves a session bus still running is gone all the
/// same: its composition ends, and the field takes keys as typed.
#[test]
fn a_composition_ends_as_the_input_method_leaves_a_running_bus() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    rig.runtime
        .block_on(rig.server.release_name("org.freedesktop.IBus"))
        .expect("the stand-in lets its name go");
    rig.settle("the composition ends", || rig.store.composition().is_none());
    let pressed = Instant::now();
    rig.press('b');
    rig.settle("the key as typed", || !rig.delivered().is_empty());
    assert!(
        pressed.elapsed() < ANSWER_TIMEOUT,
        "no key waits on an input method that went"
    );
}

/// A context the input method makes after it was given up on is destroyed
/// when it arrives, so a slow daemon keeps none it was asked for.
#[test]
fn a_context_made_after_it_was_given_up_on_is_destroyed() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let late = CREATE_TIMEOUT + Duration::from_millis(300);
    rig.create_delay.store(
        u64::try_from(late.as_millis()).expect("a short delay"),
        Ordering::SeqCst,
    );
    // A key left unanswered replaces the window's context: the old one is
    // destroyed and a new one asked for, which comes late.
    rig.press('l');
    rig.settle("the old context destroyed", || {
        rig.heard_count("destroy") == 1 && rig.heard_count("context for") == 2
    });
    let deadline = Instant::now() + late + Duration::from_secs(2);
    loop {
        rig.ime.deliver(Instant::now());
        if rig.heard_count("destroy") == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the late context is never destroyed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A deletion the input method counts over a selection takes the selected
/// text with it.
#[test]
fn a_deletion_over_the_selection_takes_it_too() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.store
        .app_replace(Utf16Range::collapsed(Utf16Offset::new(0)), "abcd");
    rig.store.app_select(Selection {
        anchor: Utf16Offset::new(1),
        active: Utf16Offset::new(3),
    });
    rig.settle("the field told", || {
        rig.heard_any("surrounding \"abcd\" 3 1")
    });
    rig.press('x');
    rig.settle("the deletion", || rig.store.text() != "abcd");
    assert_eq!(
        rig.store.text(),
        "",
        "three before the cursor and one after"
    );
}

/// An engine that asks for the field (attached after the field was told)
/// is told it again.
#[test]
fn an_engine_asking_for_the_field_is_told_it() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.store
        .app_replace(Utf16Range::collapsed(Utf16Offset::new(0)), "abc");
    rig.settle("the field told", || rig.heard_any("surrounding \"abc\""));
    let told = rig.heard_count("surrounding \"abc\"");
    rig.press('r');
    rig.settle("the field told again", || {
        rig.heard_count("surrounding \"abc\"") > told
    });
}

/// A key a full hold lets go is typed as one overdue: the window's context
/// is replaced, so what the input method does with it late reaches no field.
#[test]
fn a_key_a_full_hold_lets_go_retires_its_context() {
    let Some(rig) = Rig::start() else {
        return;
    };
    for _ in 0..=MAX_HELD_KEYS {
        rig.press('c');
    }
    assert_eq!(
        rig.delivered().len(),
        MAX_HELD_KEYS + 1,
        "the window's keys go as typed with its context"
    );
    rig.settle("the field stated to a new context", || {
        rig.heard_count("context for") == 2 && rig.heard_any("destroy")
    });
}

/// An input method that sends more than the owner queues has its context
/// retired: an answer past the gap does not take a key whose edit was
/// dropped, which goes as typed.
#[test]
fn a_context_whose_edits_overflow_is_retired() {
    let Some(rig) = Rig::start() else {
        return;
    };
    // Delivered as of the press, so no deadline passes: only the gap
    // retires the context.
    let pressed = Instant::now();
    rig.press('m');
    let wait = |what: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !rig.heard_any(what) {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    wait("flooded");
    let deadline = Instant::now() + Duration::from_secs(5);
    // Past the bound by the mark of what was dropped.
    while rig.ime.bus.inbox().queued() <= MAX_HEARD {
        assert!(
            Instant::now() < deadline,
            "timed out: the owner's queue overflowing"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    rig.ime.deliver(pressed);
    wait("answered");
    rig.ime.deliver(pressed);
    assert!(
        rig.delivered().iter().any(|(key, _)| *key == 'm'),
        "the key goes as typed"
    );
    rig.settle("the field stated to a new context", || {
        rig.heard_count("context for") == 2 && rig.heard_any("destroy")
    });
}

/// A composition left behind as the application focuses another field is
/// forgotten by the input method too: its engine is reset, so it does not
/// resume in the next field.
#[test]
fn a_composition_left_for_another_field_resets_the_engine() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    let resets = rig.heard_count("reset");
    let next = InMemoryTextStore::new("");
    rig.text
        .client()
        .expect("the window's client")
        .focus_store(Some(Rc::clone(&next) as Rc<dyn TextStore>));
    rig.settle("the engine is reset", || rig.heard_count("reset") > resets);
}

/// A focus the bus could not take leaves the earlier field's edits fenced
/// off until the focus, sent again once the bus takes it, is answered.
#[test]
fn an_edit_for_the_field_before_an_unsent_focus_reaches_no_field() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('a');
    let deadline = Instant::now() + Duration::from_secs(5);
    while !rig.heard_any("key 97") {
        assert!(
            Instant::now() < deadline,
            "the stand-in never heard the key"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(100));
    let context = rig
        .ime
        .state
        .with(|ime| ime.by_id(rig.window.id).map(|held| held.wire.context.get()))
        .flatten()
        .expect("the window's context");
    let mut full = false;
    for _ in 0..1_000_000 {
        if !rig.ime.bus.send(context, super::bus::Request::FocusOut) {
            full = true;
            break;
        }
    }
    assert!(full, "the bus task fell behind");
    let resets = rig.heard_count("reset");
    let next = InMemoryTextStore::new("");
    rig.text
        .client()
        .expect("the window's client")
        .focus_store(Some(Rc::clone(&next) as Rc<dyn TextStore>));
    let settled = Instant::now() + Duration::from_millis(300);
    rig.settle("the turns after", || Instant::now() >= settled);
    assert_eq!(
        next.text(),
        "",
        "the earlier field's commit reaches no field"
    );
    rig.settle("the next field's focus sent again", || {
        rig.heard_count("focus in") == 2
    });
    assert!(
        rig.heard_count("reset") > resets,
        "the engine is reset with the focus sent again"
    );
}

/// A press of the pointer, which may move the caret, comes after the keys
/// typed before it: one still unanswered goes as typed first, and its late
/// answer edits nothing.
#[test]
fn a_pointer_press_comes_after_the_keys_typed_before_it() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('c');
    assert!(rig.delivered().is_empty(), "the key waits for its answer");
    rig.ime.pointer_pressed(rig.window.xid);
    assert_eq!(
        rig.delivered(),
        [('c', String::new())],
        "typed before the press reaches the window"
    );
    rig.settle("the field stated to a new context", || {
        rig.heard_count("context for") == 2 && rig.heard_any("destroy")
    });
}

/// A key typed right after one that moves the field (a Tab the input method
/// lets through) reaches the next field: its edit, made for the field
/// before, is fenced off, so it goes as typed.
#[test]
fn a_key_typed_after_one_that_moves_the_field_reaches_the_next() {
    let Some(rig) = Rig::start() else {
        return;
    };
    let next = InMemoryTextStore::new("");
    let client = rig.text.client().expect("the window's client");
    let moved = Rc::clone(&next);
    // `b` is let through, and moves the focus as a Tab would.
    rig.ime.key(X11Key {
        window: rig.window,
        keysym: u32::from('b'),
        keycode: 38,
        state: 0,
        release: false,
        time: 0,
        deliver: Box::new(move || {
            client.focus_store(Some(Rc::clone(&moved) as Rc<dyn TextStore>));
        }),
    });
    rig.press('a');
    rig.settle("the key after the move", || {
        !rig.delivered().is_empty() || !next.text().is_empty()
    });
    assert_eq!(
        rig.delivered(),
        [('a', String::new())],
        "the key goes as typed, its edit for the field before fenced off"
    );
    assert_eq!(rig.store.text(), "", "nothing reaches the field before");
}

/// A key the bus refuses mid-composition goes as typed to a field no longer
/// composing, which would not take it otherwise.
#[test]
fn a_key_the_bus_refuses_mid_composition_reaches_a_field_no_longer_composing() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.press('p');
    rig.settle("the preedit", || rig.store.composition().is_some());
    let context = rig
        .ime
        .state
        .with(|ime| ime.by_id(rig.window.id).map(|held| held.wire.context.get()))
        .flatten()
        .expect("the window's context");
    // The bus task may take a command between the queue filling and the
    // key: the key is tried until the bus refuses it, which delivers it at
    // once.
    for _ in 0..20 {
        let mut full = false;
        for _ in 0..1_000_000 {
            if !rig.ime.bus.send(context, super::bus::Request::FocusOut) {
                full = true;
                break;
            }
        }
        assert!(full, "the bus task fell behind");
        let composing = Rc::new(Cell::new(None));
        let (seen, store) = (Rc::clone(&composing), Rc::clone(&rig.store));
        rig.ime.key(X11Key {
            window: rig.window,
            keysym: u32::from('b'),
            keycode: 38,
            state: 0,
            release: false,
            time: 0,
            deliver: Box::new(move || seen.set(Some(store.composition().is_some()))),
        });
        if let Some(composing) = composing.get() {
            assert!(!composing, "the composition ends before the key goes");
            return;
        }
        rig.settle("the key the bus took", || composing.get().is_some());
        assert!(
            rig.store.composition().is_some(),
            "the composition stays for the next try"
        );
    }
    panic!("the bus never refused the key");
}

/// A focus the input method leaves unanswered retires its context: what it
/// sends can no longer be told to be for the field it names.
#[test]
fn a_focus_unanswered_in_time_retires_its_context() {
    let Some(rig) = Rig::start() else {
        return;
    };
    rig.focus_delay.store(1_000, Ordering::SeqCst);
    let next = InMemoryTextStore::new("");
    rig.text
        .client()
        .expect("the window's client")
        .focus_store(Some(Rc::clone(&next) as Rc<dyn TextStore>));
    rig.settle("the context retired", || {
        rig.heard_count("context for") >= 2 && rig.heard_any("destroy")
    });
    rig.focus_delay.store(0, Ordering::SeqCst);
}
