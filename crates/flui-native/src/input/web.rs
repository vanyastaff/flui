//! The web host's input: the DOM's pointer, keyboard, wheel and gesture
//! events on the canvas, converted into FLUI's owned `PlatformInput`.
//!
//! - [`keys`]: which key belongs to the input method; built on every target
//!   and public, hidden from the documentation, for the backend conformance
//!   table in `tests/`;
//! - `wheel`: the step a `WheelEvent` makes, by its `deltaMode`; pure, built
//!   for the web and for tests on every host;
//! - `pinch`: the browser's trackpad pinch, decoded from what each engine
//!   dispatches for it; pure, built for the web and for tests on every host;
//! - `events`: the listeners on the canvas, which the window registers through
//!   the loop's input hook, and the conversion of the DOM's events, which
//!   read the DOM.

#[doc(hidden)]
pub mod keys {
    //! The decidable half of the web backend's key translation: which key
    //! belongs to the input method. Cfg-free plain functions over integers, so
    //! Linux CI pins them; `events` reads the DOM.

    #![forbid(unsafe_code)]

    use flui_platform::keyboard::ImeComposition;

    /// `KeyboardEvent.keyCode` of a key an input method processes: every
    /// browser reports 229 for it, including WebKit for the key that commits a
    /// composition, which it dispatches after `compositionend` with
    /// `isComposing` already false.
    pub const IME_PROCESS_KEY_CODE: u32 = 229;

