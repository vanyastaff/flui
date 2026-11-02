//! The X11 host's text input: each window's focused field served to the
//! session's input method (IBus or Fcitx5) through a D-Bus input context of
//! the window's own, and every key handed to that input method before the
//! window hears it.
//!
//! The window's field is a [`TextInputClient`], the client the Wayland host
//! serves through `zwp_text_input_v3`, over a wire ([`BusWire`]) that states
//! the field to the window's input context: its focus, the text around its
//! caret, its purpose, and where its caret is on the screen. What the input
//! method sends back (a commit, a preedit, a deletion around the caret) is
//! one batch applied at once through the client, so it takes the same
//! commit gate (ADR-0142) and the same queue of requests made inside a call
//! into a store.
//!
//! A key typed into a window whose field has the input method's focus is
//! asked of the input method and held (`input_context::KeyHold`): the keys
//! behind it wait in order, and it is delivered when the input method did
//! not take it, or as not taken when its answer does not come in time. A
//! window that loses the focus, an input context that goes and a window
//! that closes deliver the keys they held, in order.
//!
//! The bus runs on the background executor (`bus`); what it hears waits for
//! the owner's turn, which applies it with no borrow of this feature held.
//! With no input method on the bus, every key goes to the window as typed,
//! as it did before.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_foundation::panic::FirstPanic;
use flui_platform::WindowId;
use flui_platform::text_store::TextStoreHost;
use web_time::Instant;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt as _, NotifyDetail, NotifyMode};

use super::input_context::{KeyHold, Told, chars_before, deletion};
use super::text_input::{ChangeCause, ContentType, TextInputClient, TextInputWire};
use crate::executor::BackgroundExecutor;
use crate::host::Text;
use crate::owner::linux::x11::{
    Doorbell, LoopText, WindowRef, WindowScale, X11Connection, X11Key, X11WindowText,
};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::slot::OwnerCell;
use crate::text::{TextInput, WindowTextInput};

mod bus;
#[cfg(test)]
mod bus_tests;

use bus::{Bus, ContextId, Heard, Reach, Request};

// ============================================================================
// The text position
// ============================================================================

/// The X11 table's text position: each window's [`WindowText`].
#[derive(Debug)]
pub struct X11Text;

impl Text<WindowRef> for X11Text {
    fn for_window(_window: &WindowRef) -> Arc<dyn TextInput> {
        Arc::new(WindowText::new())
    }
}

/// One window's text input: its client, made on the owner thread with the
/// window.
pub struct WindowText {
    client: OwnerCell<Option<Rc<TextInputClient>>>,
}

impl WindowText {
    fn new() -> Self {
        let client = OwnerCell::new();
        client.with(|client| *client = Some(TextInputClient::new()));
        Self { client }
    }

    /// The window's client, on the owner thread, until the window goes.
    fn client(&self) -> Option<Rc<TextInputClient>> {
        self.client.with(|client| client.clone()).flatten()
    }
}

impl TextInput for WindowText {
    fn window_text_input(&self) -> Option<WindowTextInput> {
        let client = self.client()?;
        Some(WindowTextInput::Store(client as Rc<dyn TextStoreHost>))
    }

    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    fn detach(&self) {
        if let Some(client) = self.client.with(Option::take).flatten() {
            client.retire();
        }
    }

    fn x11_text(self: Arc<Self>) -> Option<Arc<dyn X11WindowText>> {
        Some(self)
    }
}

impl X11WindowText for WindowText {}

impl std::fmt::Debug for WindowText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowText").finish_non_exhaustive()
    }
}

// ============================================================================
// The wire
// ============================================================================

/// Where a rectangle of the window's logical coordinates is on the screen:
/// `x`, `y`, width and height in pixels of the root window.
type Locate = Box<dyn Fn(Bounds<f64>) -> Option<(i32, i32, i32, i32)>>;

