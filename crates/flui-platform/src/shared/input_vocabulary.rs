//! The bridge from the `ui-events` values the backends build today to FLUI's own input
//! vocabulary (`flui_platform_api::{pointer, keyboard}`, ADR-0143).
//!
//! It lives here, beside the backends, because it names `ui-events` and `keyboard-types`
//! types, which no Stable signature may (ADR-0089): `flui-platform` is internal, so the
//! bridge is reachable only from the composition root. The functions are stateless and total
//! over what the backends emit; what `ui-events` cannot express is read as follows.
//!
//! - **Sensors.** `ui-events` has no "no sensor" value, so a W3C default is read as one: a
//!   mouse has no pressure (the backends' 0.5-while-pressed stand-in is dropped); a tangential
//!   pressure of 0, the default orientation and a 1×1 contact are "not reported". A touch or
//!   pen pressure is kept as reported, including a backend's 0.5 stand-in for a touch screen
//!   without force sensing, which only the backend can tell apart. Pressures are saturated
//!   into their range (devices overshoot by rounding) and the altitude, an `f32` widened to
//!   `f64`, is clamped to `[0, π/2]`; a non-finite reading is dropped.
//! - **Eraser.** The W3C eraser button (`PenEraser`) becomes [`PenTool::Eraser`] on a pen, and
//!   the button it pressed becomes [`PointerButton::PRIMARY`].
//! - **Identity.** A pointer event without a pointer id is dropped (no backend emits one),
//!   except a scroll, which a wheel without an id (the DOM's) reports for the primary mouse.
//!   `ui-events`' persistent device id has no accessor, so the device id is always `None`.
//! - **Time.** `Cancel`, `Enter` and `Leave` carry no time in `ui-events`; the caller passes
//!   the time it observed the event.
//! - **Non-finite input.** A `Down`, `Move`, scroll or gesture whose position is not finite
//!   is dropped; an `Up` whose position is not finite becomes a cancellation
//!   ([`CancelReason::InvalidInput`]), so the sequence still ends and no tap lands at a
//!   guessed position. A scroll whose distance is not finite scrolls nowhere (a zero distance
//!   in the same unit), so the event and its unit still arrive.
//! - **Trackpad gestures.** A `Gesture` tick becomes a pan/zoom `Update` whose transform is the
//!   tick itself (`Pinch(d)` is a scale of `1 + d`), which is how the pipeline reads it today:
//!   `ui-events` has neither phases nor a cumulative transform. The backends' own gesture
//!   phases replace this when they produce the vocabulary directly.
//! - **Keys.** A named key or code is matched by its W3C spelling; one the generated tables do
//!   not list is `Unidentified`, and the legacy `Hyper` and `Super` keys and modifiers are
//!   `Meta`.

use core::num::NonZeroU8;

use flui_foundation::geometry::{Offset, Point, Size};
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
};
use flui_platform_api::pointer::{
    ButtonDirection, CancelReason, ContactSize, PanZoomEvent, PanZoomPhase, PanZoomTransform,
    PenOrientation, PenTool, PointerButton, PointerButtonEvent, PointerButtons, PointerCancel,
    PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerRole,
    PointerSample, PointerSignal, Pressure, ScrollDelta, ScrollEvent, ScrollUnit,
    TangentialPressure,
};
use ui_events::keyboard as upstream_keyboard;
use ui_events::pointer as upstream;

/// Every `ui-events` button, in bit order: index `i` is button number `i + 1`.
const UPSTREAM_BUTTONS: [upstream::PointerButton; 32] = {
    use upstream::PointerButton::{
        Auxiliary, B7, B8, B9, B10, B11, B12, B13, B14, B15, B16, B17, B18, B19, B20, B21, B22,
        B23, B24, B25, B26, B27, B28, B29, B30, B31, B32, PenEraser, Primary, Secondary, X1, X2,
    };
    [
        Primary, Secondary, Auxiliary, X1, X2, PenEraser, B7, B8, B9, B10, B11, B12, B13, B14, B15,
        B16, B17, B18, B19, B20, B21, B22, B23, B24, B25, B26, B27, B28, B29, B30, B31, B32,
    ]
};

