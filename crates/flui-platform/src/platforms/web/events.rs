//! DOM event → PlatformInput mapping
//!
//! Registers DOM event listeners on the canvas and converts browser events
//! into FLUI's owned PlatformInput types.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::geometry::Point;
use flui_platform_api::{
    EventTime,
    keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey},
    pointer::{
        ButtonChange, PenTool, PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo,
        PointerKind, PointerPosition, PointerRole, ScrollEvent,
    },
};

use crate::DispatchEventResult;
use crate::shared::pointer_assembly::{
    Assembled, ContactRegistry, DecodeError, GesturePhase, GestureReading, MoveBatch, NativeCancel,
    NativeClock, NativeContact, NativeReading, NativeSurface, PenAngles, RawSensors, ReadingAction,
    ReadingTime, decode,
};
use crate::shared::web_pinch::{
    CtrlWheel, GestureEventKind, GestureTracker, WheelRoute, WheelStep,
};
use crate::shared::web_text::key_composition;
use icu_normalizer::{ComposingNormalizerBorrowed, properties::CanonicalCombiningClassMapBorrowed};
use icu_properties::{CodePointMapData, props::GeneralCategory};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::{WindowExecutionState, shared::WindowCallbacks, traits::PlatformInput};

use super::{
    registration::{Registration, effective_focus},
    window::WebWindow,
};

/// Register all DOM event listeners on the canvas.
///
/// Failure rolls back the installed prefix through the registration's Drop.
pub(super) fn register_event_listeners(
    window: &WebWindow,
    transfer: &Arc<super::drag::WebTransfer>,
    registration: &mut Registration,
) -> Result<CanvasServices, crate::OpenWindowError> {
    let canvas = window.canvas();
    let callbacks = Arc::clone(window.callbacks());
    let contacts = Rc::new(RefCell::new(
        ContactRegistry::new(NativeClock::epoch_only()).with_reserved_contact(MOUSE_CONTACT),
    ));
    let pinch = Rc::new(Pinch {
        callbacks: Arc::clone(&callbacks),
        accepting: Rc::clone(&registration.accepting),
        contacts: Rc::clone(&contacts),
        wheel: RefCell::default(),
        gesture: RefCell::default(),
        focal: Cell::new(None),
    });
    let keys = Rc::new(KeyRoute {
        callbacks: Arc::clone(&callbacks),
        pinch: Rc::clone(&pinch),
        accepting: Rc::clone(&registration.accepting),
        composing: Rc::default(),
    });

    // The drag source learns of a press before the press is dispatched: the
    // runtime may start a session from inside that dispatch.
    let drag = super::drag::register(canvas, transfer, &callbacks, registration)?;
    let menu = Rc::new(MenuAnswer::default());
    register_pointer_events(canvas, &contacts, &pinch, &menu, registration)?;
    register_keyboard_events(canvas, &keys, registration)?;
    let focus = register_focus_events(window, registration)?;
    let text = super::text_input::WebTextInput::new(
        canvas,
        registration.editor_slot(),
        Rc::clone(&registration.accepting),
        Rc::clone(&keys),
        Rc::clone(&focus),
    );
    let visibility = Rc::new(window.visibility_sync(Rc::clone(&registration.accepting)));
    let observer = Rc::clone(&visibility);
    registration.listen(
        visibility.document.as_ref(),
        "visibilitychange",
        move |_| {
            observer.sync();
        },
    )?;
    register_wheel_events(canvas, &callbacks, &pinch, registration)?;
    register_gesture_events(canvas, &pinch, registration)?;
    register_context_menu(canvas, &menu, registration)?;
    register_layout_events(window, registration)?;
    register_appearance_events(&callbacks, registration)?;
    register_page_lifecycle_events(window, registration)?;
    register_fullscreen_events(window, registration)?;
    register_context_loss_events(window, registration)?;
    // Host registration calls can change native state before its listeners
    // exist. Reconcile after successful installation, before Created.
    focus.sync();
    visibility.sync();
    Ok(CanvasServices { drag, text })
}

/// What a canvas registration serves beside its listeners: its drag source
/// and target, and its text input.
pub(super) struct CanvasServices {
    pub(super) drag: super::drag::CanvasDrag,
    pub(super) text: Rc<super::text_input::WebTextInput>,
}

/// How a DOM key event reaches the window: from the canvas, or from the
/// text input's own editor.
pub(super) struct KeyRoute {
    callbacks: Arc<WindowCallbacks>,
    pinch: Rc<Pinch>,
    accepting: Rc<Cell<bool>>,
    /// Whether the window's text input composes now.
    pub(super) composing: Rc<Cell<bool>>,
}

impl KeyRoute {
    /// Deliver `event` as a key `state`, answering it with the result.
    pub(super) fn deliver(&self, event: &web_sys::Event, state: KeyState) {
        if !self.accepting.get() {
            return;
        }
        let input = convert_keyboard_event(event.unchecked_ref(), state, self.composing.get());
        // DOM getters can close this registration before callback admission.
        if !self.accepting.get() {
            return;
        }
        self.pinch.flush(event_time(event.time_stamp()));
        if !self.accepting.get() {
            return;
        }
        answer(event, self.callbacks.dispatch_input(input));
    }
}

// ==================== Layout (size) ====================

/// Keep the canvas's backing store and the window's tracked size in step
/// with its CSS box — see `WebWindow::new`'s "Size" section. A
/// `ResizeObserver` on the canvas sees every layout change (a viewport
/// resize under `100vw`, a container reflow, a style change); the window's
/// `resize` event is registered too because a browser-zoom change moves
/// the device pixel ratio without necessarily moving the box, and the
/// observer reports boxes, not ratios. `LayoutSync::sync` is a no-op when
/// nothing changed, so the two overlapping sources cost nothing.
fn register_layout_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let sync = Rc::new(window.layout_sync());
    let observer_sync = Rc::clone(&sync);
    registration.observe(move || observer_sync.sync())?;
    let browser = web_sys::window().expect("BUG: window construction verified browser environment");
    registration.listen(browser.as_ref(), "resize", move |_| sync.sync())
}

// ==================== Appearance ====================

/// Report `prefers-color-scheme` flips. The query list fires `change` only
/// when its `matches` flips, so every event is a real change; a browser that
/// cannot evaluate the query has nothing to report.
fn register_appearance_events(
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let Some(query) = super::window::dark_scheme_query() else {
        return Ok(());
    };
    let callbacks = Arc::clone(callbacks);
    registration.listen(query.as_ref(), "change", move |_| {
        callbacks.dispatch_appearance_changed();
    })
}