/// A window's input context as its client reaches it: what the client
/// states is gathered and sent at its `commit`.
struct BusWire {
    /// The window's input context: replaced when the input method left a
    /// key unanswered, so nothing it sends late reaches the field.
    context: Cell<ContextId>,
    bus: Arc<Bus>,
    locate: Locate,
    batch: RefCell<Vec<Request>>,
    /// The surrounding text last stated, which a deletion counts in.
    told: RefCell<Told>,
    /// The input method's preedit as it last sent it, which every batch
    /// states again: a commit or a deletion leaves it as it was.
    preedit: RefCell<(String, i32)>,
    commits: Cell<u32>,
    /// The last focus asked of the input method, and the last it answered:
    /// until they match, what it sends was for the field it served before,
    /// and reaches none.
    fence: Cell<u64>,
    fenced: Cell<u64>,
    /// The field has the input method's focus: keys are asked of it.
    focused: Cell<bool>,
    /// The context was asked for.
    opened: Cell<bool>,
    /// The context went: nothing is asked of it again.
    lost: Cell<bool>,
}

impl BusWire {
    /// Whether a key typed now is asked of the input method.
    fn asks(&self) -> bool {
        self.focused.get() && !self.lost.get()
    }

    /// Whether what the input method sends now is for the field served now:
    /// it answered the last focus asked of it.
    fn current(&self) -> bool {
        self.fenced.get() == self.fence.get()
    }

    /// The composition ended (the field changed, or the window lost the
    /// focus): its preedit is stated again in no later batch.
    fn forget_preedit(&self) {
        *self.preedit.borrow_mut() = (String::new(), 0);
    }
}

impl TextInputWire for BusWire {
    /// A field takes text input, or the input method is reset: what it sent
    /// before is fenced off until it answers this focus.
    fn enable(&self) {
        // Every enablement starts with no composition in the field: the
        // client ended it (completed in place, or the field changed, its
        // preedit committed in the field it left), so the input method
        // forgets it too. A focus alone would not reach an engine already
        // focused, and one that keeps its composition across a focus out
        // would bring it to the next field.
        self.focused.set(true);
        self.batch.borrow_mut().push(Request::Reset);
        self.forget_preedit();
        let fence = self.fence.get().wrapping_add(1);
        self.fence.set(fence);
        self.batch.borrow_mut().push(Request::FocusIn { fence });
    }

    fn disable(&self) {
        self.focused.set(false);
        self.forget_preedit();
        self.batch.borrow_mut().push(Request::FocusOut);
    }

    fn set_surrounding_text(&self, text: &str, cursor: usize, anchor: usize) {
        let (cursor_chars, anchor_chars) = (chars_before(text, cursor), chars_before(text, anchor));
        *self.told.borrow_mut() = Told {
            text: text.to_owned(),
            cursor,
            anchor,
        };
        self.batch.borrow_mut().push(Request::Surrounding {
            text: text.to_owned(),
            cursor: cursor_chars,
            anchor: anchor_chars,
        });
    }

    // Neither daemon has a cause: a change the input method made is told
    // like any other.
    fn set_text_change_cause(&self, _cause: ChangeCause) {}

    fn set_content_type(&self, content: ContentType) {
        self.batch.borrow_mut().push(Request::Content(content));
    }

    fn set_cursor_rectangle(&self, rect: Bounds<f64>) {
        if let Some((x, y, width, height)) = (self.locate)(rect) {
            self.batch.borrow_mut().push(Request::Cursor {
                x,
                y,
                width,
                height,
            });
        }
    }

    fn commit(&self) {
        let batch = std::mem::take(&mut *self.batch.borrow_mut());
        if !self.lost.get() {
            for request in batch {
                let fence = match request {
                    Request::FocusIn { fence } => Some(fence),
                    _ => None,
                };
                if !self.bus.send(self.context.get(), request) {
                    tracing::debug!(
                        "the input method's bus is behind: a field's state goes untold"
                    );
                    // A focus never sent is never answered: it fences nothing.
                    if let Some(fence) = fence {
                        self.fenced.set(fence);
                    }
                }
            }
        }
        self.commits.set(self.commits.get().wrapping_add(1));
    }

    /// Wrapping, as the client compares a batch's count with it alone.
    fn commits(&self) -> u32 {
        self.commits.get()
    }
}

/// A coordinate in whole pixels: a non-finite or out-of-range value pinned
/// to the nearest an `i32` holds.
fn whole(value: f64) -> i32 {
    if value.is_nan() {
        return 0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped into i32's range first, so the cast is exact"
    )]
    let whole = value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    whole
}

