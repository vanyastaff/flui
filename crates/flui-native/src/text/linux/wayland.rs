//! The Wayland host's text input: each seat's `zwp_text_input_v3`, which the
//! seats' input binds and records, and the windows' [`TextInputClient`]s it
//! drives.
//!
//! A dispatch only records: the event becomes a [`TextWire`] in the seats'
//! queue, in order with the keys and pointers recorded around it, so a
//! string the input method commits and a key it forwards reach the field in
//! the order the compositor sent them. On each turn the seats' input hands
//! each recorded event to [`SeatText::deliver`], with no cell of the input
//! borrowed, and the client of the window the seat's text input is in
//! applies it.
//!
//! A compositor that offers no `zwp_text_input_manager_v3` gives the windows
//! no text input: their store host is `None` (FLUI's gap 5 names it), and a
//! debug warning says so once.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use flui_foundation::geometry::Bounds;
use flui_platform::WindowId;
use flui_platform::text_store::TextStoreHost;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::{
    self, ZwpTextInputManagerV3,
};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    self, ContentHint, ContentPurpose, ZwpTextInputV3,
};

use super::text_input::{ChangeCause, ContentType, TextInputClient, TextInputWire};
use crate::host::Text;
use crate::owner::linux::wayland::{
    SeatText, SeatWindowText, TextEvent, TextRecorder, TextWire, WaylandState,
};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::slot::OwnerCell;
use crate::text::{TextInput, WindowTextInput};

/// `zwp_text_input_manager_v3` version 1: the version every compositor
/// speaks; version 2's actions and input-panel requests are not asked for.
const TEXT_INPUT_VERSION: u32 = 1;

/// What a `zwp_text_input_v3` carries: the queue it records into, held
/// weakly, so an object outliving the input records nothing.
pub(crate) struct TextInputData {
    recorder: Weak<dyn TextRecorder>,
}

impl Dispatch<ZwpTextInputManagerV3, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: zwp_text_input_manager_v3::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZwpTextInputV3, TextInputData> for WaylandState {
    fn event(
        _: &mut Self,
        input: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        data: &TextInputData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            zwp_text_input_v3::Event::Enter { surface } => TextEvent::Enter(surface),
            zwp_text_input_v3::Event::Leave { .. } => TextEvent::Leave,
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => TextEvent::Preedit {
                text,
                begin: cursor_begin,
                end: cursor_end,
            },
            zwp_text_input_v3::Event::CommitString { text } => TextEvent::Commit(text),
            zwp_text_input_v3::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => TextEvent::Delete {
                before: before_length,
                after: after_length,
            },
            zwp_text_input_v3::Event::Done { serial } => TextEvent::Done(serial),
            // Version 2's events, never asked for.
            _ => return,
        };
        if let Some(recorder) = data.recorder.upgrade() {
            recorder.record_text(TextWire {
                input: input.id(),
                event,
            });
        }
    }
}

/// A coordinate of the protocol's rectangle: whole logical pixels, a
/// non-finite or out-of-range value pinned to the nearest an `int` holds.
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

/// The protocol's `int` for a byte offset of the surrounding text, which
/// the client caps far below `i32::MAX`.
fn offset(bytes: usize) -> i32 {
    i32::try_from(bytes).unwrap_or(i32::MAX)
}

/// One seat's `zwp_text_input_v3` as a window's client reaches it.
struct SeatWire {
    object: ZwpTextInputV3,
    commits: Cell<u32>,
}

impl TextInputWire for SeatWire {
    fn enable(&self) {
        self.object.enable();
    }

    fn disable(&self) {
        self.object.disable();
    }

    fn set_surrounding_text(&self, text: &str, cursor: usize, anchor: usize) {
        self.object
            .set_surrounding_text(text.to_owned(), offset(cursor), offset(anchor));
    }

    fn set_text_change_cause(&self, cause: ChangeCause) {
        self.object.set_text_change_cause(match cause {
            ChangeCause::InputMethod => zwp_text_input_v3::ChangeCause::InputMethod,
            ChangeCause::Other => zwp_text_input_v3::ChangeCause::Other,
        });
    }

    fn set_content_type(&self, content: ContentType) {
        let purpose = ContentPurpose::try_from(content.purpose).unwrap_or(ContentPurpose::Normal);
        self.object
            .set_content_type(ContentHint::from_bits_truncate(content.hint), purpose);
    }