/// FLUI's button for a `ui-events` one, or `None` for the pen eraser, which is a tool.
fn button(button: upstream::PointerButton) -> Option<PointerButton> {
    let index = UPSTREAM_BUTTONS
        .iter()
        .position(|candidate| *candidate == button)
        .expect("BUG: UPSTREAM_BUTTONS lists every ui-events button");
    let number = u8::try_from(index + 1).expect("BUG: there are 32 buttons");
    PointerButton::try_from(number).ok()
}

/// The held buttons, with the eraser bit read as the primary contact.
fn buttons(held: upstream::PointerButtons) -> PointerButtons {
    UPSTREAM_BUTTONS
        .into_iter()
        .filter(|upstream_button| held.contains(*upstream_button))
        .map(|upstream_button| button(upstream_button).unwrap_or(PointerButton::PRIMARY))
        .collect()
}

/// The modifiers, with the legacy `Hyper` and `Super` read as `Meta`.
#[must_use]
pub fn modifiers(upstream: upstream_keyboard::Modifiers) -> Modifiers {
    use upstream_keyboard::Modifiers as Up;
    #[expect(
        deprecated,
        reason = "the legacy modifiers still arrive and are read as Meta"
    )]
    let pairs = [
        (Up::ALT, Modifiers::ALT),
        (Up::ALT_GRAPH, Modifiers::ALT_GRAPH),
        (Up::CAPS_LOCK, Modifiers::CAPS_LOCK),
        (Up::CONTROL, Modifiers::CONTROL),
        (Up::FN, Modifiers::FN),
        (Up::FN_LOCK, Modifiers::FN_LOCK),
        (Up::META, Modifiers::META),
        (Up::NUM_LOCK, Modifiers::NUM_LOCK),
        (Up::SCROLL_LOCK, Modifiers::SCROLL_LOCK),
        (Up::SHIFT, Modifiers::SHIFT),
        (Up::SYMBOL, Modifiers::SYMBOL),
        (Up::SYMBOL_LOCK, Modifiers::SYMBOL_LOCK),
        (Up::HYPER, Modifiers::META),
        (Up::SUPER, Modifiers::META),
    ];
    pairs
        .into_iter()
        .filter(|(from, _)| upstream.contains(*from))
        .fold(Modifiers::NONE, |set, (_, to)| set | to)
}

fn kind(info: &upstream::PointerInfo, held: Option<upstream::PointerButtons>) -> PointerKind {
    match info.pointer_type {
        upstream::PointerType::Mouse => PointerKind::Mouse,
        upstream::PointerType::Touch => PointerKind::Touch,
        upstream::PointerType::Pen => {
            let eraser = held.is_some_and(|held| held.contains(upstream::PointerButton::PenEraser));
            PointerKind::Pen {
                tool: if eraser {
                    PenTool::Eraser
                } else {
                    PenTool::Tip
                },
            }
        }
        _ => PointerKind::Unknown,
    }
}

fn info(info: &upstream::PointerInfo, kind: PointerKind) -> Option<PointerInfo> {
    let id = PointerId::new(info.pointer_id?.get_inner());
    let role = if info.is_primary_pointer() {
        PointerRole::Primary
    } else {
        PointerRole::Additional
    };
    Some(PointerInfo::new(id, kind).with_role(role))
}

fn position(state: &upstream::PointerState) -> Option<PointerPosition> {
    // `ui-events` types the position as physical, but every backend stores logical pixels
    // there (`flui_platform_api::PlatformInput`'s contract), so it is read as is.
    PointerPosition::try_new(Point::new(state.position.x, state.position.y)).ok()
}

fn sample(state: &upstream::PointerState, kind: PointerKind) -> Option<PointerSample> {
    let mut sample = PointerSample::new(EventTime::from_nanos(state.time), position(state)?);
    if matches!(kind, PointerKind::Touch | PointerKind::Pen { .. })
        && let Ok(pressure) = Pressure::saturating(state.pressure)
    {
        sample = sample.with_pressure(pressure);
    }
    if state.tangential_pressure != 0.0
        && let Ok(pressure) = TangentialPressure::saturating(state.tangential_pressure)
    {
        sample = sample.with_tangential_pressure(pressure);
    }
    if state.orientation != upstream::PointerOrientation::default()
        && let Ok(orientation) = PenOrientation::try_new(
            f64::from(state.orientation.altitude),
            f64::from(state.orientation.azimuth),
        )
    {
        sample = sample.with_orientation(orientation);
    }
    let geometry = state.contact_geometry;
    if kind != PointerKind::Mouse
        && (geometry.width, geometry.height) != (1.0, 1.0)
        && let Ok(size) = ContactSize::try_new(Size::new(geometry.width, geometry.height))
    {
        sample = sample.with_contact_size(size);
    }
    Some(sample)
}