    /// Whether a DOM key event belongs to the input method: the event says it
    /// composes, carries the input method's key code, or arrives while the
    /// window's own text input composes. Such a key reaches the application
    /// marked [`ImeComposition::Active`], which no shortcut, focus traversal or
    /// drag cancellation takes, so the input method sees it first: Escape
    /// cancels the composition, not a dialog.
    #[must_use]
    pub const fn key_composition(
        is_composing: bool,
        key_code: u32,
        host_composing: bool,
    ) -> ImeComposition {
        if is_composing || key_code == IME_PROCESS_KEY_CODE || host_composing {
            ImeComposition::Active
        } else {
            ImeComposition::Inactive
        }
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod wheel {
    //! The DOM's wheel step. A `WheelEvent`'s deltas already point FLUI's way
    //! (positive scrolls down and right), and `deltaMode` names their unit:
    //! `DOM_DELTA_PIXEL` deltas are CSS, that is logical, pixels.
    //! <https://w3c.github.io/uievents/#events-wheelevents>

    #![forbid(unsafe_code)]

    use flui_platform::pointer::{InputValueError, ScrollDelta, ScrollUnit};

    use crate::input::scroll::{WheelAmount, wheel_step};

    /// The step a `WheelEvent` with `delta_mode` and these deltas makes. A
    /// mode the DOM may add later follows `DOM_DELTA_PIXEL`.
    ///
    /// # Errors
    /// [`InputValueError`] when a delta is not finite; the window drops such
    /// an event.
    pub(crate) fn step(
        delta_mode: u32,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<ScrollDelta, InputValueError> {
        let unit = match delta_mode {
            1 => ScrollUnit::Lines,
            2 => ScrollUnit::Pages,
            _ => ScrollUnit::Pixels,
        };
        wheel_step(
            unit,
            WheelAmount::offset(delta_x),
            WheelAmount::offset(delta_y),
        )
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod pinch {
    //! Browser trackpad pinch, decoded from what each engine dispatches for it
    //! (ADR-0213 §2, ADR-0217 §4): Chromium and Firefox send a trackpad pinch as
    //! `wheel` events with `ctrlKey` set, WebKit as `gesturestart`,
    //! `gesturechange` and `gestureend`. Cfg-free plain state, so Linux CI pins
    //! it; `events` feeds it DOM values and delivers what it returns.
    //!
    //! # Ctrl+wheel
    //!
    //! The DOM does not say whether a Control-held wheel event came from a
    //! pinch or from a mouse wheel with the key held, so the decision is
    //! [`Availability::Emulated`](flui_platform::Availability)
    //! and is taken once per sequence, from its first event:
    //!
    //! - a pinch is a pixel-mode vertical delta that is a whole number neither
    //!   of CSS pixels nor of device pixels (Chromium reports `-100 · ln(scale)`
    //!   CSS pixels, never a whole number in practice); a mouse detent is a
    //!   whole number of one of them, or lines: at a page zoom its CSS pixels
    //!   are fractional and its device pixels whole;
    //! - a sequence is the run of Control-held wheel events no further apart than
    //!   [`PINCH_GAP`] on the event timeline, and every event of it is treated as
    //!   its first was, so one whole-number step of a pinch stays in it and one
    //!   fractional detent of a Control-held mouse wheel stays a scroll;
    //! - a pinch ends at its deadline, the last event's time plus [`PINCH_GAP`]:
    //!   the End is stamped with that time on the event timeline, whether the
    //!   browser's timer or the next input reaches it first. A wheel event
    //!   without Control ends it at that event's time;
    //! - the application may refuse the sequence: when it does not prevent the
    //!   default of the sequence's first event, the pinch is cancelled at once and
    //!   the rest of the sequence reaches neither the application nor
    //!   `preventDefault`, so the browser zooms the page as it would have without
    //!   FLUI.
    //!
    //! The factor a step applies is `exp(-deltaY / 100)`, the inverse of
    //! Chromium's encoding; the focal point is the event's position.
    //!
    //! # WebKit gesture events
    //!
    //! `GestureEvent.scale` and `.rotation` are cumulative from `gesturestart`
    //! (rotation in degrees, clockwise positive); [`GestureTracker`] turns them
    //! into the per-step factor and radians the shared accumulator takes, and the
    //! same refusal applies to its first event.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use flui_platform::EventTime;

    use crate::input::assembly::{GestureComponent, GestureDelta, GesturePhase, GestureReading};

    /// The longest pause between two events of one Control-held wheel sequence.
    /// Chromium dispatches a pinch's steps at the display rate; a longer pause
    /// is a new gesture.
    pub const PINCH_GAP: Duration = Duration::from_millis(120);

    /// `WheelEvent.DOM_DELTA_PIXEL`.
    const DOM_DELTA_PIXEL: u32 = 0;

    /// The values of one DOM `wheel` event the decision reads.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct WheelStep {
        /// `ctrlKey`.
        pub ctrl: bool,
        /// `deltaMode`.
        pub mode: u32,
        /// `deltaX`.
        pub dx: f64,
        /// `deltaY`.
        pub dy: f64,
        /// `devicePixelRatio`: device pixels per CSS pixel, page zoom included.
        pub scale: f64,
        /// `timeStamp` on the event timeline.
        pub time: EventTime,
    }

    /// What a wheel event is, once [`CtrlWheel::route`] has read it.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum WheelRoute {
        /// A scroll: delivered as `PointerEvent::Scroll`.
        Scroll,
        /// A step of a pinch: delivered as the gesture reading.
        Pinch(GestureReading),
        /// A step of a refused pinch: delivered to nobody, and its default (the
        /// browser's zoom) kept.
        Refused,
    }

    /// What a Control-held wheel sequence was classified as.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Sequence {
        Pinch,
        Scroll,
        Refused,
    }

    /// The open Control-held sequence and its deadline.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Open {
        kind: Sequence,
        deadline: EventTime,
    }

    /// The ctrl+wheel pinch decoder of one surface (see the module doc).
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct CtrlWheel {
        open: Option<Open>,
    }

    /// The End of the pinch open until `deadline`, stamped with it.
    fn end_at(deadline: EventTime) -> (GestureReading, EventTime) {
        (
            GestureReading {
                component: GestureComponent::Scale,
                phase: GesturePhase::Ended,
                delta: GestureDelta::None,
            },
            deadline,
        )
    }

    impl CtrlWheel {
        /// When the open pinch ends unless another step arrives, for the timer
        /// that delivers its End; `None` when no pinch is open.
        #[must_use]
        pub fn deadline(&self) -> Option<EventTime> {
            self.open
                .filter(|open| open.kind == Sequence::Pinch)
                .map(|open| open.deadline)
        }

        /// End whatever sequence is due at `now`, returning a pinch's End stamped
        /// with its deadline. Call it before delivering any input, and from the
        /// deadline's timer.
        pub fn expire(&mut self, now: EventTime) -> Option<(GestureReading, EventTime)> {
            let open = self.open?;
            if now <= open.deadline {
                return None;
            }
            self.open = None;
            (open.kind == Sequence::Pinch).then(|| end_at(open.deadline))
        }

        /// End the open pinch now, at `time` (no later than its deadline): a
        /// wheel event without Control, or the surface going away.
        pub fn finish(&mut self, time: EventTime) -> Option<(GestureReading, EventTime)> {
            let open = self.open.take()?;
            (open.kind == Sequence::Pinch).then(|| end_at(time.min(open.deadline)))
        }

        /// Refuse the open pinch: the application did not prevent the default of
        /// its first event. Returns the cancellation of the pinch the first event
        /// started; the rest of the sequence is [`WheelRoute::Refused`].
        pub fn refuse(&mut self) -> Option<GestureReading> {
            let open = self.open.as_mut()?;
            if open.kind != Sequence::Pinch {
                return None;
            }
            open.kind = Sequence::Refused;
            Some(GestureReading {
                component: GestureComponent::Scale,
                phase: GesturePhase::Cancelled,
                delta: GestureDelta::None,
            })
        }

        /// Route `step`. A Control-held event within the open sequence's gap
        /// continues it; otherwise it starts one, classified by this event.
        /// Call [`Self::expire`] (or [`Self::finish`] for an event without
        /// Control) first, so a due End is delivered before it.
        pub fn route(&mut self, step: WheelStep) -> WheelRoute {
            if !step.ctrl {
                self.open = None;
                return WheelRoute::Scroll;
            }
            // The end of the timeline closes a sequence no later step can extend.
            let deadline = step
                .time
                .checked_add(PINCH_GAP)
                .unwrap_or(EventTime::from_nanos(u64::MAX));
            let continuing = self.open.filter(|open| step.time <= open.deadline);
            let kind = continuing.map_or_else(|| classify(step), |open| open.kind);
            self.open = Some(Open { kind, deadline });
            match kind {
                Sequence::Scroll => WheelRoute::Scroll,
                Sequence::Refused => WheelRoute::Refused,
                Sequence::Pinch => {
                    let factor = (-step.dy / 100.0).exp();
                    let delta = if factor.is_finite() && factor > 0.0 {
                        GestureDelta::Scale(factor)
                    } else {
                        GestureDelta::None
                    };
                    WheelRoute::Pinch(GestureReading {
                        component: GestureComponent::Scale,
                        phase: if continuing.is_some() {
                            GesturePhase::Changed
                        } else {
                            GesturePhase::Began
                        },
                        delta,
                    })
                }
            }
        }
    }

    /// How close to a whole number a delta is a whole number: CSS pixels reach
    /// the page through single precision, and a detent at a page zoom is a
    /// whole number of device pixels only to that precision.
    const WHOLE_TOLERANCE: f64 = 1e-3;

    /// Whether `value` is a whole number, to [`WHOLE_TOLERANCE`].
    fn is_whole(value: f64) -> bool {
        (value - value.round()).abs() <= WHOLE_TOLERANCE
    }

    /// Whether the first event of a Control-held sequence is a pinch: a
    /// pixel-mode vertical delta that is a whole number neither of CSS pixels
    /// nor of device pixels. A mouse detent is whole in one of them: at a page
    /// zoom Chromium divides it into fractional CSS pixels (100 at 110% is
    /// 90.909…, 100 device pixels), and an accelerated wheel lands within single
    /// precision of a whole number (4.000244).
    fn classify(step: WheelStep) -> Sequence {
        let device = step.dy * step.scale;
        if step.mode == DOM_DELTA_PIXEL
            && step.dx == 0.0
            && step.dy.is_finite()
            && !is_whole(step.dy)
            && !(device.is_finite() && step.scale > 0.0 && is_whole(device))
        {
            Sequence::Pinch
        } else {
            Sequence::Scroll
        }
    }

    /// A WebKit gesture event's phase.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GestureEventKind {
        /// `gesturestart`.
        Start,
        /// `gesturechange`.
        Change,
        /// `gestureend`.
        End,
    }