/// Where `window`'s rectangles are on the screen: scaled to its pixels and
/// moved by its origin on the root window, which the server is asked for.
/// With no connection (a test), the window's pixels are the screen's.
fn locate(
    connection: Option<Arc<X11Connection>>,
    window: xproto::Window,
    scale: WindowScale,
) -> Locate {
    Box::new(move |rect: Bounds<f64>| {
        let scale = scale.get();
        let pixels = |value: f64| whole(value * scale);
        let (x, y) = (pixels(rect.origin.x), pixels(rect.origin.y));
        let (width, height) = (
            pixels(rect.size.width).max(0),
            pixels(rect.size.height).max(0),
        );
        let Some(connection) = &connection else {
            return Some((x, y, width, height));
        };
        let reply = connection
            .xcb()
            .translate_coordinates(
                window,
                connection.root(),
                i16::try_from(x).ok()?,
                i16::try_from(y).ok()?,
            )
            .ok()?
            .reply()
            .ok()?;
        Some((
            i32::from(reply.dst_x),
            i32::from(reply.dst_y),
            width,
            height,
        ))
    })
}

// ============================================================================
// The windows' input method
// ============================================================================

/// A window as the input method reaches it.
struct Attached {
    window: WindowRef,
    text: Arc<WindowText>,
    wire: Rc<BusWire>,
}

#[derive(Default)]
struct Ime {
    windows: Vec<Attached>,
    hold: KeyHold<X11Key>,
    /// The window with the keyboard's focus.
    focused: Option<WindowId>,
    next: ContextId,
}

impl Ime {
    fn by_xid(&self, xid: xproto::Window) -> Option<&Attached> {
        self.windows.iter().find(|held| held.window.xid == xid)
    }

    fn by_id(&self, id: WindowId) -> Option<&Attached> {
        self.windows.iter().find(|held| held.window.id == id)
    }

    fn by_context(&self, context: ContextId) -> Option<&Attached> {
        self.windows
            .iter()
            .find(|held| held.wire.context.get() == context)
    }
}

/// A window losing the focus: its client and wire, which the client hears
/// leave once the keys it held are delivered.
struct Leaving {
    client: Option<Rc<TextInputClient>>,
    wire: Rc<BusWire>,
}

/// The X11 loop's text input: the windows' input contexts on the input
/// method's bus, and the keys held for its answers.
pub(crate) struct X11Ime {
    connection: Option<Arc<X11Connection>>,
    bus: Arc<Bus>,
    state: OwnerCell<Ime>,
}

impl std::fmt::Debug for X11Ime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Ime").finish_non_exhaustive()
    }
}

impl X11Ime {
    /// The text input of `connection`'s windows, the input method found as
    /// the session names it, on `executor`; what it sends rings `doorbell`.
    pub(crate) fn start(
        connection: &Arc<X11Connection>,
        executor: &BackgroundExecutor,
        doorbell: &Doorbell,
    ) -> Arc<Self> {
        let doorbell = doorbell.clone();
        let wake = Box::new(move || {
            if let Err(error) = doorbell.ring() {
                tracing::debug!(%error, "cannot wake the X11 loop for the input method");
            }
        });
        let bus = Bus::start(executor.handle(), Reach::Session, wake);
        Arc::new(Self::with_bus(Some(Arc::clone(connection)), bus))
    }

    fn with_bus(connection: Option<Arc<X11Connection>>, bus: Bus) -> Self {
        Self {
            connection,
            bus: Arc::new(bus),
            state: OwnerCell::new(),
        }
    }

    /// Deliver the keys whose turn came, each contained.
    fn deliver_ready(&self, failure: &mut FirstPanic) {
        let ready = self.state.with(|ime| ime.hold.ready()).unwrap_or_default();
        for key in ready {
            failure.contain(key.deliver);
        }
    }

    /// The window `xid` took the keyboard's focus: its field reaches the
    /// input method, after the window that had it let go.
    fn focus_in(&self, xid: xproto::Window, failure: &mut FirstPanic) {
        let moved = self
            .state
            .with(|ime| {
                let held = ime.by_xid(xid)?;
                let id = held.window.id;
                if ime.focused == Some(id) {
                    return None;
                }
                let entering = (held.text.client(), Rc::clone(&held.wire));
                let previous = ime.focused.replace(id);
                let leaving = previous.and_then(|previous| self.leave(ime, previous));
                Some((entering, leaving))
            })
            .flatten();
        let Some(((client, wire), leaving)) = moved else {
            return;
        };
        if let Some(leaving) = leaving {
            self.finish_leaving(leaving, failure);
        }
        if !wire.opened.replace(true) {
            self.bus.open(wire.context.get());
        }
        if let Some(client) = client {
            failure.contain(|| client.entered(wire as Rc<dyn TextInputWire>));
        }
    }