fn button_event<D: ButtonDirection>(
    event: &upstream::PointerButtonEvent,
) -> Option<PointerButtonEvent<D>> {
    // The tool is the one that changed as well as the ones still held: an
    // eraser release reports a held set without the eraser.
    let mut tool_buttons = event.state.buttons;
    if let Some(changed) = event.button {
        tool_buttons.insert(changed);
    }
    let kind = kind(&event.pointer, Some(tool_buttons));
    let pointer = info(&event.pointer, kind)?;
    let sample = sample(&event.state, kind)?;
    // A touch or pen contact without a button, and the eraser, press the primary button.
    let changed = event
        .button
        .and_then(button)
        .unwrap_or(PointerButton::PRIMARY);
    let converted =
        PointerButtonEvent::<D>::new(pointer, changed, buttons(event.state.buttons), sample)
            .with_modifiers(modifiers(event.state.modifiers));
    Some(match NonZeroU8::new(event.state.count) {
        Some(count) => converted.with_click_count(count),
        None => converted,
    })
}

/// FLUI's pointer event for a `ui-events` one, or `None` when it cannot be delivered (see the
/// module documentation). `observed` stamps the events `ui-events` carries no time for.
#[must_use]
pub fn pointer_event(event: &upstream::PointerEvent, observed: EventTime) -> Option<PointerEvent> {
    match event {
        upstream::PointerEvent::Down(event) => button_event(event).map(PointerEvent::Down),
        upstream::PointerEvent::Up(event) => {
            if let Some(up) = button_event(event) {
                return Some(PointerEvent::Up(up));
            }
            let pointer = info(
                &event.pointer,
                kind(&event.pointer, Some(event.state.buttons)),
            )?;
            Some(PointerEvent::Cancel(PointerCancel::new(
                pointer,
                EventTime::from_nanos(event.state.time),
                CancelReason::InvalidInput,
            )))
        }
        upstream::PointerEvent::Move(update) => {
            let kind = kind(&update.pointer, Some(update.current.buttons));
            let pointer = info(&update.pointer, kind)?;
            let current = sample(&update.current, kind)?;
            let samples = |states: &[upstream::PointerState]| {
                states
                    .iter()
                    .filter_map(|state| sample(state, kind))
                    .collect::<Vec<_>>()
            };
            Some(PointerEvent::Move(
                PointerMove::new(pointer, buttons(update.current.buttons), current)
                    .with_modifiers(modifiers(update.current.modifiers))
                    .with_coalesced(samples(&update.coalesced))
                    .with_predicted(samples(&update.predicted)),
            ))
        }
        upstream::PointerEvent::Cancel(pointer) => {
            let pointer = info(pointer, kind(pointer, None))?;
            Some(PointerEvent::Cancel(PointerCancel::new(
                pointer,
                observed,
                CancelReason::Platform,
            )))
        }
        upstream::PointerEvent::Enter(pointer) => info(pointer, kind(pointer, None))
            .map(|pointer| PointerEvent::Enter(PointerSignal::new(pointer, observed))),
        upstream::PointerEvent::Leave(pointer) => info(pointer, kind(pointer, None))
            .map(|pointer| PointerEvent::Leave(PointerSignal::new(pointer, observed))),
        upstream::PointerEvent::Scroll(event) => scroll(event).map(PointerEvent::Scroll),
        upstream::PointerEvent::Gesture(event) => gesture(event).map(PointerEvent::PanZoom),
    }
}