    /// The cumulative values of a WebKit gesture since its start, turned into
    /// per-step readings (see the module doc).
    #[derive(Debug, Clone, Copy, PartialEq, Default)]
    pub struct GestureTracker {
        /// The last cumulative scale and rotation (degrees) delivered, while a
        /// gesture is open; `refused` once the application refused it.
        open: Option<(f64, f64)>,
        refused: bool,
    }

    impl GestureTracker {
        /// The readings one gesture event makes: scale, then rotation. A value
        /// that is not finite (or a scale that is not positive) changes nothing.
        /// Empty for an event of a refused gesture, or for a change or end with
        /// no gesture open.
        pub fn step(
            &mut self,
            kind: GestureEventKind,
            scale: f64,
            rotation_degrees: f64,
        ) -> [Option<GestureReading>; 2] {
            let phase = match kind {
                GestureEventKind::Start => {
                    self.open = Some((1.0, 0.0));
                    self.refused = false;
                    GesturePhase::Began
                }
                GestureEventKind::Change => GesturePhase::Changed,
                GestureEventKind::End => GesturePhase::Ended,
            };
            let Some((last_scale, last_rotation)) = self.open else {
                return [None, None];
            };
            if kind == GestureEventKind::End {
                self.open = None;
            }
            if self.refused {
                return [None, None];
            }
            let scale_ok = scale.is_finite() && scale > 0.0;
            let rotation_ok = rotation_degrees.is_finite();
            let (next_scale, next_rotation) = (
                if scale_ok { scale } else { last_scale },
                if rotation_ok {
                    rotation_degrees
                } else {
                    last_rotation
                },
            );
            if kind != GestureEventKind::End {
                self.open = Some((next_scale, next_rotation));
            }
            let factor = next_scale / last_scale;
            let scale_delta = if scale_ok && factor.is_finite() && factor > 0.0 {
                GestureDelta::Scale(factor)
            } else {
                GestureDelta::None
            };
            let turn = (next_rotation - last_rotation).to_radians();
            let rotation_delta = if rotation_ok && turn.is_finite() {
                GestureDelta::Rotation(turn)
            } else {
                GestureDelta::None
            };
            [
                Some(GestureReading {
                    component: GestureComponent::Scale,
                    phase,
                    delta: scale_delta,
                }),
                Some(GestureReading {
                    component: GestureComponent::Rotation,
                    phase,
                    delta: rotation_delta,
                }),
            ]
        }