// ==================== Page lifecycle ====================

/// `freeze` and `resume` (Page Lifecycle): a frozen page runs no script at
/// all, so the window is `Suspended` from the last event it can handle until
/// `resume`. A browser without the API never raises them and the window stays
/// `Running`.
fn register_page_lifecycle_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let document = document_of(window);
    let execution = Rc::new(window.execution_sync());
    for (kind, state) in [
        ("freeze", WindowExecutionState::Suspended),
        ("resume", WindowExecutionState::Running),
    ] {
        let execution = Rc::clone(&execution);
        registration.listen(document.as_ref(), kind, move |_| execution.set(state))?;
    }
    Ok(())
}

// ==================== Fullscreen ====================

/// `fullscreenchange`: the browser answers `requestFullscreen`, Escape and the
/// user's own exit with this one event, so the published state is read again
/// here and nowhere else.
fn register_fullscreen_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let publisher = window.publisher();
    registration.listen(
        document_of(window).as_ref(),
        "fullscreenchange",
        move |_| {
            publisher.publish();
        },
    )
}

fn document_of(window: &WebWindow) -> web_sys::Document {
    window
        .canvas()
        .owner_document()
        .expect("BUG: window construction verified the canvas document")
}

// ==================== GPU context ====================

/// Tell the presentation when the canvas's WebGL context goes and comes back,
/// as the surface becoming unavailable and available again.
///
/// The browser takes a context away whenever it reclaims GPU memory, often from
/// a tab nobody is looking at. `preventDefault` on the loss is what asks the
/// browser to restore the context later; without it the context is gone for
/// good. Everything built on the lost context is dead either way, so the
/// restoration is reported too and the presentation rebuilds on it.
fn register_context_loss_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    for (kind, available) in [("webglcontextlost", false), ("webglcontextrestored", true)] {
        let callbacks = Arc::clone(window.callbacks());
        registration.listen(window.canvas().as_ref(), kind, move |event| {
            if !available {
                event.prevent_default();
            }
            callbacks.dispatch_surface_status_change(available);
        })?;
    }
    Ok(())
}

// ==================== Pointer Events ====================

fn register_pointer_events(
    canvas: &web_sys::HtmlCanvasElement,
    contacts: &Rc<RefCell<ContactRegistry>>,
    pinch: &Rc<Pinch>,
    menu: &Rc<MenuAnswer>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let active = Rc::clone(&registration.pointers);
    // DOM getters and application callbacks can synchronously dispatch another
    // pointer event. Listener captures are immutable; mutable state is scoped
    // inside the active-pointer cell instead of the JavaScript closure.
    // pointerdown
    {
        let active = Rc::clone(&active);
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let capture = canvas.clone();
        let accepting = Rc::clone(&registration.accepting);
        let custody = registration.capture_owner();
        let menu = Rc::clone(menu);
        registration.listen(canvas.as_ref(), "pointerdown", move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let buttons = pe.buttons();
            let secondary = pe.button() == SECONDARY_BUTTON;
            if !accepting.get() {
                return;
            }
            active.borrow_mut().insert(id, buttons);
            if !custody.admit(id) {
                active.borrow_mut().remove(&id);
                return;
            }
            // Admission precedes application callbacks, which can remove the
            // canvas or synchronously dispatch another terminal event.
            if let Err(error) = capture.set_pointer_capture(id) {
                tracing::debug!(?error, "browser pointer capture was not admitted");
            }
            if !accepting.get() {
                // The host can close before performing the native operation.
                // Release that outgoing capture unless a newer owner has
                // already admitted this pointer on the same actual canvas.
                custody.release(id);
                return;
            }
            let pressed = dispatch(&pinch, &pe, convert_pointer_down(&pe, &contacts));
            menu.record(secondary, pressed);
        })?;
    }

    // pointermove
    {
        let active = Rc::clone(&active);
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let accepting = Rc::clone(&registration.accepting);
        let menu = Rc::clone(menu);
        registration.listen(canvas.as_ref(), "pointermove", move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let buttons = pe.buttons();
            let secondary = pe.button() == SECONDARY_BUTTON;
            if !accepting.get() {
                return;
            }
            if let Some(held) = active.borrow_mut().get_mut(&id) {
                *held = buttons;
            }
            // A chorded secondary press opens the menu as a first one does.
            let pressed = dispatch(&pinch, &pe, convert_pointer_move(&pe, &contacts));
            menu.record(secondary, pressed);
        })?;
    }

    // A mouse or pen coming over the canvas and leaving it, outside a press
    // (a captured press sees its boundary events after its release): a pen
    // that leaves is out of range, and its contact ends. A touch enters at
    // its press and leaves at its release.
    for (kind, action) in [
        ("pointerenter", ReadingAction::Enter),
        ("pointerleave", ReadingAction::Leave),
    ] {
        let active = Rc::clone(&active);
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let accepting = Rc::clone(&registration.accepting);
        registration.listen(canvas.as_ref(), kind, move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let hovers = matches!(pe.pointer_type().as_str(), "mouse" | "pen");
            if !accepting.get() || !hovers || active.borrow().contains_key(&id) {
                return;
            }
            dispatch(
                &pinch,
                &pe,
                assemble(&contacts, &pointer_reading(&pe, action)),
            );
        })?;
    }

    let loss = Rc::new(PointerLoss {
        contacts: Rc::clone(contacts),
        pinch: Rc::clone(pinch),
        active: Rc::clone(&active),
        custody: registration.capture_owner(),
    });
    let browser = web_sys::window().expect("BUG: window construction verified browser");
    {
        let loss = Rc::clone(&loss);
        // The page itself losing the OS focus, not focus moving between its
        // elements.
        registration.listen(browser.as_ref(), "blur", move |e: web_sys::Event| {
            let own = e
                .target()
                .is_some_and(|target| target.dyn_ref::<web_sys::Window>().is_some());
            if own {
                loss.cancel(event_time(e.time_stamp()), NativeCancel::FocusLost);
            }
        })?;
    }
    if let Some(document) = browser.document() {
        let hidden = document.clone();
        registration.listen(
            document.as_ref(),
            "visibilitychange",
            move |e: web_sys::Event| {
                if hidden.hidden() {
                    loss.cancel(event_time(e.time_stamp()), NativeCancel::FocusLost);
                }
            },
        )?;
    }

    // pointerup
    {
        let active = Rc::clone(&active);
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let accepting = Rc::clone(&registration.accepting);
        let custody = registration.capture_owner();
        registration.listen(canvas.as_ref(), "pointerup", move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let was_active = active.borrow_mut().remove(&id).is_some();
            if !was_active {
                return;
            }
            custody.release(id);
            if !accepting.get() {
                return;
            }
            dispatch(&pinch, &pe, convert_pointer_up(&pe, &contacts));
        })?;
    }

    // A browser gesture takeover is a terminal event even when no release
    // will follow. Deliver it through the same public input callback.
    {
        let active = Rc::clone(&active);
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let accepting = Rc::clone(&registration.accepting);
        let custody = registration.capture_owner();
        registration.listen(
            canvas.as_ref(),
            "pointercancel",
            move |e: web_sys::Event| {
                let pe: web_sys::PointerEvent = e.unchecked_into();
                let id = pe.pointer_id();
                if !accepting.get() {
                    return;
                }
                let was_active = active.borrow_mut().remove(&id).is_some();
                if !was_active {
                    return;
                }
                custody.release(id);
                if !accepting.get() {
                    return;
                }
                dispatch(
                    &pinch,
                    &pe,
                    convert_pointer_cancel(&pe, NativeCancel::Platform, &contacts),
                );
            },
        )?;
    }

    {
        let contacts = Rc::clone(contacts);
        let pinch = Rc::clone(pinch);
        let accepting = Rc::clone(&registration.accepting);
        let custody = registration.capture_owner();
        registration.listen(
            canvas.as_ref(),
            "lostpointercapture",
            move |e: web_sys::Event| {
                let pe: web_sys::PointerEvent = e.unchecked_into();
                let id = pe.pointer_id();
                if !accepting.get() {
                    return;
                }
                // Up/cancel removes admission before releasing capture. The loss
                // notification following that terminal edge must stay inert.
                let was_active = active.borrow_mut().remove(&id).is_some();
                if !was_active {
                    return;
                }
                custody.forget(id);
                dispatch(
                    &pinch,
                    &pe,
                    convert_pointer_cancel(&pe, NativeCancel::CaptureLost, &contacts),
                );
            },
        )?;
    }
    Ok(())
}