    /// The window `xid` lost the keyboard's focus.
    fn focus_out(&self, xid: xproto::Window, failure: &mut FirstPanic) {
        let leaving = self
            .state
            .with(|ime| {
                let id = ime.by_xid(xid)?.window.id;
                if ime.focused != Some(id) {
                    return None;
                }
                ime.focused = None;
                self.leave(ime, id)
            })
            .flatten();
        if let Some(leaving) = leaving {
            self.finish_leaving(leaving, failure);
        }
    }

    /// Window `id` loses the focus: the keys it held stop waiting, and its
    /// input context hears it.
    fn leave(&self, ime: &mut Ime, id: WindowId) -> Option<Leaving> {
        ime.hold.release_where(|key| key.window.id == id);
        let held = ime.by_id(id)?;
        let wire = Rc::clone(&held.wire);
        if wire.focused.replace(false) && !wire.lost.get() {
            self.bus.send(wire.context.get(), Request::FocusOut);
        }
        // The client takes the preedit out of the field as it hears the
        // window leave: no later batch states it again.
        wire.forget_preedit();
        Some(Leaving {
            client: held.text.client(),
            wire,
        })
    }

    /// The keys a window held go before its client hears it leave, which
    /// takes the preedit out of its field.
    fn finish_leaving(&self, leaving: Leaving, failure: &mut FirstPanic) {
        // The composition ends in the field before the keys it held go as
        // typed, which a field still composing would not take.
        if let Some(client) = leaving.client {
            failure.contain(|| client.left(&*leaving.wire));
        }
        self.deliver_ready(failure);
    }

    /// Window `id`'s input method left a key unanswered past its deadline,
    /// which goes as typed: an edit it sends for that key late would type
    /// it twice. Its context is destroyed and a new one made, so nothing the
    /// old one sends reaches the field; the field's composition, which the
    /// old context made, ends, and the field is stated to the new one.
    fn replace_context(&self, id: WindowId, failure: &mut FirstPanic) {
        let replaced = self
            .state
            .with(|ime| {
                let next = ime.next;
                let held = ime.by_id(id)?;
                let wire = Rc::clone(&held.wire);
                if !wire.opened.get() || wire.lost.get() {
                    return None;
                }
                let client = held.text.client();
                ime.next = next.next();
                let old = wire.context.replace(next);
                // The keys the old context still holds go as typed too.
                ime.hold.release_where(|key| key.window.id == id);
                wire.fenced.set(wire.fence.get());
                wire.forget_preedit();
                Some((old, next, wire, client))
            })
            .flatten();
        let Some((old, next, wire, client)) = replaced else {
            return;
        };
        tracing::debug!("the input method left a key unanswered: its input context is replaced");
        self.bus.close(old);
        self.bus.open(next);
        let Some(client) = client.filter(|_| wire.focused.get()) else {
            self.deliver_ready(failure);
            return;
        };
        // The composition the old context made ends in the field before the
        // keys it held go as typed, which a field still composing would not
        // take.
        failure.contain(|| client.left(&*wire));
        self.deliver_ready(failure);
        // A key delivered may have moved the focus or closed the window.
        let still = self
            .state
            .with(|ime| ime.focused == Some(id) && ime.by_id(id).is_some())
            .unwrap_or(false);
        if still && !wire.lost.get() {
            let wire: Rc<dyn TextInputWire> = wire;
            failure.contain(|| client.entered(wire));
        }
    }