        /// Refuse the open gesture: the cancellation of what its first event
        /// started. The rest of it is delivered to nobody.
        pub fn refuse(&mut self) -> Option<GestureReading> {
            self.open?;
            self.refused = true;
            Some(GestureReading {
                component: GestureComponent::Scale,
                phase: GesturePhase::Cancelled,
                delta: GestureDelta::None,
            })
        }

        /// Whether the open gesture was refused.
        #[must_use]
        pub const fn is_refused(&self) -> bool {
            self.refused
        }
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod events {
    //! The canvas's input listeners and the conversion of the DOM's events into
    //! FLUI's owned `PlatformInput`. The window registers its canvas's input
    //! through [`WebInput`], the loop's input hook.

    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        rc::Rc,
        sync::Arc,
    };

    use flui_foundation::geometry::Point;
    use flui_platform::{
        EventTime,
        keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey},
        pointer::{
            ButtonChange, PenTool, PointerButton, PointerButtons, PointerEvent, PointerId,
            PointerInfo, PointerKind, PointerPosition, PointerRole, ScrollEvent,
        },
    };

    use super::keys::key_composition;
    use super::pinch::{CtrlWheel, GestureEventKind, GestureTracker, WheelRoute, WheelStep};
    use crate::input::assembly::{
        Assembled, ContactRegistry, DecodeError, GesturePhase, GestureReading, MoveBatch,
        NativeCancel, NativeClock, NativeContact, NativeReading, NativeSurface, PenAngles,
        RawSensors, ReadingAction, ReadingTime, decode,
    };
    use crate::owner::web::event::{answer, client_position, pointer_position};
    use flui_platform::DispatchEventResult;
    use icu_normalizer::{
        ComposingNormalizerBorrowed, properties::CanonicalCombiningClassMapBorrowed,
    };
    use icu_properties::{CodePointMapData, props::GeneralCategory};

    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    use crate::owner::web::{Accepting, CanvasKeys, CanvasRegistration, HostInput, PointerCustody};
    use crate::owner::window_delivery::WindowDelivery;
    use flui_platform::PlatformInput;

    /// The page's input, as the web loop holds it ([`HostInput`]).
    pub(crate) struct WebInput;

    impl HostInput for WebInput {
        /// Trackpad pinch is native where the browser has WebKit gesture
        /// events, else emulated from Control-held wheel events.
        fn availability(&self) -> flui_platform::input_capabilities::InputCapabilities {
            crate::input::availability::web(has_gesture_events())
        }

        /// The pointer, keyboard, wheel, gesture and context-menu listeners of
        /// the canvas, and the end of every pointer sequence when the page
        /// loses the OS focus or is hidden, which the browser may never
        /// release. The keys route to the window from the canvas and from the
        /// editor the text input keeps beside it.
        fn register(
            &self,
            canvas: &web_sys::HtmlCanvasElement,
            delivery: &Arc<WindowDelivery>,
            latency: flui_platform::InputLatency,
            registration: &mut dyn CanvasRegistration,
        ) -> Result<Rc<dyn CanvasKeys>, crate::OpenWindowError> {
            let accepting = registration.accepting();
            let contacts = Rc::new(RefCell::new(
                ContactRegistry::new(NativeClock::epoch_only())
                    .with_reserved_contact(MOUSE_CONTACT),
            ));
            let pinch = Rc::new(Pinch {
                delivery: Arc::clone(delivery),
                accepting: accepting.clone(),
                contacts: Rc::clone(&contacts),
                wheel: RefCell::default(),
                gesture: RefCell::default(),
                focal: Cell::new(None),
            });
            let keys = Rc::new(KeyRoute {
                delivery: Arc::clone(delivery),
                pinch: Rc::clone(&pinch),
                accepting,
                composing: Cell::new(false),
                unplaced: std::cell::RefCell::default(),
            });

            // The pointers the canvas holds captured, by DOM pointer id with
            // the buttons they hold: each is released as the registration
            // retires, which the browser would not do for a removed canvas.
            let captured: Captured = Rc::default();
            {
                let captured = Rc::clone(&captured);
                let custody = registration.custody();
                registration.on_retire(Box::new(move || {
                    release_captures(&captured, &*custody);
                }));
            }
            let menu = Rc::new(MenuAnswer::default());
            let loss = register_pointer_events(
                canvas,
                &contacts,
                &captured,
                &pinch,
                &menu,
                latency,
                registration,
            )?;
            register_keyboard_events(canvas, &keys, registration)?;
            register_loss_events(canvas, &loss, registration)?;
            register_hover_events(canvas, delivery, registration)?;
            register_wheel_events(canvas, delivery, &pinch, registration)?;
            register_gesture_events(canvas, &pinch, registration)?;
            register_context_menu(canvas, &menu, registration)?;
            Ok(keys)
        }
    }