// ==================== Keyboard Events ====================

/// Deliver assembled pointer input while the registration still accepts it:
/// a callback can close the window between two events of one reading. A
/// pinch whose deadline `event` passed ends first. Returns whether the
/// application prevented the default of a press among them.
fn dispatch(pinch: &Pinch, event: &web_sys::Event, inputs: Assembled) -> Option<bool> {
    pinch.flush(event_time(event.time_stamp()));
    let mut pressed = None;
    for input in inputs {
        if !pinch.accepting.get() {
            break;
        }
        let press = matches!(
            input,
            PlatformInput::Pointer(
                PointerEvent::Down(_) | PointerEvent::ButtonChange(ButtonChange::Pressed(_))
            )
        );
        let result = pinch.callbacks.dispatch_input(input);
        if press {
            pressed = Some(result.default_prevented);
        }
    }
    pressed
}

/// `MouseEvent.button` of the secondary button.
const SECONDARY_BUTTON: i16 = 2;

/// How the browser's context menu is answered: as the application answered
/// the secondary press that opened it (`contextmenu` follows the press, or
/// on Windows its release). A menu no secondary press opened (the keyboard's
/// menu key) stays the application's, its default prevented.
#[derive(Default)]
struct MenuAnswer {
    /// Whether the last secondary press had its default prevented, until a
    /// menu takes it.
    prevented: Cell<Option<bool>>,
}

impl MenuAnswer {
    fn record(&self, secondary: bool, prevented: Option<bool>) {
        if secondary && prevented.is_some() {
            self.prevented.set(prevented);
        }
    }
}

/// End every pointer sequence of the canvas: the page lost the OS focus or
/// was hidden, and the browser may never send their release or capture loss
/// (a press released after Alt+Tab reaches no page). Without it, the next
/// press of a button the registry still holds would be no press at all.
struct PointerLoss {
    contacts: Rc<RefCell<ContactRegistry>>,
    pinch: Rc<Pinch>,
    active: Rc<RefCell<std::collections::HashMap<i32, u16>>>,
    custody: super::registration::PointerCaptureOwner,
}

impl PointerLoss {
    fn cancel(&self, time: EventTime, reason: NativeCancel) {
        if !self.pinch.accepting.get() {
            return;
        }
        let ids: Vec<i32> = self.active.borrow_mut().drain().map(|(id, _)| id).collect();
        // Admission ends before release, which can dispatch the capture loss.
        for &id in &ids {
            self.custody.forget(id);
        }
        let ended = self
            .contacts
            .borrow_mut()
            .cancel_surface(SURFACE, time, reason);
        *self.pinch.wheel.borrow_mut() = CtrlWheel::default();
        *self.pinch.gesture.borrow_mut() = GestureTracker::default();
        self.pinch.deliver(ended.into_iter().collect(), None);
        for id in ids {
            self.custody.release(id);
        }
    }
}

/// Apply a dispatch result to the DOM event it answers, synchronously, while
/// the browser still waits on it: `propagate: false` stops it reaching the
/// canvas's ancestors (same-target listeners still run), and
/// `default_prevented` cancels its default where it is cancelable.
pub(super) fn answer(event: &web_sys::Event, result: DispatchEventResult) {
    if !result.propagate {
        event.stop_propagation();
    }
    if result.default_prevented {
        event.prevent_default();
    }
}

/// A DOM `timeStamp` (milliseconds on the page's high-resolution timeline)
/// on the event timeline.
fn event_time(millis: f64) -> EventTime {
    EventTime::from_nanos((millis * 1_000_000.0) as u64)
}

fn register_keyboard_events(
    canvas: &web_sys::HtmlCanvasElement,
    keys: &Rc<KeyRoute>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
        let keys = Rc::clone(keys);
        let target = canvas.clone();
        registration.listen(canvas.as_ref(), kind, move |event| {
            if !effective_focus(&target) || event.target().as_ref() != Some(target.as_ref()) {
                return;
            }
            keys.deliver(&event, state);
        })?;
    }
    Ok(())
}

// ==================== Focus Events ====================

/// Whether `event` only moves focus within the canvas's focus area (between
/// the canvas and its text editor), which the window does not see as a
/// focus change.
pub(super) fn is_focus_handoff(
    canvas: &web_sys::HtmlCanvasElement,
    event: &web_sys::Event,
) -> bool {
    event.type_() == "blur"
        && event
            .dyn_ref::<web_sys::FocusEvent>()
            .and_then(web_sys::FocusEvent::related_target)
            .is_some_and(|next| super::registration::is_owned_focus(canvas, &next))
}