    /// Apply what the bus heard.
    fn apply(&self, heard: Heard, failure: &mut FirstPanic) {
        let context = heard.context();
        if let Heard::Spilled(_) = heard {
            // The context's stream has a gap: it goes as one that left a key
            // unanswered.
            let window = self
                .state
                .with(|ime| ime.by_context(context).map(|held| held.window.id))
                .flatten();
            if let Some(window) = window {
                self.replace_context(window, failure);
            }
            return;
        }
        let target = self
            .state
            .with(|ime| {
                match &heard {
                    Heard::Answer(_, ticket, taken) => {
                        ime.hold.answer(*ticket, *taken);
                        return None;
                    }
                    Heard::Lost(_) => {
                        let held = ime.by_context(context)?;
                        held.wire.lost.set(true);
                        held.wire.focused.set(false);
                        let (id, client, wire) =
                            (held.window.id, held.text.client(), Rc::clone(&held.wire));
                        ime.hold.release_where(|key| key.window.id == id);
                        // Its client hears it leave, below.
                        return client.map(|client| (client, wire));
                    }
                    Heard::Fenced(_, fence) => {
                        ime.by_context(context)?.wire.fenced.set(*fence);
                        return None;
                    }
                    Heard::Ready(_) => return None,
                    _ => {}
                }
                let held = ime.by_context(context)?;
                Some((held.text.client()?, Rc::clone(&held.wire)))
            })
            .flatten();
        let Some((client, wire)) = target else {
            return;
        };
        if matches!(heard, Heard::Lost(_)) {
            // The input method went: the composition it made ends in the
            // field, before the keys it held go as typed, which a field
            // still composing would not take.
            wire.forget_preedit();
            failure.contain(|| client.left(&*wire));
            return;
        }
        if !wire.current() {
            tracing::debug!(
                ?heard,
                "the input method sent this for the field it served before"
            );
            return;
        }
        match heard {
            Heard::Commit(_, text) => {
                failure.contain(|| client.commit_string(&*wire, Some(text)));
            }
            Heard::Preedit(_, text, caret) => {
                *wire.preedit.borrow_mut() = (text, caret);
            }
            Heard::Delete(_, offset, chars) => {
                let Some((before, after)) = deletion(&wire.told.borrow(), offset, chars) else {
                    tracing::debug!(
                        offset,
                        chars,
                        "the input method deleted text away from the caret"
                    );
                    return;
                };
                failure.contain(|| client.delete_surrounding(&*wire, before, after));
                // Both daemons count the span over the selection, which
                // goes with it; the client's deletion keeps it, and an empty
                // commit replaces it.
                let told = wire.told.borrow();
                if told.cursor != told.anchor {
                    drop(told);
                    failure.contain(|| client.commit_string(&*wire, Some(String::new())));
                }
            }
            Heard::StateAsked(_) => {
                failure.contain(|| client.state_asked(&*wire));
                return;
            }
            Heard::Ready(_)
            | Heard::Answer(..)
            | Heard::Fenced(..)
            | Heard::Lost(_)
            | Heard::Spilled(_) => return,
        }
        // The batch states the preedit as it stands and applies at once.
        let (text, caret) = wire.preedit.borrow().clone();
        let text = (!text.is_empty()).then_some(text);
        failure.contain(|| {
            client.preedit(&*wire, text, caret, caret);
            client.done(&*wire, wire.commits());
        });
    }
}

impl LoopText for X11Ime {
    fn window_opened(
        &self,
        window: WindowRef,
        scale: WindowScale,
        text: Option<Arc<dyn X11WindowText>>,
    ) {
        let Some(text) = text else {
            return;
        };
        let text: Arc<dyn Any + Send + Sync> = text;
        let Ok(text) = text.downcast::<WindowText>() else {
            return;
        };
        let locate = locate(self.connection.clone(), window.xid, scale);
        let replaced = self.state.with(|ime| {
            let context = ime.next;
            ime.next = context.next();
            let wire = Rc::new(BusWire {
                context: Cell::new(context),
                bus: Arc::clone(&self.bus),
                locate,
                batch: RefCell::new(Vec::new()),
                told: RefCell::new(Told::default()),
                preedit: RefCell::new((String::new(), 0)),
                commits: Cell::new(0),
                fence: Cell::new(0),
                fenced: Cell::new(0),
                focused: Cell::new(false),
                opened: Cell::new(false),
                lost: Cell::new(false),
            });
            // The X id may name a window that is gone: only the id it names
            // now is the one opened.
            let index = ime
                .windows
                .iter()
                .position(|held| held.window.xid == window.xid);
            let replaced = index.map(|index| ime.windows.remove(index));
            ime.windows.push(Attached { window, text, wire });
            replaced
        });
        if let Some(Some(replaced)) = replaced {
            if replaced.wire.opened.get() {
                self.bus.close(replaced.wire.context.get());
            }
            contain_owner_callback(|| drop(replaced));
        }
    }