    /// The pointers a canvas holds captured, by DOM pointer id with the
    /// buttons they hold.
    type Captured = Rc<RefCell<HashMap<i32, u16>>>;

    /// End the captures the canvas still holds as its registration retires:
    /// custody first, since a release can synchronously dispatch the capture
    /// loss, then each release in a boundary of its own.
    fn release_captures(captured: &RefCell<HashMap<i32, u16>>, custody: &dyn PointerCustody) {
        let ids: Vec<i32> = captured.borrow_mut().drain().map(|(id, _)| id).collect();
        for &id in &ids {
            custody.forget(id);
        }
        for id in ids {
            crate::owner::panic_boundary::contain_owner_callback(|| custody.release(id));
        }
    }

    /// How a DOM key event reaches the window: from the canvas, or from the
    /// text input's own editor.
    struct KeyRoute {
        delivery: Arc<WindowDelivery>,
        pinch: Rc<Pinch>,
        accepting: Accepting,
        /// Whether the window's text input composes now.
        composing: Cell<bool>,
        /// Keys held down the browser names no code for, by their `keyCode`,
        /// with the key each was pressed as.
        unplaced: std::cell::RefCell<crate::input::keys::UnplacedKeys<u32>>,
    }

    impl CanvasKeys for KeyRoute {
        /// Deliver `event` as a key `state`, answering it with the result.
        fn deliver(&self, event: &web_sys::Event, state: KeyState) {
            if !self.accepting.get() {
                return;
            }
            let ke: &web_sys::KeyboardEvent = event.unchecked_ref();
            // Read before the held keys are borrowed: a DOM getter may
            // dispatch. A zero key code (a synthetic event's) names no key.
            let key_code = Some(ke.key_code()).filter(|code| *code != 0);
            let input = match convert_keyboard_event(ke, state, self.composing.get()) {
                PlatformInput::Keyboard(key) => {
                    PlatformInput::Keyboard(self.unplaced.borrow_mut().settle(key_code, key))
                }
                other => other,
            };
            // DOM getters can close this registration before callback admission.
            if !self.accepting.get() {
                return;
            }
            self.pinch.flush(event_time(event.time_stamp()));
            if !self.accepting.get() {
                return;
            }
            answer(event, self.delivery.dispatch_input(input));
        }

        fn composing(&self) -> bool {
            self.composing.get()
        }

        fn set_composing(&self, composing: bool) {
            self.composing.set(composing);
        }
    }

    /// The page losing the OS focus (`blur` does not bubble, so the browser
    /// window's own) or being hidden ends every sequence, before the window's
    /// own listeners report the change: its callback may close the window, after
    /// which nothing would cancel them.
    fn register_loss_events(
        canvas: &web_sys::HtmlCanvasElement,
        loss: &Rc<PointerLoss>,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        let browser = web_sys::window().expect("BUG: construction verified browser");
        {
            let loss = Rc::clone(loss);
            registration.listen(
                browser.as_ref(),
                "blur",
                Rc::new(move |event: web_sys::Event| {
                    let page_lost = event
                        .target()
                        .is_some_and(|target| target.dyn_ref::<web_sys::Window>().is_some());
                    if page_lost {
                        loss.cancel(event_time(event.time_stamp()), NativeCancel::FocusLost);
                    }
                }),
            )?;
        }
        let document = canvas
            .owner_document()
            .expect("BUG: window construction verified the canvas document");
        let loss = Rc::clone(loss);
        let hidden = document.clone();
        registration.listen(
            document.as_ref(),
            "visibilitychange",
            Rc::new(move |event: web_sys::Event| {
                if hidden.hidden() {
                    loss.cancel(event_time(event.time_stamp()), NativeCancel::FocusLost);
                }
            }),
        )
    }