fn register_focus_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<Rc<super::window::FocusSync>, crate::OpenWindowError> {
    let focus = Rc::new(window.focus_sync());
    let browser = web_sys::window().expect("BUG: construction verified browser");
    for target in [window.canvas().as_ref(), browser.as_ref()] {
        for kind in ["focus", "blur"] {
            let focus = Rc::clone(&focus);
            let canvas = window.canvas().clone();
            registration.listen(target, kind, move |event| {
                if !is_focus_handoff(&canvas, &event) {
                    focus.sync();
                }
            })?;
        }
    }
    for (kind, hovered) in [("pointerenter", true), ("pointerleave", false)] {
        let callbacks = Arc::clone(window.callbacks());
        registration.listen(window.canvas().as_ref(), kind, move |_| {
            callbacks.dispatch_hover_status_change(hovered);
        })?;
    }
    Ok(focus)
}

// ==================== Wheel Events ====================

/// The contact label of a trackpad pinch: above every DOM `pointerId`, which
/// is a 32-bit integer.
const PINCH_CONTACT: NativeContact = NativeContact(1 << 32);

/// The trackpad pinch of one registration, from Control-held wheel events or
/// WebKit gesture events (`shared::web_pinch`), and how it reaches the
/// window's input callback.
///
/// No borrow of its cells is held across a dispatch: an application callback
/// can dispatch another DOM event at the canvas synchronously.
struct Pinch {
    callbacks: Arc<WindowCallbacks>,
    accepting: Rc<Cell<bool>>,
    contacts: Rc<RefCell<ContactRegistry>>,
    wheel: RefCell<CtrlWheel>,
    gesture: RefCell<GestureTracker>,
    /// The last focal point, which an End or a cancellation keeps.
    focal: Cell<Option<Point>>,
}

impl Pinch {
    /// The input one gesture reading assembles into, at `focal` when given.
    fn assemble(
        &self,
        reading: GestureReading,
        time: EventTime,
        focal: Option<Point>,
        modifiers: Modifiers,
    ) -> Assembled {
        if focal.is_some() {
            self.focal.set(focal);
        }
        let mut native = NativeReading::new(
            PINCH_CONTACT,
            PointerKind::Trackpad,
            ReadingAction::Gesture(reading),
            ReadingTime::epoch(time),
        )
        .on(SURFACE)
        .with_primary(true)
        .with_modifiers(modifiers);
        if let Some(point) = self.focal.get() {
            native = native.at(point.x, point.y, 1.0);
        }
        assemble(&self.contacts, &native)
    }

    /// Deliver `inputs`, answering `event` (when given) with each result,
    /// and return the first result.
    fn deliver(
        &self,
        inputs: Assembled,
        event: Option<&web_sys::Event>,
    ) -> Option<DispatchEventResult> {
        let mut first = None;
        for input in inputs {
            if !self.accepting.get() {
                break;
            }
            let result = self.callbacks.dispatch_input(input);
            if let Some(event) = event {
                answer(event, result);
            }
            first.get_or_insert(result);
        }
        first
    }

    /// Deliver the End of a Control-held pinch whose deadline `now` passed,
    /// stamped with that deadline.
    fn flush(&self, now: EventTime) {
        let due = self.wheel.borrow_mut().expire(now);
        if let Some((end, at)) = due {
            let inputs = self.assemble(end, at, None, Modifiers::NONE);
            self.deliver(inputs, None);
        }
    }

    /// Deliver one step of a pinch for `event`. The sequence's first step is
    /// delivered alone first: when the application does not prevent its
    /// default, `refuse` cancels the pinch and the browser keeps the event
    /// (its page zoom), and nothing else of the step is delivered.
    fn step(
        &self,
        inputs: Assembled,
        first: bool,
        event: &web_sys::Event,
        refuse: impl FnOnce() -> Option<GestureReading>,
        time: EventTime,
    ) {
        let mut inputs = inputs.into_iter();
        if first {
            let Some(head) = inputs.next() else {
                return;
            };
            if !self.accepting.get() {
                return;
            }
            let result = self.callbacks.dispatch_input(head);
            if !result.default_prevented {
                if let Some(cancel) = refuse() {
                    let cancelled = self.assemble(cancel, time, None, Modifiers::NONE);
                    self.deliver(cancelled, None);
                }
                return;
            }
            answer(event, result);
        }
        self.deliver(inputs.collect(), Some(event));
    }
}

fn register_wheel_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    pinch: &Rc<Pinch>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let timer = {
        let pinch = Rc::clone(pinch);
        registration.timer(move || {
            // The timer runs no earlier than the deadline it was armed for.
            let deadline = pinch.wheel.borrow().deadline();
            if let Some(deadline) = deadline {
                let now =
                    performance_now().max(deadline.checked_add(NANOSECOND).unwrap_or(deadline));
                pinch.flush(now);
            }
        })
    };
    let callbacks = Arc::clone(callbacks);
    let pinch = Rc::clone(pinch);
    let target = canvas.clone();
    registration.listen(canvas.as_ref(), "wheel", move |e: web_sys::Event| {
        let we: &web_sys::WheelEvent = e.unchecked_ref();
        let step = WheelStep {
            ctrl: we.ctrl_key(),
            mode: we.delta_mode(),
            dx: we.delta_x(),
            dy: we.delta_y(),
            scale: web_sys::window().map_or(1.0, |window| window.device_pixel_ratio()),
            time: event_time(we.time_stamp()),
        };
        let ended = {
            let mut wheel = pinch.wheel.borrow_mut();
            if step.ctrl {
                wheel.expire(step.time)
            } else {
                wheel.finish(step.time)
            }
        };
        if let Some((end, at)) = ended {
            let inputs = pinch.assemble(end, at, None, Modifiers::NONE);
            pinch.deliver(inputs, None);
        }
        let route = pinch.wheel.borrow_mut().route(step);
        match route {
            WheelRoute::Refused => {}
            WheelRoute::Scroll => {
                if let Some(input) = convert_wheel_event(we, &target, &pinch.contacts) {
                    answer(&e, callbacks.dispatch_input(input));
                }
            }
            WheelRoute::Pinch(reading) => {
                let Ok(focal) = PointerPosition::try_new(wheel_position(we, &target)) else {
                    return;
                };
                // Chromium sets Control on a pinch it encodes as a wheel; it
                // is not a key the user holds.
                let modifiers = observed_modifiers(
                    [we.shift_key(), false, we.alt_key(), we.meta_key()],
                    |name| name != "Control" && we.get_modifier_state(name),
                );
                let inputs = pinch.assemble(reading, step.time, Some(focal.get()), modifiers);
                let first = reading.phase == GesturePhase::Began;
                pinch.step(
                    inputs,
                    first,
                    &e,
                    || pinch.wheel.borrow_mut().refuse(),
                    step.time,
                );
                let deadline = pinch.wheel.borrow().deadline();
                if let Some(deadline) = deadline {
                    timer.arm(deadline.saturating_duration_since(performance_now()) + TIMER_SLACK);
                }
            }
        }
    })
}