    /// The window's logical coordinates are its surface's: the surface's
    /// origin is the window's.
    fn set_cursor_rectangle(&self, rect: Bounds<f64>) {
        let (x, y) = (rect.origin.x, rect.origin.y);
        let (width, height) = (rect.size.width, rect.size.height);
        self.object.set_cursor_rectangle(
            whole(x),
            whole(y),
            whole(width).max(0),
            whole(height).max(0),
        );
    }

    fn commit(&self) {
        self.object.commit();
        self.commits.set(self.commits.get().wrapping_add(1));
    }

    /// Wrapping, as the protocol's serial is a `uint` the compositor counts
    /// the same way.
    fn commits(&self) -> u32 {
        self.commits.get()
    }
}

/// A seat's text input and the window it is in.
struct HeldInput {
    seat: WlSeat,
    wire: Rc<SeatWire>,
    window: Option<WindowId>,
}

/// A window's text, as the seats' text inputs reach it.
struct AttachedText {
    id: WindowId,
    surface: WlSurface,
    text: Arc<WindowText>,
}

impl AttachedText {
    fn client(&self) -> Option<Rc<TextInputClient>> {
        self.text.client()
    }
}

#[derive(Default)]
struct TextObjects {
    manager: Option<(u32, ZwpTextInputManagerV3)>,
    inputs: Vec<HeldInput>,
    windows: Vec<AttachedText>,
    /// The missing manager was reported.
    warned: bool,
}

impl TextObjects {
    fn input(&self, id: &ObjectId) -> Option<&HeldInput> {
        self.inputs.iter().find(|held| held.wire.object.id() == *id)
    }

    fn input_mut(&mut self, id: &ObjectId) -> Option<&mut HeldInput> {
        self.inputs
            .iter_mut()
            .find(|held| held.wire.object.id() == *id)
    }

    fn window(&self, id: WindowId) -> Option<&AttachedText> {
        self.windows.iter().find(|window| window.id == id)
    }
}

/// The seats' text inputs: what the seats' input holds of text, reached on
/// the owner thread.
pub(crate) struct SeatTexts {
    objects: OwnerCell<TextObjects>,
}

impl SeatTexts {
    pub(crate) fn new() -> Self {
        Self {
            objects: OwnerCell::new(),
        }
    }
}

impl SeatText for SeatTexts {
    /// Whether `interface` is the global the text inputs come from.
    fn speaks(&self, interface: &str) -> bool {
        interface == ZwpTextInputManagerV3::interface().name
    }

    /// Whether the text inputs hold a manager.
    fn holds_manager(&self) -> bool {
        self.objects
            .with(|objects| objects.manager.is_some())
            .unwrap_or(false)
    }