    /// The mouse coming over the canvas and leaving it, as the window's hover.
    fn register_hover_events(
        canvas: &web_sys::HtmlCanvasElement,
        delivery: &Arc<WindowDelivery>,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        for (kind, hovered) in [("pointerenter", true), ("pointerleave", false)] {
            let delivery = Arc::clone(delivery);
            registration.listen(
                canvas.as_ref(),
                kind,
                Rc::new(move |_: web_sys::Event| {
                    delivery.report_hover(hovered);
                }),
            )?;
        }
        Ok(())
    }

    // ==================== Pointer Events ====================

    /// The DOM event that carries a pointer's movement: `pointermove` is
    /// coalesced to one per frame, `pointerrawupdate` (Chromium) fires for every
    /// sample, so a [`Low`](flui_platform::InputLatency::Low) window listens to it where
    /// the browser has it, and to `pointermove` otherwise. Never both: a move
    /// would arrive twice.
    fn move_event(input_latency: flui_platform::InputLatency) -> &'static str {
        let raw = web_sys::window().is_some_and(|window| {
            js_sys::Reflect::has(&window, &JsValue::from_str("onpointerrawupdate")).unwrap_or(false)
        });
        if input_latency == flui_platform::InputLatency::Low && raw {
            "pointerrawupdate"
        } else {
            "pointermove"
        }
    }

    fn register_pointer_events(
        canvas: &web_sys::HtmlCanvasElement,
        contacts: &Rc<RefCell<ContactRegistry>>,
        captured: &Captured,
        pinch: &Rc<Pinch>,
        menu: &Rc<MenuAnswer>,
        input_latency: flui_platform::InputLatency,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<Rc<PointerLoss>, crate::OpenWindowError> {
        let active = Rc::clone(captured);
        // DOM getters and application callbacks can synchronously dispatch another
        // pointer event. Listener captures are immutable; mutable state is scoped
        // inside the active-pointer cell instead of the JavaScript closure.
        // pointerdown
        {
            let active = Rc::clone(&active);
            let contacts = Rc::clone(contacts);
            let pinch = Rc::clone(pinch);
            let capture = canvas.clone();
            let accepting = registration.accepting();
            let custody = registration.custody();
            let menu = Rc::clone(menu);
            registration.listen(
                canvas.as_ref(),
                "pointerdown",
                Rc::new(move |e: web_sys::Event| {
                    let pe: web_sys::PointerEvent = e.unchecked_into();
                    let id = pe.pointer_id();
                    if !accepting.get() {
                        return;
                    }
                    let buttons = pe.buttons();
                    let contextual = opens_menu(pe.button(), pe.ctrl_key());
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
                    menu.record(contextual, pressed);
                }),
            )?;
        }

        // pointermove
        {
            let active = Rc::clone(&active);
            let contacts = Rc::clone(contacts);
            let pinch = Rc::clone(pinch);
            let accepting = registration.accepting();
            let menu = Rc::clone(menu);
            registration.listen(
                canvas.as_ref(),
                move_event(input_latency),
                Rc::new(move |e: web_sys::Event| {
                    let pe: web_sys::PointerEvent = e.unchecked_into();
                    let id = pe.pointer_id();
                    if !accepting.get() {
                        return;
                    }
                    let buttons = pe.buttons();
                    let contextual = opens_menu(pe.button(), pe.ctrl_key());
                    if !accepting.get() {
                        return;
                    }
                    if let Some(held) = active.borrow_mut().get_mut(&id) {
                        *held = buttons;
                    }
                    // A chorded press opens the menu as a first one does.
                    let pressed = dispatch(&pinch, &pe, convert_pointer_move(&pe, &contacts));
                    menu.record(contextual, pressed);
                }),
            )?;
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
            let accepting = registration.accepting();
            registration.listen(
                canvas.as_ref(),
                kind,
                Rc::new(move |e: web_sys::Event| {
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
                }),
            )?;
        }

        // pointerup
        {
            let active = Rc::clone(&active);
            let contacts = Rc::clone(contacts);
            let pinch = Rc::clone(pinch);
            let accepting = registration.accepting();
            let custody = registration.custody();
            registration.listen(
                canvas.as_ref(),
                "pointerup",
                Rc::new(move |e: web_sys::Event| {
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
                }),
            )?;
        }

        // A browser gesture takeover is a terminal event even when no release
        // will follow. Deliver it through the same public input callback.
        {
            let active = Rc::clone(&active);
            let contacts = Rc::clone(contacts);
            let pinch = Rc::clone(pinch);
            let accepting = registration.accepting();
            let custody = registration.custody();
            registration.listen(
                canvas.as_ref(),
                "pointercancel",
                Rc::new(move |e: web_sys::Event| {
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
                }),
            )?;
        }

        {
            let contacts = Rc::clone(contacts);
            let pinch = Rc::clone(pinch);
            let accepting = registration.accepting();
            let custody = registration.custody();
            registration.listen(
                canvas.as_ref(),
                "lostpointercapture",
                Rc::new(move |e: web_sys::Event| {
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
                }),
            )?;
        }
        // The page's focus and visibility listeners end the sequences through
        // it, before they report the change: a callback for the change can close
        // the window, after which nothing would cancel them.
        Ok(Rc::new(PointerLoss {
            contacts: Rc::clone(contacts),
            pinch: Rc::clone(pinch),
            active: Rc::clone(captured),
            custody: registration.custody(),
            menu: Rc::clone(menu),
        }))
    }

    // ==================== Keyboard Events ====================

    /// Deliver assembled pointer input while the registration still accepts it:
    /// a callback can close the window between two events of one reading. A
    /// pinch whose deadline `event` passed ends first. Returns how the
    /// application answered a press among them.
    fn dispatch(
        pinch: &Pinch,
        event: &web_sys::Event,
        inputs: Assembled,
    ) -> Option<DispatchEventResult> {
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
            let result = pinch.delivery.dispatch_input(input);
            if press {
                pressed = Some(result);
            }
        }
        pressed
    }

    /// `MouseEvent.button` of the secondary button.
    const SECONDARY_BUTTON: i16 = 2;

    /// Whether a press of `button`, with Control held when `control`, opens the
    /// context menu: the secondary button, or on macOS a Control-held primary
    /// click (its `contextmenu` reports the primary button with Control). A
    /// Control-held primary click elsewhere opens none, and no menu takes its
    /// answer.
    const fn opens_menu(button: i16, control: bool) -> bool {
        button == SECONDARY_BUTTON || (button == 0 && control)
    }

    /// How the browser's context menu is answered: as the application answered
    /// the press that opened it (`contextmenu` follows the press, or on Windows
    /// its release), its propagation and its default alike. A menu no press
    /// opened (the keyboard's menu key) stays the application's, its default
    /// prevented.
    #[derive(Default)]
    struct MenuAnswer {
        /// The answer to the last press that opens a menu, until a menu takes it.
        answer: Cell<Option<DispatchEventResult>>,
    }

    impl MenuAnswer {
        fn record(&self, contextual: bool, answer: Option<DispatchEventResult>) {
            if contextual && answer.is_some() {
                self.answer.set(answer);
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
        active: Captured,
        custody: Rc<dyn PointerCustody>,
        /// A menu answer the ended press left is not a later menu's.
        menu: Rc<MenuAnswer>,
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
            self.menu.answer.set(None);
            self.pinch.deliver(ended.into_iter().collect(), None);
            for id in ids {
                self.custody.release(id);
            }
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
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
            let keys = Rc::clone(keys);
            let focus = registration.focus();
            let target = canvas.clone();
            registration.listen(
                canvas.as_ref(),
                kind,
                Rc::new(move |event: web_sys::Event| {
                    if !focus.has_focus() || event.target().as_ref() != Some(target.as_ref()) {
                        return;
                    }
                    keys.deliver(&event, state);
                }),
            )?;
        }
        Ok(())
    }

    // ==================== Wheel Events ====================

    /// The contact label of a trackpad pinch: above every DOM `pointerId`, which
    /// is a 32-bit integer.
    const PINCH_CONTACT: NativeContact = NativeContact(1 << 32);

    /// The trackpad pinch of one registration, from Control-held wheel events or
    /// WebKit gesture events (`pinch`), and how it reaches the
    /// window's input callback.
    ///
    /// No borrow of its cells is held across a dispatch: an application callback
    /// can dispatch another DOM event at the canvas synchronously.
    struct Pinch {
        delivery: Arc<WindowDelivery>,
        accepting: Accepting,
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
                native = native.at_logical(point.x, point.y);
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
                let result = self.delivery.dispatch_input(input);
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
                let result = self.delivery.dispatch_input(head);
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
        delivery: &Arc<WindowDelivery>,
        pinch: &Rc<Pinch>,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        let timer = {
            let pinch = Rc::clone(pinch);
            registration.timer(Box::new(move || {
                // The timer runs no earlier than the deadline it was armed for.
                let deadline = pinch.wheel.borrow().deadline();
                if let Some(deadline) = deadline {
                    let now =
                        performance_now().max(deadline.checked_add(NANOSECOND).unwrap_or(deadline));
                    pinch.flush(now);
                }
            }))
        };
        let delivery = Arc::clone(delivery);
        let pinch = Rc::clone(pinch);
        let target = canvas.clone();
        registration.listen(
            canvas.as_ref(),
            "wheel",
            Rc::new(move |e: web_sys::Event| {
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
                            answer(&e, delivery.dispatch_input(input));
                        }
                    }
                    WheelRoute::Pinch(reading) => {
                        let Ok(focal) = PointerPosition::try_new(wheel_position(we, &target))
                        else {
                            return;
                        };
                        // Chromium sets Control on a pinch it encodes as a wheel; it
                        // is not a key the user holds.
                        let modifiers = observed_modifiers(
                            [we.shift_key(), false, we.alt_key(), we.meta_key()],
                            |name| name != "Control" && we.get_modifier_state(name),
                        );
                        let inputs =
                            pinch.assemble(reading, step.time, Some(focal.get()), modifiers);
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
                            timer.arm(
                                deadline.saturating_duration_since(performance_now()) + TIMER_SLACK,
                            );
                        }
                    }
                }
            }),
        )
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
    fn has_gesture_events() -> bool {
        web_sys::window().is_some_and(|window| {
            js_sys::Reflect::has(&window, &JsValue::from_str("GestureEvent")).unwrap_or(false)
        })
    }

    fn register_gesture_events(
        canvas: &web_sys::HtmlCanvasElement,
        pinch: &Rc<Pinch>,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        for (name, kind) in [
            ("gesturestart", GestureEventKind::Start),
            ("gesturechange", GestureEventKind::Change),
            ("gestureend", GestureEventKind::End),
        ] {
            let pinch = Rc::clone(pinch);
            let target = canvas.clone();
            registration.listen(
                canvas.as_ref(),
                name,
                Rc::new(move |e: web_sys::Event| {
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
                }),
            )?;
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
        registration: &mut dyn CanvasRegistration,
    ) -> Result<(), crate::OpenWindowError> {
        let menu = Rc::clone(menu);
        registration.listen(
            canvas.as_ref(),
            "contextmenu",
            Rc::new(move |e: web_sys::Event| {
                let contextual = e
                    .dyn_ref::<web_sys::MouseEvent>()
                    .is_some_and(|mouse| opens_menu(mouse.button(), mouse.ctrl_key()));
                let answered = if contextual { menu.answer.take() } else { None };
                match answered {
                    Some(result) => answer(&e, result),
                    None => e.prevent_default(),
                }
            }),
        )
    }

    // ==================== Event Conversion ====================

    // web-sys reads some of these getters through older signatures that lose
    // what the DOM reports (fractions, unpaired surrogates) before Rust receives
    // it. Bind the actual DOM getters with their own result types.
    #[wasm_bindgen]
    extern "C" {
        // DOMString can contain unpaired surrogates. A Rust String getter would
        // replace them before the source-specific admission check can refuse them.
        type Utf16KeyboardEvent;
        #[wasm_bindgen(method, getter, js_name = key)]
        fn key_utf16(this: &Utf16KeyboardEvent) -> js_sys::JsString;

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
        let client = client_position(event);
        Some(Point::new(client.x - origin.x, client.y - origin.y))
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
            sensors.tangential_pressure =
                tangential.is_finite().then(|| tangential.clamp(-1.0, 1.0));
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
        .at_logical(point.x, point.y)
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
    /// The step is [`super::wheel::step`]: the DOM already speaks FLUI's
    /// wheel direction, and `deltaMode` names the unit. It goes through the
    /// one wheel rule (`crate::input::scroll`) with the backends that turn
    /// or rescale their amounts, and its table test runs them alike.
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
        let delta = super::wheel::step(we.delta_mode(), we.delta_x(), we.delta_y()).ok()?;
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
                    PointerId::try_from(1_u64)
                        .expect("BUG: the mouse's reserved identity is nonzero"),
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

        PlatformInput::Keyboard(
            KeyEvent::new(
                state,
                key,
                Code::from_w3c(&ke.code()).unwrap_or(Code::Unidentified),
                EventTime::from_nanos((ke.time_stamp() * 1_000_000.0) as u64),
            )
            .with_location(crate::input::keys::location_of_number(ke.location()))
            .with_modifiers(modifiers)
            .with_repeat(if ke.repeat() {
                KeyRepeat::AutoRepeat
            } else {
                KeyRepeat::First
            })
            .with_composition(key_composition(
                ke.is_composing(),
                ke.key_code(),
                composing,
            )),
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

    /// The input position of the web table: the loop answers it.
    impl crate::host::Input<crate::owner::web::WebLoop> for crate::owner::web::WebLoop {
        fn keyboard_layout(_: &Self) -> String {
            "en-US".to_string()
        }

        fn on_keyboard_layout_change(_: &Self, callback: Box<dyn FnMut() + Send>) {
            // The layout is the fixed "en-US" above; nothing changes it.
            drop(callback);
        }
    }
}