    fn window_closed(&self, window: WindowRef) {
        let removed = self
            .state
            .with(|ime| {
                let index = ime
                    .windows
                    .iter()
                    .position(|held| held.window.id == window.id)?;
                if ime.focused == Some(window.id) {
                    ime.focused = None;
                }
                ime.hold.release_where(|key| key.window.id == window.id);
                Some(ime.windows.remove(index))
            })
            .flatten();
        let mut failure = FirstPanic::new();
        self.deliver_ready(&mut failure);
        if let Some(removed) = removed {
            if removed.wire.opened.get() {
                self.bus.close(removed.wire.context.get());
            }
            failure.contain(|| drop(removed));
        }
        failure.resume();
    }

    fn event(&self, event: &Event) {
        let mut failure = FirstPanic::new();
        match event {
            // A focus that moves to the pointer's window is no field's.
            Event::FocusIn(focus) if focus.detail != NotifyDetail::POINTER => {
                self.focus_in(focus.event, &mut failure);
            }
            // As the input does: a grab's focus change is the grab's, and a
            // child taking the focus keeps it in the window.
            Event::FocusOut(focus)
                if focus.mode == NotifyMode::NORMAL
                    && focus.detail != NotifyDetail::INFERIOR
                    && focus.detail != NotifyDetail::POINTER =>
            {
                self.focus_out(focus.event, &mut failure);
            }
            _ => {}
        }
        failure.resume();
    }

    fn key(&self, key: X11Key) {
        let now = Instant::now();
        let mut evicted = None;
        self.state.with(|ime| {
            let wire = ime
                .focused
                .filter(|focused| *focused == key.window.id)
                .and_then(|focused| ime.by_id(focused))
                .map(|held| Rc::clone(&held.wire))
                .filter(|wire| wire.asks());
            let Some(wire) = wire else {
                ime.hold.pass(key);
                return;
            };
            let (keysym, keycode, state, release, time) =
                (key.keysym, key.keycode, key.state, key.release, key.time);
            let ticket = ime
                .hold
                .ask(key, now, |oldest| evicted = Some(oldest.window.id));
            let request = Request::Key {
                ticket,
                keysym,
                keycode,
                state,
                release,
                time,
            };
            if !self.bus.send(wire.context.get(), request) {
                // The bus takes no more now: the key goes as typed.
                ime.hold.answer(ticket, false);
            }
        });
        let mut failure = FirstPanic::new();
        // A key the full hold let go is typed as one overdue: an edit the
        // input method sends for it late must reach no field.
        if let Some(window) = evicted {
            self.replace_context(window, &mut failure);
        }
        self.deliver_ready(&mut failure);
        failure.resume();
    }

    fn deliver(&self, now: Instant) {
        let mut failure = FirstPanic::new();
        while let Some(heard) = self.bus.inbox().take() {
            self.apply(heard, &mut failure);
            // An answer delivers the keys it let through before what the
            // input method sent after it.
            self.deliver_ready(&mut failure);
        }
        let mut overdue = Vec::new();
        self.state.with(|ime| {
            ime.hold.expire(now, |key| {
                if !overdue.contains(&key.window.id) {
                    overdue.push(key.window.id);
                }
            });
        });
        for window in overdue {
            self.replace_context(window, &mut failure);
        }
        self.deliver_ready(&mut failure);
        failure.resume();
    }

    fn deadline(&self) -> Option<Instant> {
        self.state.with(|ime| ime.hold.deadline()).flatten()
    }

    fn release(&self) {
        let taken = self.state.with(|ime| {
            ime.focused = None;
            (
                std::mem::take(&mut ime.hold),
                std::mem::take(&mut ime.windows),
            )
        });
        let Some((hold, windows)) = taken else {
            return;
        };
        for held in &windows {
            if held.wire.opened.get() {
                self.bus.close(held.wire.context.get());
            }
        }
        // What the keys and the windows hold goes with no borrow held.
        contain_owner_callback(|| drop(hold));
        contain_owner_callback(|| drop(windows));
    }
}