/// One nanosecond on the event timeline.
const NANOSECOND: std::time::Duration = std::time::Duration::from_nanos(1);
/// How much later than a deadline its timer is armed for, so a timer the
/// browser runs at the deadline's millisecond does not find it pending.
const TIMER_SLACK: std::time::Duration = std::time::Duration::from_millis(1);

/// The page's high-resolution time now, on the event timeline.
fn performance_now() -> EventTime {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(EventTime::from_nanos(0), |performance| {
            event_time(performance.now())
        })
}

// ==================== WebKit Gesture Events ====================

/// WebKit's `GestureEvent`, which web-sys does not bind.
#[wasm_bindgen]
extern "C" {
    type GestureEventValues;
    #[wasm_bindgen(method, getter, catch)]
    fn scale(this: &GestureEventValues) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch)]
    fn rotation(this: &GestureEventValues) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch, js_name = clientX)]
    fn gesture_client_x(this: &GestureEventValues) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch, js_name = clientY)]
    fn gesture_client_y(this: &GestureEventValues) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, js_name = shiftKey)]
    fn gesture_shift(this: &GestureEventValues) -> Option<bool>;
    #[wasm_bindgen(method, getter, js_name = altKey)]
    fn gesture_alt(this: &GestureEventValues) -> Option<bool>;
    #[wasm_bindgen(method, getter, js_name = metaKey)]
    fn gesture_meta(this: &GestureEventValues) -> Option<bool>;
    #[wasm_bindgen(method, getter, js_name = ctrlKey)]
    fn gesture_ctrl(this: &GestureEventValues) -> Option<bool>;
}

/// Whether this browser dispatches WebKit gesture events.
pub(super) fn has_gesture_events() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window, &JsValue::from_str("GestureEvent")).unwrap_or(false)
    })
}

fn register_gesture_events(
    canvas: &web_sys::HtmlCanvasElement,
    pinch: &Rc<Pinch>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    for (name, kind) in [
        ("gesturestart", GestureEventKind::Start),
        ("gesturechange", GestureEventKind::Change),
        ("gestureend", GestureEventKind::End),
    ] {
        let pinch = Rc::clone(pinch);
        let target = canvas.clone();
        registration.listen(canvas.as_ref(), name, move |e: web_sys::Event| {
            let values: &GestureEventValues = e.unchecked_ref();
            let time = event_time(e.time_stamp());
            pinch.flush(time);
            // WebKit on a touch screen reports a two-finger pinch as gesture
            // events beside the touches themselves, which the application
            // already has: delivering both would scale it twice. A gesture
            // that starts over an open touch is left to the touches.
            if kind == GestureEventKind::Start
                && pinch
                    .contacts
                    .borrow()
                    .has_open_kind(SURFACE, PointerKind::Touch)
            {
                *pinch.gesture.borrow_mut() = GestureTracker::default();
                return;
            }
            let scale = values.scale().unwrap_or(f64::NAN);
            let rotation = values.rotation().unwrap_or(f64::NAN);
            let focal = values
                .gesture_client_x()
                .ok()
                .zip(values.gesture_client_y().ok())
                .and_then(|(x, y)| client_to_canvas(&target, x, y))
                .and_then(|point| PointerPosition::try_new(point).ok())
                .map(PointerPosition::get);
            let modifiers = [
                (values.gesture_shift(), Modifiers::SHIFT),
                (values.gesture_ctrl(), Modifiers::CONTROL),
                (values.gesture_alt(), Modifiers::ALT),
                (values.gesture_meta(), Modifiers::META),
            ]
            .into_iter()
            .filter(|(held, _)| held.unwrap_or(false))
            .fold(Modifiers::NONE, |all, (_, flag)| all | flag);
            let readings = pinch.gesture.borrow_mut().step(kind, scale, rotation);
            let mut inputs = Assembled::new();
            for reading in readings.into_iter().flatten() {
                inputs.extend(pinch.assemble(reading, time, focal, modifiers));
            }
            if pinch.gesture.borrow().is_refused() {
                return;
            }
            pinch.step(
                inputs,
                kind == GestureEventKind::Start,
                &e,
                || pinch.gesture.borrow_mut().refuse(),
                time,
            );
        })?;
    }
    Ok(())
}

/// Viewport coordinates as canvas-local ones, through the same untransformed
/// box geometry the wheel path uses; `None` for a transformed ancestry,
/// whose local coordinates a bounding rectangle cannot recover.
fn client_to_canvas(canvas: &web_sys::HtmlCanvasElement, x: f64, y: f64) -> Option<Point> {
    let origin = untransformed_origin(canvas)?;
    Some(Point::new(x - origin.x, y - origin.y))
}

// ==================== Context Menu Block ====================

/// The browser's context menu over the canvas, answered as [`MenuAnswer`]
/// says.
fn register_context_menu(
    canvas: &web_sys::HtmlCanvasElement,
    menu: &Rc<MenuAnswer>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let menu = Rc::clone(menu);
    registration.listen(canvas.as_ref(), "contextmenu", move |e: web_sys::Event| {
        let secondary = e
            .dyn_ref::<web_sys::MouseEvent>()
            .is_some_and(|mouse| mouse.button() == SECONDARY_BUTTON);
        let answered = if secondary {
            menu.prevented.take()
        } else {
            None
        };
        if answered.unwrap_or(true) {
            e.prevent_default();
        }
    })
}

// ==================== Event Conversion ====================