    /// Bind the manager, global `name`, and make a text input for each of
    /// `seats`.
    fn bind(
        &self,
        registry: &WlRegistry,
        name: u32,
        version: u32,
        queue: &QueueHandle<WaylandState>,
        recorder: &Weak<dyn TextRecorder>,
        seats: &[WlSeat],
    ) {
        let manager = registry.bind::<ZwpTextInputManagerV3, (), WaylandState>(
            name,
            version.min(TEXT_INPUT_VERSION),
            queue,
            (),
        );
        let windows = self
            .objects
            .with(|objects| {
                for seat in seats {
                    if !objects.inputs.iter().any(|held| held.seat == *seat) {
                        objects
                            .inputs
                            .push(make_input(&manager, seat, queue, recorder));
                    }
                }
                objects.manager = Some((name, manager));
                objects
                    .windows
                    .iter()
                    .map(|window| Arc::clone(&window.text))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for text in windows {
            text.set_offered(true);
        }
    }

    /// The compositor removed global `name`: when it was the manager, every
    /// text input goes and the windows they were in hear them leave.
    fn global_removed(&self, name: u32) {
        let left = self
            .objects
            .with(|objects| {
                if objects
                    .manager
                    .as_ref()
                    .is_none_or(|(held, _)| *held != name)
                {
                    return Vec::new();
                }
                objects.manager = None;
                let inputs = std::mem::take(&mut objects.inputs);
                let mut left = Vec::new();
                for held in inputs {
                    held.wire.object.destroy();
                    if let Some(id) = held.window
                        && let Some(client) = objects.window(id).and_then(AttachedText::client)
                    {
                        left.push(Left {
                            client,
                            wire: held.wire,
                            next: None,
                        });
                    }
                }
                left
            })
            .unwrap_or_default();
        for left in left {
            left.deliver();
        }
    }

    /// Seat `seat` was bound: it gets a text input where the manager is held.
    fn seat_added(
        &self,
        seat: &WlSeat,
        queue: &QueueHandle<WaylandState>,
        recorder: &Weak<dyn TextRecorder>,
    ) {
        self.objects.with(|objects| {
            if let Some((_, manager)) = &objects.manager
                && !objects.inputs.iter().any(|held| held.seat == *seat)
            {
                let input = make_input(manager, seat, queue, recorder);
                objects.inputs.push(input);
            }
        });
    }

    /// Seat `seat` went: its text input goes, and the window it was in hears
    /// it leave.
    fn seat_removed(&self, seat: &WlSeat) {
        let left = self
            .objects
            .with(|objects| {
                let index = objects.inputs.iter().position(|held| held.seat == *seat)?;
                let held = objects.inputs.remove(index);
                held.wire.object.destroy();
                objects.leaving(held.window?, held.wire)
            })
            .flatten();
        if let Some(left) = left {
            left.deliver();
        }
    }

    /// Window `id` opened on `surface`: a seat's text input that enters it
    /// reaches `text`, which takes text input where the compositor offers it.
    fn attach(&self, id: WindowId, surface: &WlSurface, text: &Arc<dyn SeatWindowText>) {
        let text: Arc<dyn Any + Send + Sync> = Arc::<dyn SeatWindowText>::clone(text);
        let text = text
            .downcast::<WindowText>()
            .expect("BUG: a window's seat text is the WindowText this feature made");
        let offered = self
            .objects
            .with(|objects| {
                objects.windows.retain(|held| held.id != id);
                objects.windows.push(AttachedText {
                    id,
                    surface: surface.clone(),
                    text: Arc::clone(&text),
                });
                let offered = objects.manager.is_some();
                if !offered && !objects.warned {
                    objects.warned = true;
                    tracing::warn!(
                        "the Wayland compositor offers no zwp_text_input_v3: no input method \
                         reaches FLUI's text fields (gap 5)"
                    );
                }
                offered
            })
            .unwrap_or(false);
        text.set_offered(offered);
    }

    /// Window `id` closed: nothing more reaches it. Its client is retired by
    /// the window.
    fn detach(&self, id: WindowId) {
        let removed = self
            .objects
            .with(|objects| {
                for held in &mut objects.inputs {
                    if held.window == Some(id) {
                        held.window = None;
                    }
                }
                let index = objects.windows.iter().position(|held| held.id == id)?;
                Some(objects.windows.remove(index))
            })
            .flatten();
        contain_owner_callback(|| drop(removed));
    }

    /// Apply one recorded event to the client of the window its text input
    /// is in, with no cell borrowed.
    fn deliver(&self, wire: TextWire) {
        let routed = self
            .objects
            .with(|objects| {
                let TextWire { input, event } = wire;
                let event = match event {
                    TextEvent::Enter(surface) => {
                        let window = objects
                            .windows
                            .iter()
                            .find(|window| window.surface == surface)
                            .map(|window| window.id);
                        let held = objects.input_mut(&input)?;
                        held.window = window;
                        let wire = Rc::clone(&held.wire);
                        let client = objects.window(window?)?.client()?;
                        return Some(Routed::Enter(client, wire));
                    }
                    TextEvent::Leave => {
                        let held = objects.input_mut(&input)?;
                        let window = held.window.take()?;
                        let wire = Rc::clone(&held.wire);
                        return objects.leaving(window, wire).map(Routed::Leave);
                    }
                    event => event,
                };
                let held = objects.input(&input)?;
                let wire = Rc::clone(&held.wire);
                let client = objects.window(held.window?)?.client()?;
                Some(Routed::Event(client, wire, event))
            })
            .flatten();
        match routed {
            Some(Routed::Enter(client, wire)) => client.entered(wire),
            Some(Routed::Leave(left)) => left.deliver(),
            Some(Routed::Event(client, wire, event)) => match event {
                TextEvent::Preedit { text, begin, end } => client.preedit(&*wire, text, begin, end),
                TextEvent::Commit(text) => client.commit_string(&*wire, text),
                TextEvent::Delete { before, after } => {
                    client.delete_surrounding(&*wire, before, after);
                }
                TextEvent::Done(serial) => client.done(&*wire, serial),
                TextEvent::Enter(_) | TextEvent::Leave => {}
            },
            None => {}
        }
    }

    /// The loop ends: the text inputs go and no window is reached again.
    fn release(&self) {
        let windows = self
            .objects
            .with(|objects| {
                for held in objects.inputs.drain(..) {
                    held.wire.object.destroy();
                }
                if let Some((_, manager)) = objects.manager.take() {
                    manager.destroy();
                }
                std::mem::take(&mut objects.windows)
            })
            .unwrap_or_default();
        contain_owner_callback(|| drop(windows));
    }
}

/// A delivered event and the client it goes to.
enum Routed {
    Enter(Rc<TextInputClient>, Rc<SeatWire>),
    Leave(Left),
    Event(Rc<TextInputClient>, Rc<SeatWire>, TextEvent),
}

/// A seat's text input leaving a window: the window's client, the text
/// input that left, and another seat's still in the window, which the
/// client serves next when the one that left was the one it served.
struct Left {
    client: Rc<TextInputClient>,
    wire: Rc<SeatWire>,
    next: Option<Rc<SeatWire>>,
}

impl Left {
    /// Tell the client, with no cell borrowed.
    fn deliver(self) {
        if self.client.left(&*self.wire)
            && let Some(next) = self.next
        {
            self.client.entered(next);
        }
    }
}

impl TextObjects {
    /// `wire` left window `window`, no longer holding it: what the window's
    /// client hears.
    fn leaving(&self, window: WindowId, wire: Rc<SeatWire>) -> Option<Left> {
        let client = self.window(window)?.client()?;
        let next = self
            .inputs
            .iter()
            .find(|held| held.window == Some(window) && !Rc::ptr_eq(&held.wire, &wire))
            .map(|held| Rc::clone(&held.wire));
        Some(Left { client, wire, next })
    }
}

fn make_input(
    manager: &ZwpTextInputManagerV3,
    seat: &WlSeat,
    queue: &QueueHandle<WaylandState>,
    recorder: &Weak<dyn TextRecorder>,
) -> HeldInput {
    let object = manager.get_text_input(
        seat,
        queue,
        TextInputData {
            recorder: recorder.clone(),
        },
    );
    HeldInput {
        seat: seat.clone(),
        wire: Rc::new(SeatWire {
            object,
            commits: Cell::new(0),
        }),
        window: None,
    }
}

// ============================================================================
// The text position
// ============================================================================

/// The Wayland table's text position: each window's [`WindowText`].
#[derive(Debug)]
pub struct WaylandText;

impl<W: ?Sized> Text<W> for WaylandText {
    fn for_window(_window: &W) -> Arc<dyn TextInput> {
        Arc::new(WindowText::new())
    }
}

/// One window's text input: its client, made on the owner thread with the
/// window, and whether the compositor offers text input at all.
pub struct WindowText {
    client: OwnerCell<Option<Rc<TextInputClient>>>,
    offered: AtomicBool,
}

impl WindowText {
    fn new() -> Self {
        let client = OwnerCell::new();
        client.with(|client| *client = Some(TextInputClient::new()));
        Self {
            client,
            offered: AtomicBool::new(false),
        }
    }

    /// The window's client, on the owner thread.
    pub(crate) fn client(&self) -> Option<Rc<TextInputClient>> {
        self.client.with(|client| client.clone()).flatten()
    }

    fn set_offered(&self, offered: bool) {
        self.offered.store(offered, Ordering::Release);
    }
}

impl TextInput for WindowText {
    fn window_text_input(&self) -> Option<WindowTextInput> {
        if !self.offered.load(Ordering::Acquire) {
            return None;
        }
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

    fn seat_text(self: Arc<Self>) -> Option<Arc<dyn SeatWindowText>> {
        Some(self)
    }
}

impl SeatWindowText for WindowText {}

impl std::fmt::Debug for WindowText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowText")
            .field("offered", &self.offered.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}