fn scroll(event: &upstream::PointerScrollEvent) -> Option<ScrollEvent> {
    let kind = kind(&event.pointer, Some(event.state.buttons));
    let pointer = match info(&event.pointer, kind) {
        Some(pointer) => pointer,
        // A DOM wheel event names no pointer; it comes from the primary mouse.
        None if event.pointer.pointer_id.is_none() => PointerInfo::new(
            PointerId::new(upstream::PointerId::PRIMARY.get_inner()),
            kind,
        )
        .with_role(PointerRole::Primary),
        None => return None,
    };
    let (unit, x, y) = match event.delta {
        ui_events::ScrollDelta::LineDelta(x, y) => (ScrollUnit::Lines, f64::from(x), f64::from(y)),
        ui_events::ScrollDelta::PixelDelta(pixels) => (ScrollUnit::Pixels, pixels.x, pixels.y),
        ui_events::ScrollDelta::PageDelta(x, y) => (ScrollUnit::Pages, f64::from(x), f64::from(y)),
    };
    let delta = ScrollDelta::try_new(unit, x, y).unwrap_or(ScrollDelta::zero(unit));
    Some(
        ScrollEvent::new(
            pointer,
            EventTime::from_nanos(event.state.time),
            position(&event.state)?,
            delta,
        )
        .with_modifiers(modifiers(event.state.modifiers)),
    )
}

fn gesture(event: &upstream::PointerGestureEvent) -> Option<PanZoomEvent> {
    let pointer = info(&event.pointer, PointerKind::Trackpad)?;
    let (scale, rotation) = match event.gesture {
        upstream::PointerGesture::Pinch(delta) => (1.0 + f64::from(delta), 0.0),
        upstream::PointerGesture::Rotate(radians) => (1.0, f64::from(radians)),
    };
    let transform = PanZoomTransform::try_new(Offset::new(0.0, 0.0), scale, rotation).ok()?;
    Some(
        PanZoomEvent::new(
            pointer,
            EventTime::from_nanos(event.state.time),
            position(&event.state)?,
            PanZoomPhase::Update(transform),
        )
        .with_modifiers(modifiers(event.state.modifiers)),
    )
}

/// FLUI's named key for a `keyboard-types` one, matched by W3C spelling; the legacy `Hyper`
/// and `Super` are `Meta`, and a key the generated table does not list is `Unidentified`.
#[must_use]
pub fn named_key(key: upstream_keyboard::NamedKey) -> NamedKey {
    let spelling = key.to_string();
    match spelling.as_str() {
        "Hyper" | "Super" => NamedKey::Meta,
        spelling => NamedKey::from_w3c(spelling).unwrap_or(NamedKey::Unidentified),
    }
}

/// FLUI's physical key for a `keyboard-types` one, matched by W3C spelling; a code the
/// generated table does not list is `Unidentified`.
#[must_use]
pub fn code(code: upstream_keyboard::Code) -> Code {
    Code::from_w3c(&code.to_string()).unwrap_or(Code::Unidentified)
}

/// FLUI's key event for a `keyboard-types` one; `observed` is when the backend saw it, which
/// `keyboard-types` does not carry.
#[must_use]
pub fn key_event(event: &upstream_keyboard::KeyboardEvent, observed: EventTime) -> KeyEvent {
    let state = match event.state {
        upstream_keyboard::KeyState::Down => KeyState::Down,
        upstream_keyboard::KeyState::Up => KeyState::Up,
    };
    let key = match &event.key {
        upstream_keyboard::Key::Character(text) => Key::character(text.as_str()),
        upstream_keyboard::Key::Named(named) => Key::Named(named_key(*named)),
    };
    let location = match event.location {
        upstream_keyboard::Location::Standard => Location::Standard,
        upstream_keyboard::Location::Left => Location::Left,
        upstream_keyboard::Location::Right => Location::Right,
        upstream_keyboard::Location::Numpad => Location::Numpad,
    };
    let repeat = if event.repeat {
        KeyRepeat::AutoRepeat
    } else {
        KeyRepeat::First
    };
    let composition = if event.is_composing {
        ImeComposition::Active
    } else {
        ImeComposition::Inactive
    };
    KeyEvent::new(state, key, code(event.code), observed)
        .with_location(location)
        .with_modifiers(modifiers(event.modifiers))
        .with_repeat(repeat)
        .with_composition(composition)
}