// CSSOM View defines these offsets as doubles. web-sys exposes the older
// integer signature, which discards subpixel positions before Rust receives
// them. Bind the actual DOM getters with their floating-point result type.
#[wasm_bindgen]
extern "C" {
    // DOMString can contain unpaired surrogates. A Rust String getter would
    // replace them before the source-specific admission check can refuse them.
    type Utf16KeyboardEvent;
    #[wasm_bindgen(method, getter, js_name = key)]
    fn key_utf16(this: &Utf16KeyboardEvent) -> js_sys::JsString;

    type PreciseMouseEvent;
    #[wasm_bindgen(method, getter, js_name = offsetX)]
    fn offset_x_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = offsetY)]
    fn offset_y_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = clientX)]
    fn client_x_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = clientY)]
    fn client_y_f64(this: &PreciseMouseEvent) -> f64;

    type ExtendedPointerEvent;
    #[wasm_bindgen(method, getter, js_name = width)]
    fn width_f64(this: &ExtendedPointerEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = height)]
    fn height_f64(this: &ExtendedPointerEvent) -> f64;
    #[wasm_bindgen(method, getter, catch, js_name = altitudeAngle)]
    fn altitude_angle(this: &ExtendedPointerEvent) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch, js_name = azimuthAngle)]
    fn azimuth_angle(this: &ExtendedPointerEvent) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch, js_name = twist)]
    fn twist_degrees(this: &ExtendedPointerEvent) -> Result<f64, JsValue>;
    // Pointer Events 3 allows fractional tilt; web-sys reads it as an integer.
    #[wasm_bindgen(method, getter, catch, js_name = tiltX)]
    fn tilt_x_degrees(this: &ExtendedPointerEvent) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, getter, catch, js_name = tiltY)]
    fn tilt_y_degrees(this: &ExtendedPointerEvent) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getCoalescedEvents)]
    fn coalesced_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getPredictedEvents)]
    fn predicted_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
}

pub(super) fn pointer_position(event: &web_sys::MouseEvent) -> Point {
    let event: &PreciseMouseEvent = event.unchecked_ref();
    Point::new(event.offset_x_f64(), event.offset_y_f64())
}

/// MouseEvent implementations can round wheel offsets even when the canvas
/// origin is fractional. Recover its padding-edge position from viewport
/// coordinates only when the whole ancestry has no coordinate transform.
/// A bounding rectangle cannot invert a rotation, skew or perspective;
/// those shapes keep the browser's own local offset instead of guessing.
fn wheel_position(event: &web_sys::WheelEvent, canvas: &web_sys::HtmlCanvasElement) -> Point {
    untransformed_wheel_position(event, canvas).unwrap_or_else(|| pointer_position(event))
}

fn untransformed_wheel_position(
    event: &web_sys::WheelEvent,
    canvas: &web_sys::HtmlCanvasElement,
) -> Option<Point> {
    let origin = untransformed_origin(canvas)?;
    let source: &PreciseMouseEvent = event.unchecked_ref();
    Some(Point::new(
        source.client_x_f64() - origin.x,
        source.client_y_f64() - origin.y,
    ))
}

/// The viewport position of the canvas's padding edge, when its whole
/// ancestry has no coordinate transform or zoom.
fn untransformed_origin(canvas: &web_sys::HtmlCanvasElement) -> Option<Point> {
    let window = web_sys::window()?;
    let canvas_style = window.get_computed_style(canvas).ok()??;
    let mut ancestor = Some(canvas.unchecked_ref::<web_sys::Element>().clone());
    while let Some(element) = ancestor {
        let style = window.get_computed_style(&element).ok()??;
        for property in ["transform", "rotate", "scale", "translate", "perspective"] {
            if style.get_property_value(property).ok()? != "none" {
                return None;
            }
        }
        if !matches!(
            style.get_property_value("zoom").ok()?.as_str(),
            "1" | "normal"
        ) {
            return None;
        }
        ancestor = element.parent_element();
    }
    let border_left = canvas_style.get_property_value("border-left-width").ok()?;
    let border_top = canvas_style.get_property_value("border-top-width").ok()?;
    let border_left: f64 = border_left.strip_suffix("px")?.parse().ok()?;
    let border_top: f64 = border_top.strip_suffix("px")?.parse().ok()?;
    let rect = canvas.get_bounding_client_rect();
    Some(Point::new(
        rect.left() + border_left,
        rect.top() + border_top,
    ))
}

/// Translate the DOM `MouseEvent.buttons` bitmask into `PointerButtons`.
///
/// This is the W3C field itself, so no derivation is needed: the browser
/// already reports the set held *after* the event. Reporting an empty set for
/// an in-contact move would make the framework classify a drag as a hover, so
/// no gesture recognizer would ever see it.
///
/// Bit values are fixed by the UI Events spec: 1 primary, 2 secondary,
/// 4 auxiliary, 8 back and 16 forward.
fn buttons_from_mask(mask: u16) -> PointerButtons {
    [
        (1, PointerButton::PRIMARY),
        (2, PointerButton::SECONDARY),
        (4, PointerButton::AUXILIARY),
        (8, PointerButton::BACK),
        (16, PointerButton::FORWARD),
        (32, PointerButton::PRIMARY),
    ]
    .into_iter()
    .filter(|(bit, _)| mask & bit != 0)
    .map(|(_, button)| button)
    .collect()
}

fn map_button(button: i16) -> Option<PointerButton> {
    match button {
        0 | 5 => Some(PointerButton::PRIMARY),
        1 => Some(PointerButton::AUXILIARY),
        2 => Some(PointerButton::SECONDARY),
        3 => Some(PointerButton::BACK),
        4 => Some(PointerButton::FORWARD),
        _ => None,
    }
}

/// Each canvas registration serves one surface.
const SURFACE: NativeSurface = NativeSurface(0);

/// The contact label of the mouse: a page has one mouse pointer, whose
/// `pointerId` differs between engines (1 in Chromium and WebKit, 0 in
/// Gecko), and a wheel event names none. Above every DOM `pointerId` and
/// apart from [`PINCH_CONTACT`]; the registry reserves pointer identity one
/// for it, so wheel and pointer events share one mouse.
const MOUSE_CONTACT: NativeContact = NativeContact((1 << 32) + 1);

/// The contact's label: the mouse's own, or the DOM `pointerId`.
fn pointer_label(pe: &web_sys::PointerEvent) -> NativeContact {
    if pe.pointer_type() == "mouse" {
        return MOUSE_CONTACT;
    }
    NativeContact(u64::from(u32::from_ne_bytes(pe.pointer_id().to_ne_bytes())))
}

fn pointer_kind(pe: &web_sys::PointerEvent) -> PointerKind {
    match pe.pointer_type().as_str() {
        "mouse" => PointerKind::Mouse,
        "pen" => PointerKind::Pen {
            tool: if pe.button() == 5 || pe.buttons() & 32 != 0 {
                PenTool::Eraser
            } else {
                PenTool::Tip
            },
        },
        "touch" => PointerKind::Touch,
        _ => PointerKind::Unknown,
    }
}

/// The sensors a touch or pen event reports. DOM has no per-device sensor
/// capability query: reported touch and pen pressure is kept, including zero,
/// without treating a mouse fallback as force. Values the browser reports
/// slightly out of range are clamped.
fn pointer_sensors(pe: &web_sys::PointerEvent) -> RawSensors {
    let mut sensors = RawSensors::default();
    let kind = pe.pointer_type();
    if !matches!(kind.as_str(), "touch" | "pen") {
        return sensors;
    }
    let pressure = f64::from(pe.pressure());
    sensors.pressure = pressure.is_finite().then(|| pressure.clamp(0.0, 1.0));
    let extended: &ExtendedPointerEvent = pe.unchecked_ref();
    let (width, height) = (extended.width_f64(), extended.height_f64());
    sensors.contact_size =
        (width.is_finite() && height.is_finite() && width >= 0.0 && height >= 0.0)
            .then_some((width, height));
    if kind == "pen" {
        let tangential = f64::from(pe.tangential_pressure());
        sensors.tangential_pressure = tangential.is_finite().then(|| tangential.clamp(-1.0, 1.0));
        let altitude = extended
            .altitude_angle()
            .ok()
            .filter(|altitude| (0.0..=core::f64::consts::FRAC_PI_2).contains(altitude));
        let azimuth = extended
            .azimuth_angle()
            .ok()
            .filter(|azimuth| azimuth.is_finite());
        sensors.angles = if altitude.is_some() || azimuth.is_some() {
            Some(PenAngles::Spherical { altitude, azimuth })
        } else {
            pen_tilt(extended)
        };
        sensors.twist_degrees = extended
            .twist_degrees()
            .ok()
            .filter(|degrees| (0.0..360.0).contains(degrees));
    }
    sensors
}

/// The pen's tilt planes, for a browser that reports `tiltX`/`tiltY` but not
/// the spherical angles (Pointer Events 2): degrees within `[-90, 90]`.
fn pen_tilt(event: &ExtendedPointerEvent) -> Option<PenAngles> {
    let plane = |value: Result<f64, JsValue>| {
        value
            .ok()
            .filter(|degrees| (-90.0..=90.0).contains(degrees))
    };
    Some(PenAngles::TiltDegrees {
        x: plane(event.tilt_x_degrees())?,
        y: plane(event.tilt_y_degrees())?,
    })
}

/// A DOM pointer event as a reading of its contact, in CSS (logical) pixels.
fn pointer_reading(pe: &web_sys::PointerEvent, action: ReadingAction) -> NativeReading {
    let point = pointer_position(pe);
    NativeReading::new(
        pointer_label(pe),
        pointer_kind(pe),
        action,
        ReadingTime::epoch(EventTime::from_nanos(
            (pe.time_stamp() * 1_000_000.0) as u64,
        )),
    )
    .on(SURFACE)
    .at(point.x, point.y, 1.0)
    .with_primary(pe.is_primary())
    .with_buttons(buttons_from_mask(pe.buttons()))
    .with_modifiers(extract_modifiers_from_mouse(pe))
    .with_sensors(pointer_sensors(pe))
}

fn assemble(contacts: &RefCell<ContactRegistry>, reading: &NativeReading) -> Assembled {
    decode(&mut contacts.borrow_mut(), reading).unwrap_or_else(DecodeError::into_inputs)
}

fn convert_pointer_down(
    pe: &web_sys::PointerEvent,
    contacts: &RefCell<ContactRegistry>,
) -> Assembled {
    let Some(button) = map_button(pe.button()) else {
        return Assembled::new();
    };
    assemble(contacts, &pointer_reading(pe, ReadingAction::Press(button)))
}

fn convert_pointer_up(
    pe: &web_sys::PointerEvent,
    contacts: &RefCell<ContactRegistry>,
) -> Assembled {
    let Some(button) = map_button(pe.button()) else {
        return Assembled::new();
    };
    assemble(
        contacts,
        &pointer_reading(pe, ReadingAction::Release(button)),
    )
}

fn convert_pointer_cancel(
    pe: &web_sys::PointerEvent,
    reason: NativeCancel,
    contacts: &RefCell<ContactRegistry>,
) -> Assembled {
    assemble(
        contacts,
        &pointer_reading(pe, ReadingAction::Cancel(reason)),
    )
}

/// Pass the browser's coalesced or predicted events of `pe`'s own contact
/// to `push`.
fn batch_events(
    pe: &web_sys::PointerEvent,
    values: Result<js_sys::Array, JsValue>,
    mut push: impl FnMut(NativeReading),
) {
    let Ok(values) = values else {
        return;
    };
    let (id, kind) = (pe.pointer_id(), pe.pointer_type());
    values
        .iter()
        .filter_map(|value| value.dyn_into::<web_sys::PointerEvent>().ok())
        .filter(|sample| sample.pointer_id() == id && sample.pointer_type() == kind)
        .for_each(|sample| push(pointer_reading(&sample, ReadingAction::Move)));
}

/// A `pointermove`: a chorded button change when `pe.button()` names a button
/// whose held state differs from the contact's, otherwise a move carrying the
/// browser's coalesced and predicted events.
fn convert_pointer_move(
    pe: &web_sys::PointerEvent,
    contacts: &RefCell<ContactRegistry>,
) -> Assembled {
    let reported = buttons_from_mask(pe.buttons());
    let held = contacts.borrow().buttons(SURFACE, None, pointer_label(pe));
    if let (Some(held), Some(button)) = (held, map_button(pe.button()))
        && held.contains(button) != reported.contains(button)
    {
        let action = if reported.contains(button) {
            ReadingAction::Press(button)
        } else {
            ReadingAction::Release(button)
        };
        return assemble(contacts, &pointer_reading(pe, action));
    }
    let extended: &ExtendedPointerEvent = pe.unchecked_ref();
    let mut batch = MoveBatch::new();
    batch_events(pe, extended.coalesced_events(), |reading| {
        batch.push_coalesced(reading);
    });
    batch_events(pe, extended.predicted_events(), |reading| {
        batch.push_predicted(reading);
    });
    batch
        .decode(
            &mut contacts.borrow_mut(),
            &pointer_reading(pe, ReadingAction::Move),
        )
        .unwrap_or_else(DecodeError::into_inputs)
}

/// Convert a DOM `WheelEvent` to a W3C `PointerEvent::Scroll`.
///
/// The DOM already speaks the cross-backend wheel convention (positive =
/// content scrolls down/right; `DOM_DELTA_PIXEL` deltas are CSS = logical
/// pixels), so `from_web` is a passthrough that only maps `deltaMode` onto
/// the `ScrollDelta` variants — kept in `crate::shared::scroll` so this
/// boundary appears in the same sign/unit table (and executing contract
/// tests) as the backends that DO have to flip or rescale.
///
/// A wheel event names no pointer: it is the mouse's, the contact pointer
/// events of the mouse open, with pointer identity one (reserved for it, so
/// no touch or pen takes it), at the wheel event's own position. No scroll
/// phase is guessed.
fn convert_wheel_event(
    we: &web_sys::WheelEvent,
    canvas: &web_sys::HtmlCanvasElement,
    contacts: &RefCell<ContactRegistry>,
) -> Option<PlatformInput> {
    let delta =
        crate::shared::scroll::from_web(we.delta_mode(), we.delta_x(), we.delta_y()).ok()?;
    // DOM getters run before the registry is borrowed: they can reenter.
    let position = PointerPosition::try_new(wheel_position(we, canvas)).ok()?;
    let time = event_time(we.time_stamp());
    let modifiers = extract_modifiers_from_mouse(we);
    // Before the mouse's first pointer event its contact is not open yet.
    let pointer = contacts
        .borrow()
        .pointer(SURFACE, None, MOUSE_CONTACT)
        .unwrap_or_else(|| {
            PointerInfo::new(
                PointerId::try_from(1_u64).expect("BUG: the mouse's reserved identity is nonzero"),
                PointerKind::Mouse,
            )
            .with_role(PointerRole::Primary)
        });
    Some(PlatformInput::Pointer(PointerEvent::Scroll(
        ScrollEvent::new(pointer, time, position, delta).with_modifiers(modifiers),
    )))
}

/// A DOM key event as the window delivers it; `composing` is whether the
/// window's own text input composes now.
pub(super) fn convert_keyboard_event(
    ke: &web_sys::KeyboardEvent,
    state: KeyState,
    composing: bool,
) -> PlatformInput {
    let modifiers = observed_modifiers(
        [ke.shift_key(), ke.ctrl_key(), ke.alt_key(), ke.meta_key()],
        |name| ke.get_modifier_state(name),
    );

    let source: &Utf16KeyboardEvent = ke.unchecked_ref();
    let key = map_key_value(source.key_utf16());

    let location = match ke.location() {
        1 => Location::Left,
        2 => Location::Right,
        3 => Location::Numpad,
        // 0 is DOM_KEY_LOCATION_STANDARD; an unrecognised value is treated
        // the same way rather than dropped.
        _ => Location::Standard,
    };

    PlatformInput::Keyboard(
        KeyEvent::new(
            state,
            key,
            Code::from_w3c(&ke.code()).unwrap_or(Code::Unidentified),
            EventTime::from_nanos((ke.time_stamp() * 1_000_000.0) as u64),
        )
        .with_location(location)
        .with_modifiers(modifiers)
        .with_repeat(if ke.repeat() {
            KeyRepeat::AutoRepeat
        } else {
            KeyRepeat::First
        })
        .with_composition(key_composition(ke.is_composing(), ke.key_code(), composing)),
    )
}

// ==================== Helpers ====================

fn extract_modifiers_from_mouse(e: &web_sys::MouseEvent) -> Modifiers {
    observed_modifiers(
        [e.shift_key(), e.ctrl_key(), e.alt_key(), e.meta_key()],
        |name| e.get_modifier_state(name),
    )
}

/// Preserve positive source observations; interpretation belongs to consumers.
/// DOM getters can reenter, so this collects local values without borrowing state.
fn observed_modifiers(convenience: [bool; 4], state: impl Fn(&str) -> bool) -> Modifiers {
    let mut modifiers = Modifiers::NONE;
    for (active, flag) in convenience.into_iter().zip([
        Modifiers::SHIFT,
        Modifiers::CONTROL,
        Modifiers::ALT,
        Modifiers::META,
    ]) {
        if active {
            modifiers |= flag;
        }
    }
    for (name, flag) in [
        ("Shift", Modifiers::SHIFT),
        ("Control", Modifiers::CONTROL),
        ("Alt", Modifiers::ALT),
        ("Meta", Modifiers::META),
        ("AltGraph", Modifiers::ALT_GRAPH),
        ("CapsLock", Modifiers::CAPS_LOCK),
        ("NumLock", Modifiers::NUM_LOCK),
        ("ScrollLock", Modifiers::SCROLL_LOCK),
        ("Fn", Modifiers::FN),
        ("FnLock", Modifiers::FN_LOCK),
        ("Symbol", Modifiers::SYMBOL),
        ("SymbolLock", Modifiers::SYMBOL_LOCK),
        ("Hyper", Modifiers::META),
        ("Super", Modifiers::META),
    ] {
        if state(name) {
            modifiers |= flag;
        }
    }
    modifiers
}

/// Named values precede source-specific Unicode admission; refusal preserves the event.
fn map_key_value(key: js_sys::JsString) -> Key {
    if !key.is_valid_utf16() {
        return Key::Named(NamedKey::Unidentified);
    }
    let key = String::from(key);
    if matches!(key.as_str(), "Hyper" | "Super") {
        Key::Named(NamedKey::Meta)
    } else if let Some(named) = NamedKey::from_w3c(&key) {
        Key::Named(named)
    } else if is_web_key_string(&key) {
        Key::character(key)
    } else {
        Key::Named(NamedKey::Unidentified)
    }
}

/// W3C key strings are nonempty NFC text: an optional non-Cc base, then
/// combining characters (Mc or a nonzero canonical combining class).
fn is_web_key_string(key: &str) -> bool {
    let mut characters = key.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let categories = CodePointMapData::<GeneralCategory>::new();
    if categories.get(first) == GeneralCategory::Control {
        return false;
    }
    let combining_classes = CanonicalCombiningClassMapBorrowed::new();
    characters.all(|character| {
        let category = categories.get(character);
        category != GeneralCategory::Control
            && (category == GeneralCategory::SpacingMark
                || combining_classes.get_u8(character) != 0)
    }) && ComposingNormalizerBorrowed::new_nfc().is_normalized(key)
}
