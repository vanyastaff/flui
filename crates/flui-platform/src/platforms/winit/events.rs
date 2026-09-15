//! Winit event conversion to W3C ui-events
//!
//! Converts winit 0.30 events to W3C-compliant PlatformInput types.

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::{
    keyboard::{KeyState, KeyboardEvent},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
        PointerOrientation, PointerState, PointerType, PointerUpdate,
    },
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::{
    shared::events::{event_timestamp_ns, primary_mouse_info},
    traits::PlatformInput,
};

/// Build a `PointerState` from position and scale factor.
///
/// `buttons` is the set of buttons HELD at this instant — the W3C
/// `PointerEvent.buttons` field, and what the gesture layer uses to tell a
/// drag-move from a hover (`flui-interaction`'s own move constructors stamp
/// the held set the same way). A move that always reports an empty set is
/// classified as a hover, so an active pan never receives updates and
/// drag-scrolling in a live window silently does nothing.
/// `count` is the W3C click/tap count: `1` on Down/Up transitions, `0` on
/// motion, hover, scroll and gesture states — the synthetic constructors
/// stamp the same rule, so a recognizer reading it sees one contract on
/// both wires.
fn pointer_state(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    pressure: f32,
    modifiers: KeyboardModifiers,
    buttons: PointerButtons,
    count: u8,
) -> PointerState {
    let logical_x = position.x / scale_factor;
    let logical_y = position.y / scale_factor;

    PointerState {
        time: event_timestamp_ns(),
        position: PhysicalPosition::new(logical_x, logical_y),
        buttons,
        modifiers,
        count,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: PointerOrientation::default(),
        pressure,
        tangential_pressure: 0.0,
        scale_factor,
    }
}

/// Convert winit MouseButton to W3C PointerButton
pub(crate) fn convert_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Auxiliary,
        MouseButton::Back => PointerButton::X1,
        MouseButton::Forward => PointerButton::X2,
        MouseButton::Other(id) => vendor_button(id),
    }
}

/// Map a vendor-specific button id onto ui-events' exotic-button band.
///
/// winit reports mouse buttons beyond left/right/middle/back/forward as
/// `Other(id)` with a backend-specific id. Such a button must never alias
/// onto an actuating button — `Primary` in particular makes every tap/click
/// recognizer treat the press as tap-eligible, so a vendor side-button would
/// click whatever is under the cursor. ui-events reserves `B7`..`B32` for
/// exactly these devices; the id is folded onto that band with a modulo, so
/// the release of a vendor button always carries the same `PointerButton`
/// as its press. Two ids a multiple of the band width apart share a slot —
/// acceptable, because nothing in the gesture layer actuates on this band;
/// determinism per id is the load-bearing property (the raw winit button
/// set tracked by the platform, not this normalized value, is what decides
/// which physical buttons are still held).
fn vendor_button(id: u16) -> PointerButton {
    const EXOTIC_BAND: [PointerButton; 26] = [
        PointerButton::B7,
        PointerButton::B8,
        PointerButton::B9,
        PointerButton::B10,
        PointerButton::B11,
        PointerButton::B12,
        PointerButton::B13,
        PointerButton::B14,
        PointerButton::B15,
        PointerButton::B16,
        PointerButton::B17,
        PointerButton::B18,
        PointerButton::B19,
        PointerButton::B20,
        PointerButton::B21,
        PointerButton::B22,
        PointerButton::B23,
        PointerButton::B24,
        PointerButton::B25,
        PointerButton::B26,
        PointerButton::B27,
        PointerButton::B28,
        PointerButton::B29,
        PointerButton::B30,
        PointerButton::B31,
        PointerButton::B32,
    ];
    EXOTIC_BAND[usize::from(id) % EXOTIC_BAND.len()]
}

/// Convert winit modifiers state to keyboard-types Modifiers
pub fn convert_modifiers(modifiers: winit::event::Modifiers) -> KeyboardModifiers {
    let state = modifiers.state();
    let mut mods = KeyboardModifiers::empty();

    if state.shift_key() {
        mods |= KeyboardModifiers::SHIFT;
    }
    if state.control_key() {
        mods |= KeyboardModifiers::CONTROL;
    }
    if state.alt_key() {
        mods |= KeyboardModifiers::ALT;
    }
    if state.super_key() {
        mods |= KeyboardModifiers::META;
    }

    mods
}

/// Convert winit CursorMoved to W3C PointerEvent::Move
///
/// `held_buttons` comes from the platform's tracked button state (winit's
/// `CursorMoved` carries no button information of its own): with a button
/// held this is a drag-move, without one a hover.
pub fn cursor_moved_event(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
    held_buttons: PointerButtons,
) -> PlatformInput {
    let pressure = if held_buttons == PointerButtons::default() {
        0.0
    } else {
        0.5
    };
    let state = pointer_state(position, scale_factor, pressure, modifiers, held_buttons, 0);

    let event = PointerEvent::Move(PointerUpdate {
        pointer: primary_mouse_info(),
        current: state,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    });

    PlatformInput::Pointer(event)
}

/// Convert winit MouseInput to W3C PointerEvent::Down/Up
/// `held_buttons` is the set held AFTER this transition (press included /
/// release excluded), per the W3C `buttons` contract for down/up events.
pub fn mouse_button_event(
    button: MouseButton,
    state: ElementState,
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
    held_buttons: PointerButtons,
) -> PlatformInput {
    let is_down = state == ElementState::Pressed;
    let pointer_button = convert_mouse_button(button);
    let pressure = if is_down { 0.5 } else { 0.0 };
    let pointer_state = pointer_state(position, scale_factor, pressure, modifiers, held_buttons, 1);

    let event = if is_down {
        PointerEvent::Down(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state: pointer_state,
            button: Some(pointer_button),
        })
    } else {
        PointerEvent::Up(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state: pointer_state,
            button: Some(pointer_button),
        })
    };

    PlatformInput::Pointer(event)
}

/// Convert winit `Touch` to a per-contact W3C pointer event.
///
/// Contact identity: `pointer_id` is allocated by the platform from the
/// `(device, contact)` pair (see `WinitPlatformState::touch_contacts`) —
/// never `PointerId::PRIMARY` (the mouse), never shared between two live
/// contacts even across touch devices, and never reused within a session.
///
/// Buttons: a touch contact IS the primary "button" for its whole
/// Started..Ended span — the W3C contract reports `buttons = 1` while any
/// part of the finger touches. Move events must carry it, or the gesture
/// layer classifies them as hovers and an active pan never receives
/// updates (the exact live-drag failure the mouse path once shipped).
pub fn touch_event(
    touch: winit::event::Touch,
    pointer_id: u64,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    use winit::event::TouchPhase;

    let info = PointerInfo {
        pointer_id: PointerId::new(pointer_id),
        pointer_type: PointerType::Touch,
        persistent_device_id: None,
    };
    // Hardware without force reporting gets the same 0.5 stand-in the mouse
    // path uses for a held button.
    let contact_pressure = touch.force.map_or(0.5, |force| force.normalized() as f32);
    let contact_buttons = PointerButtons::from(PointerButton::Primary);

    let event = match touch.phase {
        TouchPhase::Started => PointerEvent::Down(PointerButtonEvent {
            pointer: info,
            state: pointer_state(
                touch.location,
                scale_factor,
                contact_pressure,
                modifiers,
                contact_buttons,
                1,
            ),
            button: Some(PointerButton::Primary),
        }),
        TouchPhase::Moved => PointerEvent::Move(PointerUpdate {
            pointer: info,
            current: pointer_state(
                touch.location,
                scale_factor,
                contact_pressure,
                modifiers,
                contact_buttons,
                0,
            ),
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }),
        TouchPhase::Ended => PointerEvent::Up(PointerButtonEvent {
            pointer: info,
            state: pointer_state(
                touch.location,
                scale_factor,
                0.0,
                modifiers,
                PointerButtons::default(),
                1,
            ),
            button: Some(PointerButton::Primary),
        }),
        TouchPhase::Cancelled => PointerEvent::Cancel(info),
    };

    PlatformInput::Pointer(event)
}

/// Convert a winit trackpad pinch or rotation tick into a
/// `PointerEvent::Gesture` — the producer side the pan-zoom lane never had
/// (its consumer chain, `from_w3c_event` → `Listener::on_pointer_pan_zoom_update`,
/// existed with zero producers on any backend).
///
/// Delta conventions, each converted AT THIS boundary:
/// - winit `PinchGesture.delta` is a magnification fraction (positive =
///   zoom in) — the exact semantics of `PointerGesture::Pinch`, passed
///   through (NaN, which winit documents as possible, is dropped by the
///   caller).
/// - winit `RotationGesture.delta` is COUNTERCLOCKWISE degrees;
///   `PointerGesture::Rotate` wants CLOCKWISE radians — negate and convert.
///
/// winit's `PanGesture` (iOS-only) has no `PointerGesture` counterpart and
/// macOS trackpad pans already arrive as `MouseWheel` `PixelDelta` ticks;
/// `DoubleTapGesture` carries no delta to translate. Both stay untranslated
/// deliberately.
///
/// The synthetic gesture pointer: gestures arrive without a pointer id from
/// winit; the whole tick stream shares one identity distinct from the mouse
/// and any touch contact, typed `Touch` (a trackpad gesture is a touch-class
/// input, and the binding's ephemeral no-contact dispatch keys on the id
/// only).
pub fn trackpad_gesture_event(
    gesture: ui_events::pointer::PointerGesture,
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    let event = PointerEvent::Gesture(ui_events::pointer::PointerGestureEvent {
        pointer: PointerInfo {
            pointer_id: PointerId::new(crate::shared::gestures::GESTURE_POINTER_ID),
            pointer_type: PointerType::Touch,
            persistent_device_id: None,
        },
        gesture,
        state: pointer_state(
            position,
            scale_factor,
            0.0,
            modifiers,
            PointerButtons::default(),
            0,
        ),
    });

    PlatformInput::Pointer(event)
}

/// Convert winit MouseWheel to W3C PointerEvent::Scroll
pub fn mouse_wheel_event(
    delta: MouseScrollDelta,
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    // Normalized at THIS boundary so every backend hands consumers the
    // same convention — the oracle's `scrollDelta`: positive = content
    // scrolls down (the web backend's DOM `deltaY` already arrives that
    // way). winit's wheel axes are the inverse (positive = away from the
    // user), and its `PixelDelta` is PHYSICAL pixels while pointer
    // positions (and the scroll positions consuming this) are logical —
    // flip the sign and divide by the scale factor here (via the shared
    // per-backend table in `crate::shared::scroll`), never in a
    // platform-neutral widget.
    let scroll_delta = match delta {
        MouseScrollDelta::LineDelta(x, y) => crate::shared::scroll::from_winit_lines(x, y),
        MouseScrollDelta::PixelDelta(pos) => {
            crate::shared::scroll::from_winit_pixels(pos.x, pos.y, scale_factor)
        }
    };

    let state = pointer_state(
        position,
        scale_factor,
        0.0,
        modifiers,
        PointerButtons::default(),
        0,
    );

    let event = PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
        pointer: primary_mouse_info(),
        state,
        delta: scroll_delta,
    });

    PlatformInput::Pointer(event)
}

/// Convert winit's `Ime` event to [`flui_types::ImeEvent`].
///
/// A pure, unit-tested mapping: winit's `Ime` enum is already
/// [`flui_types::ImeEvent`]'s reference shape (see that type's module doc),
/// so this is a direct variant-for-variant translation with no coordinate
/// or encoding conversion.
pub fn ime_event(event: &winit::event::Ime) -> PlatformInput {
    use winit::event::Ime;

    let ime_event = match event {
        Ime::Enabled => flui_types::ImeEvent::Enabled,
        Ime::Preedit(text, cursor) => flui_types::ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        Ime::Commit(text) => flui_types::ImeEvent::Commit(text.clone()),
        Ime::Disabled => flui_types::ImeEvent::Disabled,
    };

    PlatformInput::Ime(ime_event)
}

/// Convert winit KeyboardInput to W3C KeyboardEvent
pub fn keyboard_event(
    event: &winit::event::KeyEvent,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    // Physical code, logical key, and location all delegate to
    // `ui-events-winit` (the ecosystem bridge from winit's native keyboard
    // types to this crate's canonical `ui_events`/`keyboard-types`
    // vocabulary — also used by Masonry) rather than a hand-written table
    // maintained here. `Code::Unidentified` therefore means exactly one
    // thing: winit itself reported `PhysicalKey::Unidentified` for this
    // press, i.e. the OS/backend could not name the physical key — never
    // an incomplete conversion table. The Win32 and AppKit backends still
    // hand-write their own `Code`/`Key` tables in `shared/keys.rs` and
    // `shared/keys_macos.rs` because no such bridge crate exists for their
    // native scancode/keycode spaces; `cross_backend_physical_key_agreement`
    // below is what keeps this delegation and those two tables in
    // agreement.
    let key = ui_events_winit::keyboard::from_winit_key(event.logical_key.clone());
    let code = ui_events_winit::keyboard::from_winit_code(event.physical_key);
    let location = ui_events_winit::keyboard::from_winit_location(event.location);
    let state = match event.state {
        ElementState::Pressed => KeyState::Down,
        ElementState::Released => KeyState::Up,
    };

    let keyboard_event = KeyboardEvent {
        state,
        key,
        code,
        location,
        modifiers,
        repeat: event.repeat,
        is_composing: false,
    };

    PlatformInput::Keyboard(keyboard_event)
}

#[cfg(test)]
mod pointer_translation_tests {
    use std::time::Instant;

    use super::*;
    use ui_events::{ScrollDelta, pointer::PointerEvent};

    /// A cursor move with a button held is a DRAG move: the emitted
    /// `PointerState.buttons` carries the held set (what the gesture layer
    /// uses to route the move to an active pan instead of the hover path)
    /// and a non-zero pressure, mirroring `flui-interaction`'s own move
    /// constructors. An empty set stays a hover.
    #[test]
    fn cursor_move_with_held_button_is_a_drag_not_a_hover() {
        let held = PointerButtons::from(PointerButton::Primary);
        let input = cursor_moved_event(
            winit::dpi::PhysicalPosition::new(100.0, 100.0),
            1.0,
            KeyboardModifiers::empty(),
            held,
        );
        let PlatformInput::Pointer(PointerEvent::Move(update)) = input else {
            panic!("cursor move must translate to PointerEvent::Move");
        };
        assert_eq!(update.current.buttons, held);
        assert!(update.current.pressure > 0.0);

        let hover = cursor_moved_event(
            winit::dpi::PhysicalPosition::new(100.0, 100.0),
            1.0,
            KeyboardModifiers::empty(),
            PointerButtons::default(),
        );
        let PlatformInput::Pointer(PointerEvent::Move(update)) = hover else {
            panic!("cursor move must translate to PointerEvent::Move");
        };
        assert_eq!(update.current.buttons, PointerButtons::default());
        assert_eq!(update.current.pressure, 0.0);
    }

    /// Wheel deltas are normalized at this boundary to the cross-backend
    /// convention — positive = content scrolls down — with `PixelDelta`
    /// converted from winit's physical pixels to logical: a 2x-DPI
    /// trackpad tick must not scroll twice as far.
    #[test]
    fn wheel_deltas_are_normalized_and_logical() {
        let line = mouse_wheel_event(
            MouseScrollDelta::LineDelta(0.0, -1.0),
            winit::dpi::PhysicalPosition::new(0.0, 0.0),
            2.0,
            KeyboardModifiers::empty(),
        );
        let PlatformInput::Pointer(PointerEvent::Scroll(event)) = line else {
            panic!("wheel must translate to PointerEvent::Scroll");
        };
        // winit's wheel-down is a NEGATIVE line-y; normalized it is
        // POSITIVE (content down). Lines are unit-less: no DPI scaling.
        assert!(matches!(event.delta, ScrollDelta::LineDelta(x, y) if x == 0.0 && y == 1.0));

        let pixel = mouse_wheel_event(
            MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, -100.0)),
            winit::dpi::PhysicalPosition::new(0.0, 0.0),
            2.0,
            KeyboardModifiers::empty(),
        );
        let PlatformInput::Pointer(PointerEvent::Scroll(event)) = pixel else {
            panic!("wheel must translate to PointerEvent::Scroll");
        };
        let ScrollDelta::PixelDelta(delta) = event.delta else {
            panic!("pixel deltas must stay pixel deltas");
        };
        assert_eq!(
            (delta.x, delta.y),
            (0.0, 50.0),
            "sign flipped to content-down-positive AND physical 100 at 2x \
             DPI becomes logical 50"
        );
    }

    /// Down/up events carry the post-transition held set — press included,
    /// release excluded (the W3C `buttons` contract).
    #[test]
    fn button_events_carry_the_post_transition_held_set() {
        let after_press = PointerButtons::from(PointerButton::Primary);
        let down = mouse_button_event(
            MouseButton::Left,
            ElementState::Pressed,
            winit::dpi::PhysicalPosition::new(0.0, 0.0),
            1.0,
            KeyboardModifiers::empty(),
            after_press,
        );
        let PlatformInput::Pointer(PointerEvent::Down(event)) = down else {
            panic!("press must translate to PointerEvent::Down");
        };
        assert_eq!(event.state.buttons, after_press);

        let up = mouse_button_event(
            MouseButton::Left,
            ElementState::Released,
            winit::dpi::PhysicalPosition::new(0.0, 0.0),
            1.0,
            KeyboardModifiers::empty(),
            PointerButtons::default(),
        );
        let PlatformInput::Pointer(PointerEvent::Up(event)) = up else {
            panic!("release must translate to PointerEvent::Up");
        };
        assert_eq!(event.state.buttons, PointerButtons::default());
    }

    /// A vendor side-button (winit `MouseButton::Other`) must never
    /// translate to an actuating button: `Primary` makes every tap
    /// recognizer treat the press as a left click, so button 6/7/... on a
    /// gaming mouse would click whatever is under the cursor. It lands in
    /// ui-events' exotic `B7`..`B32` band, deterministically per id, so a
    /// release always carries the same button as its press.
    #[test]
    fn vendor_side_buttons_do_not_synthesize_primary_clicks() {
        let actuating = [
            PointerButton::Primary,
            PointerButton::Secondary,
            PointerButton::Auxiliary,
            PointerButton::X1,
            PointerButton::X2,
        ];
        for id in [0u16, 6, 7, 25, 26, 1000] {
            let converted = convert_mouse_button(MouseButton::Other(id));
            for banned in actuating {
                assert_ne!(
                    converted, banned,
                    "Other({id}) must not alias onto the actuating button {banned:?}"
                );
            }
            assert_eq!(
                converted,
                convert_mouse_button(MouseButton::Other(id)),
                "press and release of Other({id}) must carry the same button"
            );
        }

        // Full translation shape: the Down event for Other(6) carries the
        // non-actuating button, and the held set never gains Primary.
        let side_button = convert_mouse_button(MouseButton::Other(6));
        let down = mouse_button_event(
            MouseButton::Other(6),
            ElementState::Pressed,
            winit::dpi::PhysicalPosition::new(10.0, 10.0),
            1.0,
            KeyboardModifiers::empty(),
            PointerButtons::from(side_button),
        );
        let PlatformInput::Pointer(PointerEvent::Down(event)) = down else {
            panic!("press must translate to PointerEvent::Down");
        };
        assert_eq!(event.button, Some(side_button));
        assert_ne!(event.button, Some(PointerButton::Primary));
        assert!(!event.state.buttons.contains(PointerButton::Primary));
    }

    /// A touch contact must never alias the mouse pointer, must stamp the
    /// primary "button" for its whole contact span (a buttons-empty touch
    /// move is classified as a hover and kills live pan tracking), and must
    /// map every winit phase onto the matching pointer event.
    #[test]
    fn touch_phases_map_to_distinct_contact_pointer_events() {
        use winit::event::{Force, Touch, TouchPhase};

        let dev = winit::event::DeviceId::dummy();
        let touch = |phase, id| Touch {
            device_id: dev,
            phase,
            location: winit::dpi::PhysicalPosition::new(200.0, 100.0),
            force: Some(Force::Normalized(0.75)),
            id,
        };
        let pointer = |input: PlatformInput| match input {
            PlatformInput::Pointer(event) => event,
            other => panic!("expected a pointer event, got {other:?}"),
        };

        let down = pointer(touch_event(
            touch(TouchPhase::Started, 0),
            2,
            2.0,
            KeyboardModifiers::empty(),
        ));
        let PointerEvent::Down(down) = down else {
            panic!("Started must translate to Down, got {down:?}");
        };
        assert_eq!(down.pointer.pointer_type, PointerType::Touch);
        assert_ne!(
            down.pointer.pointer_id,
            Some(PointerId::PRIMARY),
            "contact 0 must not alias the mouse's PRIMARY pointer id"
        );
        assert_eq!(down.state.position.x, 100.0, "positions are logical");
        assert!(
            down.state.buttons.contains(PointerButton::Primary),
            "a contact stamps the primary button"
        );
        assert!((down.state.pressure - 0.75).abs() < 1e-6);

        let moved = pointer(touch_event(
            touch(TouchPhase::Moved, 0),
            2,
            2.0,
            KeyboardModifiers::empty(),
        ));
        let PointerEvent::Move(moved) = moved else {
            panic!("Moved must translate to Move, got {moved:?}");
        };
        assert!(
            moved.current.buttons.contains(PointerButton::Primary),
            "a contact move carries the held set — an empty set is a hover"
        );

        let up = pointer(touch_event(
            touch(TouchPhase::Ended, 0),
            2,
            2.0,
            KeyboardModifiers::empty(),
        ));
        let PointerEvent::Up(up) = up else {
            panic!("Ended must translate to Up, got {up:?}");
        };
        assert_eq!(
            up.state.buttons,
            PointerButtons::default(),
            "after release nothing is held"
        );

        let cancel = pointer(touch_event(
            touch(TouchPhase::Cancelled, 0),
            2,
            2.0,
            KeyboardModifiers::empty(),
        ));
        assert!(
            matches!(cancel, PointerEvent::Cancel(_)),
            "Cancelled must translate to Cancel, got {cancel:?}"
        );

        // Two simultaneous contacts stay distinct pointers.
        let second = pointer(touch_event(
            touch(TouchPhase::Started, 1),
            3,
            2.0,
            KeyboardModifiers::empty(),
        ));
        let PointerEvent::Down(second) = second else {
            panic!("expected Down");
        };
        assert_ne!(second.pointer.pointer_id, down.pointer.pointer_id);
    }

    /// A trackpad gesture tick becomes a `PointerEvent::Gesture` with one
    /// stable synthetic identity that can never alias the mouse or a touch
    /// contact, logical coordinates, and the delta passed through in the
    /// lane's own convention.
    #[test]
    fn trackpad_gestures_produce_gesture_events_with_a_distinct_identity() {
        use ui_events::pointer::PointerGesture;

        let pinch = trackpad_gesture_event(
            PointerGesture::Pinch(0.1),
            winit::dpi::PhysicalPosition::new(200.0, 100.0),
            2.0,
            KeyboardModifiers::empty(),
        );
        let PlatformInput::Pointer(PointerEvent::Gesture(pinch)) = pinch else {
            panic!("expected a gesture event, got {pinch:?}");
        };
        assert!(matches!(pinch.gesture, PointerGesture::Pinch(delta) if delta == 0.1));
        assert_eq!(pinch.pointer.pointer_type, PointerType::Touch);
        assert_ne!(
            pinch.pointer.pointer_id,
            Some(PointerId::PRIMARY),
            "the gesture stream must not alias the mouse"
        );
        assert_eq!(pinch.state.position.x, 100.0, "positions are logical");

        let rotate = trackpad_gesture_event(
            PointerGesture::Rotate(-0.5),
            winit::dpi::PhysicalPosition::new(0.0, 0.0),
            1.0,
            KeyboardModifiers::empty(),
        );
        let PlatformInput::Pointer(PointerEvent::Gesture(rotate)) = rotate else {
            panic!("expected a gesture event, got {rotate:?}");
        };
        assert!(matches!(rotate.gesture, PointerGesture::Rotate(delta) if delta == -0.5));
        assert_eq!(
            rotate.pointer.pointer_id, pinch.pointer.pointer_id,
            "one gesture stream, one identity"
        );
    }

    /// The cross-wire field contract (flui-interaction's module doc): time
    /// in NANOSECONDS, pressure 0.5 while a button is held on sensor-less
    /// hardware, click count 1 on transitions and 0 on motion/scroll.
    #[test]
    fn translated_events_meet_the_pointer_field_contract() {
        let position = winit::dpi::PhysicalPosition::new(10.0, 10.0);
        let held = PointerButtons::from(PointerButton::Primary);

        // time: nanosecond scale — spin ~2ms of real time between two
        // stamps; a millisecond stamp would show a delta of ~2, a
        // nanosecond stamp ~2_000_000. (A bounded spin on Instant, not a
        // pacing sleep: elapsed time IS the measured phenomenon here.)
        let PlatformInput::Pointer(PointerEvent::Move(first)) = cursor_moved_event(
            position,
            1.0,
            KeyboardModifiers::empty(),
            PointerButtons::default(),
        ) else {
            panic!("expected Move");
        };
        let spin_start = Instant::now();
        while spin_start.elapsed() < std::time::Duration::from_millis(2) {
            std::hint::spin_loop();
        }
        let PlatformInput::Pointer(PointerEvent::Move(second)) = cursor_moved_event(
            position,
            1.0,
            KeyboardModifiers::empty(),
            PointerButtons::default(),
        ) else {
            panic!("expected Move");
        };
        let delta = second.current.time - first.current.time;
        assert!(
            delta >= 1_000_000,
            "~2ms between stamps must read as ~2,000,000 time units — the \
             contract is nanoseconds, and a millisecond stamp reads {delta}"
        );

        // pressure + count on a Down with a held button.
        let PlatformInput::Pointer(PointerEvent::Down(down)) = mouse_button_event(
            MouseButton::Left,
            ElementState::Pressed,
            position,
            1.0,
            KeyboardModifiers::empty(),
            held,
        ) else {
            panic!("expected Down");
        };
        assert_eq!(down.state.pressure, 0.5, "W3C sensor-less held default");
        assert_eq!(down.state.count, 1, "a Down is a click transition");

        // A hover move carries neither.
        assert_eq!(first.current.pressure, 0.0);
        assert_eq!(first.current.count, 0, "motion is not a click");
    }
}

#[cfg(test)]
mod ime_tests {
    use flui_types::ImeEvent;
    use winit::event::Ime;

    use super::ime_event;
    use crate::traits::PlatformInput;

    /// Unwraps the `PlatformInput::Ime` arm `ime_event` always produces,
    /// asserting the wrapping variant at the same time so a future change
    /// that wraps IME events in a different `PlatformInput` variant fails
    /// loudly here instead of silently changing what these tests check.
    fn convert(event: &Ime) -> ImeEvent {
        match ime_event(event) {
            PlatformInput::Ime(inner) => inner,
            other => panic!("ime_event must return PlatformInput::Ime, got {other:?}"),
        }
    }

    #[test]
    fn enabled_maps_to_enabled() {
        assert_eq!(convert(&Ime::Enabled), ImeEvent::Enabled);
    }

    #[test]
    fn disabled_maps_to_disabled() {
        assert_eq!(convert(&Ime::Disabled), ImeEvent::Disabled);
    }

    #[test]
    fn commit_carries_the_delivered_text() {
        assert_eq!(
            convert(&Ime::Commit("hello".to_string())),
            ImeEvent::Commit("hello".to_string())
        );
    }

    #[test]
    fn preedit_with_a_cursor_position_is_preserved() {
        assert_eq!(
            convert(&Ime::Preedit("ni".to_string(), Some((1, 2)))),
            ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: Some((1, 2)),
            }
        );
    }

    #[test]
    fn preedit_with_no_cursor_hides_the_caret() {
        assert_eq!(
            convert(&Ime::Preedit("ni".to_string(), None)),
            ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: None,
            }
        );
    }
}

#[cfg(test)]
mod keyboard_conversion_tests {
    //! Pins the winit backend's delegation to `ui-events-winit` (see
    //! `keyboard_event`'s doc) rather than exercising `keyboard_event`
    //! itself: `winit::event::KeyEvent` has a `pub(crate) platform_specific`
    //! field, so nothing outside winit can construct one, and
    //! `keyboard_event` adds no logic of its own beyond forwarding to the
    //! three `ui_events_winit::keyboard::from_winit_*` functions tested
    //! here directly — so this is exact coverage of the code path, not a
    //! substitute for it.
    use ui_events::keyboard::{Code, Key, Location, NamedKey};
    use ui_events_winit::keyboard::{from_winit_code, from_winit_key, from_winit_location};
    use winit::keyboard::{
        Key as WinitKey, KeyCode, KeyLocation, NamedKey as WinitNamedKey, PhysicalKey,
    };

    /// Every `winit::keyboard::KeyCode` variant, winit 0.30.13
    /// (`winit-0.30.13/src/keyboard.rs`, the `KeyCode` enum), in the exact
    /// order it is declared there. The length assertion in the test below
    /// is what catches a silently truncated copy of this list.
    const ALL_WINIT_KEYCODES: &[KeyCode] = &[
        KeyCode::Backquote,
        KeyCode::Backslash,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Comma,
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Equal,
        KeyCode::IntlBackslash,
        KeyCode::IntlRo,
        KeyCode::IntlYen,
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
        KeyCode::Minus,
        KeyCode::Period,
        KeyCode::Quote,
        KeyCode::Semicolon,
        KeyCode::Slash,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::Backspace,
        KeyCode::CapsLock,
        KeyCode::ContextMenu,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::Enter,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::Space,
        KeyCode::Tab,
        KeyCode::Convert,
        KeyCode::KanaMode,
        KeyCode::Lang1,
        KeyCode::Lang2,
        KeyCode::Lang3,
        KeyCode::Lang4,
        KeyCode::Lang5,
        KeyCode::NonConvert,
        KeyCode::Delete,
        KeyCode::End,
        KeyCode::Help,
        KeyCode::Home,
        KeyCode::Insert,
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::NumLock,
        KeyCode::Numpad0,
        KeyCode::Numpad1,
        KeyCode::Numpad2,
        KeyCode::Numpad3,
        KeyCode::Numpad4,
        KeyCode::Numpad5,
        KeyCode::Numpad6,
        KeyCode::Numpad7,
        KeyCode::Numpad8,
        KeyCode::Numpad9,
        KeyCode::NumpadAdd,
        KeyCode::NumpadBackspace,
        KeyCode::NumpadClear,
        KeyCode::NumpadClearEntry,
        KeyCode::NumpadComma,
        KeyCode::NumpadDecimal,
        KeyCode::NumpadDivide,
        KeyCode::NumpadEnter,
        KeyCode::NumpadEqual,
        KeyCode::NumpadHash,
        KeyCode::NumpadMemoryAdd,
        KeyCode::NumpadMemoryClear,
        KeyCode::NumpadMemoryRecall,
        KeyCode::NumpadMemoryStore,
        KeyCode::NumpadMemorySubtract,
        KeyCode::NumpadMultiply,
        KeyCode::NumpadParenLeft,
        KeyCode::NumpadParenRight,
        KeyCode::NumpadStar,
        KeyCode::NumpadSubtract,
        KeyCode::Escape,
        KeyCode::Fn,
        KeyCode::FnLock,
        KeyCode::PrintScreen,
        KeyCode::ScrollLock,
        KeyCode::Pause,
        KeyCode::BrowserBack,
        KeyCode::BrowserFavorites,
        KeyCode::BrowserForward,
        KeyCode::BrowserHome,
        KeyCode::BrowserRefresh,
        KeyCode::BrowserSearch,
        KeyCode::BrowserStop,
        KeyCode::Eject,
        KeyCode::LaunchApp1,
        KeyCode::LaunchApp2,
        KeyCode::LaunchMail,
        KeyCode::MediaPlayPause,
        KeyCode::MediaSelect,
        KeyCode::MediaStop,
        KeyCode::MediaTrackNext,
        KeyCode::MediaTrackPrevious,
        KeyCode::Power,
        KeyCode::Sleep,
        KeyCode::AudioVolumeDown,
        KeyCode::AudioVolumeMute,
        KeyCode::AudioVolumeUp,
        KeyCode::WakeUp,
        KeyCode::Meta,
        KeyCode::Hyper,
        KeyCode::Turbo,
        KeyCode::Abort,
        KeyCode::Resume,
        KeyCode::Suspend,
        KeyCode::Again,
        KeyCode::Copy,
        KeyCode::Cut,
        KeyCode::Find,
        KeyCode::Open,
        KeyCode::Paste,
        KeyCode::Props,
        KeyCode::Select,
        KeyCode::Undo,
        KeyCode::Hiragana,
        KeyCode::Katakana,
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
        KeyCode::F13,
        KeyCode::F14,
        KeyCode::F15,
        KeyCode::F16,
        KeyCode::F17,
        KeyCode::F18,
        KeyCode::F19,
        KeyCode::F20,
        KeyCode::F21,
        KeyCode::F22,
        KeyCode::F23,
        KeyCode::F24,
        KeyCode::F25,
        KeyCode::F26,
        KeyCode::F27,
        KeyCode::F28,
        KeyCode::F29,
        KeyCode::F30,
        KeyCode::F31,
        KeyCode::F32,
        KeyCode::F33,
        KeyCode::F34,
        KeyCode::F35,
    ];

    /// Every winit 0.30.13 `KeyCode` has a non-`Unidentified` canonical
    /// `Code` through the production path — the property this whole issue
    /// is about. Before this change, `convert_physical_key`'s own hand
    /// table covered 71 of these 194 variants (letters, digits, a small
    /// nav/modifier subset, F1..F12) and fell through to `Unidentified`
    /// for the other 123 — numpad, Intl*, F13+, media/browser/system keys
    /// among them; seeing that failure again would mean `ui-events-winit`
    /// regressed or this crate stopped calling it.
    #[test]
    fn every_winit_keycode_maps_to_a_canonical_code() {
        assert_eq!(ALL_WINIT_KEYCODES.len(), 194);
        let unidentified: Vec<KeyCode> = ALL_WINIT_KEYCODES
            .iter()
            .copied()
            .filter(|&k| from_winit_code(PhysicalKey::Code(k)) == Code::Unidentified)
            .collect();
        assert!(
            unidentified.is_empty(),
            "{} winit KeyCode variant(s) produced Code::Unidentified: {unidentified:?}",
            unidentified.len()
        );
    }

    /// A `PhysicalKey::Unidentified` stays `Code::Unidentified` — the one
    /// case that value is reserved for (see `keyboard_event`'s doc): winit
    /// itself could not name the physical key, distinct from every case
    /// above where winit named one and the table failed to preserve it.
    #[test]
    fn physical_key_unidentified_stays_unidentified() {
        assert_eq!(
            from_winit_code(PhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified
            )),
            Code::Unidentified
        );
    }

    /// Spot pairs named in issue #1092's acceptance criteria, run through
    /// the exact function `keyboard_event` calls.
    #[test]
    fn spot_pairs_match_the_issues_acceptance_criteria() {
        let cases = [
            (KeyCode::NumpadEnter, Code::NumpadEnter),
            (KeyCode::IntlYen, Code::IntlYen),
            (KeyCode::AudioVolumeUp, Code::AudioVolumeUp),
            (KeyCode::F24, Code::F24),
            (KeyCode::ContextMenu, Code::ContextMenu),
            (KeyCode::Comma, Code::Comma),
            (KeyCode::CapsLock, Code::CapsLock),
        ];
        for (winit_code, expected) in cases {
            assert_eq!(
                from_winit_code(PhysicalKey::Code(winit_code)),
                expected,
                "{winit_code:?}"
            );
        }
    }

    /// `KeyLocation`'s four variants map name-for-name onto
    /// `ui_events::keyboard::Location`. Unlike the pre-fix code, location
    /// is no longer *derived* from the physical `Code` at all — it comes
    /// straight from winit's own `KeyEvent.location` field (see
    /// `keyboard_event`), so there is no "does `Code::X` imply
    /// `Location::Numpad`" property left to test here; that coupling is
    /// exactly what this fix removed.
    #[test]
    fn every_key_location_maps_to_its_canonical_location() {
        let cases = [
            (KeyLocation::Standard, Location::Standard),
            (KeyLocation::Left, Location::Left),
            (KeyLocation::Right, Location::Right),
            (KeyLocation::Numpad, Location::Numpad),
        ];
        for (winit_location, expected) in cases {
            assert_eq!(
                from_winit_location(winit_location),
                expected,
                "{winit_location:?}"
            );
        }
    }

    /// A handful of `NamedKey` pairs, plus the `Character` pass-through
    /// (including a non-ASCII character, since this crosses a
    /// `SmolStr`-to-`String` conversion). Exhaustive `NamedKey` coverage is
    /// `ui-events-winit`'s test suite's job, not ours; these pin the shape
    /// this crate relies on.
    #[test]
    fn logical_key_spot_pairs() {
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::Enter)),
            Key::Named(NamedKey::Enter)
        );
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::ContextMenu)),
            Key::Named(NamedKey::ContextMenu)
        );
        // winit intercepts Space as a named key; the canonical vocabulary
        // reports it as the character it types, matching every other
        // printable key.
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::Space)),
            Key::Character(" ".to_string())
        );
        assert_eq!(
            from_winit_key(WinitKey::Character("e".into())),
            Key::Character("e".to_string())
        );
        assert_eq!(
            from_winit_key(WinitKey::Character("\u{e9}".into())),
            Key::Character("\u{e9}".to_string())
        );
    }
}

#[cfg(test)]
mod cross_backend_physical_key_agreement {
    //! Cross-checks a curated set of physical keys that the winit, Win32,
    //! and AppKit conversions all claim to name explicitly (see
    //! `crate::shared::keys::scancode_to_code`'s and
    //! `crate::shared::keys_macos::keycode_to_code`'s own module docs), so a
    //! future edit to any one of the three tables that silently retargets a
    //! shared physical key is caught here instead of only shipping as a
    //! per-backend behavior change. Win32/AppKit scancodes and keycodes are
    //! transcribed directly from those modules' own match arms — read there
    //! before adding a row, since only the values THOSE tables actually use
    //! are meaningful cross-checks.
    //!
    //! Coverage is representative, not exhaustive: only keys where a
    //! meaning-preserving correspondence between all three native id spaces
    //! (a winit `KeyCode` variant, a Win32 scancode, an AppKit keycode) is
    //! unambiguous. Two families are deliberately excluded with a reason
    //! rather than silently omitted:
    //! - Win32's `KanaMode` (scancode `0x70`) has no AppKit counterpart —
    //!   the AppKit table maps its Kana key straight to `Lang1` (matching
    //!   Win32's *separate* `Lang1` scancode `0x72`), per the W3C code
    //!   registry's own platform-conditional reuse of the `Lang1`/`Lang2`
    //!   slots (Japanese Mac Eisu/Kana vs. Korean Hangul/Hanja) — not a
    //!   defect, so `KanaMode` stays a Win32-only row below.
    //! - Browser/media/launch keys (`BrowserBack`, `MediaPlayPause`, …) have
    //!   no AppKit table entries at all; those rows carry a Win32
    //!   scancode only.
    use ui_events::keyboard::Code;
    use ui_events_winit::keyboard::from_winit_code;
    use winit::keyboard::{KeyCode, PhysicalKey};

    use crate::shared::{keys::scancode_to_code, keys_macos::keycode_to_code};

    /// `(label, winit KeyCode, Win32 (scancode, extended), AppKit keycode)`.
    /// `None` means that backend has no table entry for this key.
    type PhysicalKeyRow = (&'static str, KeyCode, Option<(u16, bool)>, Option<u16>);

    const PHYSICAL_KEYS: &[PhysicalKeyRow] = &[
        // Letters/digits/punctuation (a sample, not all 26+10).
        ("KeyA", KeyCode::KeyA, Some((0x001E, false)), Some(0x00)),
        ("Digit1", KeyCode::Digit1, Some((0x0002, false)), Some(0x12)),
        ("Minus", KeyCode::Minus, Some((0x000C, false)), Some(0x1B)),
        ("Equal", KeyCode::Equal, Some((0x000D, false)), Some(0x18)),
        (
            "BracketLeft",
            KeyCode::BracketLeft,
            Some((0x001A, false)),
            Some(0x21),
        ),
        (
            "BracketRight",
            KeyCode::BracketRight,
            Some((0x001B, false)),
            Some(0x1E),
        ),
        (
            "Semicolon",
            KeyCode::Semicolon,
            Some((0x0027, false)),
            Some(0x29),
        ),
        ("Quote", KeyCode::Quote, Some((0x0028, false)), Some(0x27)),
        (
            "Backquote",
            KeyCode::Backquote,
            Some((0x0029, false)),
            Some(0x32),
        ),
        (
            "Backslash",
            KeyCode::Backslash,
            Some((0x002B, false)),
            Some(0x2A),
        ),
        ("Comma", KeyCode::Comma, Some((0x0033, false)), Some(0x2B)),
        ("Period", KeyCode::Period, Some((0x0034, false)), Some(0x2F)),
        ("Slash", KeyCode::Slash, Some((0x0035, false)), Some(0x2C)),
        // Editing / whitespace.
        ("Enter", KeyCode::Enter, Some((0x001C, false)), Some(0x24)),
        ("Escape", KeyCode::Escape, Some((0x0001, false)), Some(0x35)),
        ("Tab", KeyCode::Tab, Some((0x000F, false)), Some(0x30)),
        ("Space", KeyCode::Space, Some((0x0039, false)), Some(0x31)),
        (
            "Backspace",
            KeyCode::Backspace,
            Some((0x000E, false)),
            Some(0x33),
        ),
        (
            "CapsLock",
            KeyCode::CapsLock,
            Some((0x003A, false)),
            Some(0x39),
        ),
        // Sided modifiers.
        (
            "ShiftLeft",
            KeyCode::ShiftLeft,
            Some((0x002A, false)),
            Some(0x38),
        ),
        (
            "ShiftRight",
            KeyCode::ShiftRight,
            Some((0x0036, false)),
            Some(0x3C),
        ),
        (
            "ControlLeft",
            KeyCode::ControlLeft,
            Some((0x001D, false)),
            Some(0x3B),
        ),
        (
            "ControlRight",
            KeyCode::ControlRight,
            Some((0x001D, true)),
            Some(0x3E),
        ),
        (
            "AltLeft",
            KeyCode::AltLeft,
            Some((0x0038, false)),
            Some(0x3A),
        ),
        (
            "AltRight",
            KeyCode::AltRight,
            Some((0x0038, true)),
            Some(0x3D),
        ),
        (
            "MetaLeft",
            KeyCode::SuperLeft,
            Some((0x005B, true)),
            Some(0x37),
        ),
        (
            "MetaRight",
            KeyCode::SuperRight,
            Some((0x005C, true)),
            Some(0x36),
        ),
        // Navigation cluster.
        (
            "ArrowLeft",
            KeyCode::ArrowLeft,
            Some((0x004B, true)),
            Some(0x7B),
        ),
        (
            "ArrowRight",
            KeyCode::ArrowRight,
            Some((0x004D, true)),
            Some(0x7C),
        ),
        (
            "ArrowUp",
            KeyCode::ArrowUp,
            Some((0x0048, true)),
            Some(0x7E),
        ),
        (
            "ArrowDown",
            KeyCode::ArrowDown,
            Some((0x0050, true)),
            Some(0x7D),
        ),
        ("Home", KeyCode::Home, Some((0x0047, true)), Some(0x73)),
        ("End", KeyCode::End, Some((0x004F, true)), Some(0x77)),
        ("PageUp", KeyCode::PageUp, Some((0x0049, true)), Some(0x74)),
        (
            "PageDown",
            KeyCode::PageDown,
            Some((0x0051, true)),
            Some(0x79),
        ),
        // Apple's Insert/Delete labeling is a documented cross-platform
        // divergence in position, not in the `Code` value each produces.
        ("Insert", KeyCode::Insert, Some((0x0052, true)), Some(0x72)),
        ("Delete", KeyCode::Delete, Some((0x0053, true)), Some(0x75)),
        // NumLock: Win32's own key vs. the Mac numpad Clear key.
        (
            "NumLock",
            KeyCode::NumLock,
            Some((0x0045, true)),
            Some(0x47),
        ),
        // Numpad cluster.
        (
            "Numpad0",
            KeyCode::Numpad0,
            Some((0x0052, false)),
            Some(0x52),
        ),
        (
            "Numpad1",
            KeyCode::Numpad1,
            Some((0x004F, false)),
            Some(0x53),
        ),
        (
            "Numpad2",
            KeyCode::Numpad2,
            Some((0x0050, false)),
            Some(0x54),
        ),
        (
            "Numpad3",
            KeyCode::Numpad3,
            Some((0x0051, false)),
            Some(0x55),
        ),
        (
            "Numpad4",
            KeyCode::Numpad4,
            Some((0x004B, false)),
            Some(0x56),
        ),
        (
            "Numpad5",
            KeyCode::Numpad5,
            Some((0x004C, false)),
            Some(0x57),
        ),
        (
            "Numpad6",
            KeyCode::Numpad6,
            Some((0x004D, false)),
            Some(0x58),
        ),
        (
            "Numpad7",
            KeyCode::Numpad7,
            Some((0x0047, false)),
            Some(0x59),
        ),
        (
            "Numpad8",
            KeyCode::Numpad8,
            Some((0x0048, false)),
            Some(0x5B),
        ),
        (
            "Numpad9",
            KeyCode::Numpad9,
            Some((0x0049, false)),
            Some(0x5C),
        ),
        (
            "NumpadAdd",
            KeyCode::NumpadAdd,
            Some((0x004E, false)),
            Some(0x45),
        ),
        (
            "NumpadSubtract",
            KeyCode::NumpadSubtract,
            Some((0x004A, false)),
            Some(0x4E),
        ),
        (
            "NumpadMultiply",
            KeyCode::NumpadMultiply,
            Some((0x0037, false)),
            Some(0x43),
        ),
        (
            "NumpadDivide",
            KeyCode::NumpadDivide,
            Some((0x0035, true)),
            Some(0x4B),
        ),
        (
            "NumpadDecimal",
            KeyCode::NumpadDecimal,
            Some((0x0053, false)),
            Some(0x41),
        ),
        (
            "NumpadEnter",
            KeyCode::NumpadEnter,
            Some((0x001C, true)),
            Some(0x4C),
        ),
        (
            "NumpadEqual",
            KeyCode::NumpadEqual,
            Some((0x0059, false)),
            Some(0x51),
        ),
        (
            "NumpadComma",
            KeyCode::NumpadComma,
            Some((0x007E, false)),
            Some(0x5F),
        ),
        // System / IME keys.
        (
            "ContextMenu",
            KeyCode::ContextMenu,
            Some((0x005D, true)),
            Some(0x6E),
        ),
        ("Lang1", KeyCode::Lang1, Some((0x0072, false)), Some(0x68)),
        ("Lang2", KeyCode::Lang2, Some((0x0071, false)), Some(0x66)),
        (
            "IntlBackslash",
            KeyCode::IntlBackslash,
            Some((0x0056, false)),
            Some(0x0A),
        ),
        ("IntlRo", KeyCode::IntlRo, Some((0x0073, false)), Some(0x5E)),
        (
            "IntlYen",
            KeyCode::IntlYen,
            Some((0x007D, false)),
            Some(0x5D),
        ),
        // Media keys.
        (
            "AudioVolumeUp",
            KeyCode::AudioVolumeUp,
            Some((0x0030, true)),
            Some(0x48),
        ),
        (
            "AudioVolumeDown",
            KeyCode::AudioVolumeDown,
            Some((0x002E, true)),
            Some(0x49),
        ),
        (
            "AudioVolumeMute",
            KeyCode::AudioVolumeMute,
            Some((0x0020, true)),
            Some(0x4A),
        ),
        // Function keys, including the F13-F20 band all three tables cover.
        ("F1", KeyCode::F1, Some((0x003B, false)), Some(0x7A)),
        ("F12", KeyCode::F12, Some((0x0058, false)), Some(0x6F)),
        ("F13", KeyCode::F13, Some((0x0064, false)), Some(0x69)),
        ("F14", KeyCode::F14, Some((0x0065, false)), Some(0x6B)),
        ("F15", KeyCode::F15, Some((0x0066, false)), Some(0x71)),
        ("F16", KeyCode::F16, Some((0x0067, false)), Some(0x6A)),
        ("F17", KeyCode::F17, Some((0x0068, false)), Some(0x40)),
        ("F18", KeyCode::F18, Some((0x0069, false)), Some(0x4F)),
        ("F19", KeyCode::F19, Some((0x006A, false)), Some(0x50)),
        ("F20", KeyCode::F20, Some((0x006B, false)), Some(0x5A)),
        // Win32-only families: no AppKit table entry exists for these.
        (
            "ScrollLock",
            KeyCode::ScrollLock,
            Some((0x0046, false)),
            None,
        ),
        ("Pause", KeyCode::Pause, Some((0x0045, false)), None),
        (
            "PrintScreen",
            KeyCode::PrintScreen,
            Some((0x0037, true)),
            None,
        ),
        ("Power", KeyCode::Power, Some((0x005E, true)), None),
        ("KanaMode", KeyCode::KanaMode, Some((0x0070, false)), None),
        ("Convert", KeyCode::Convert, Some((0x0079, false)), None),
        (
            "NonConvert",
            KeyCode::NonConvert,
            Some((0x007B, false)),
            None,
        ),
        ("F21", KeyCode::F21, Some((0x006C, false)), None),
        ("F24", KeyCode::F24, Some((0x0076, false)), None),
        (
            "BrowserBack",
            KeyCode::BrowserBack,
            Some((0x006A, true)),
            None,
        ),
        (
            "BrowserForward",
            KeyCode::BrowserForward,
            Some((0x0069, true)),
            None,
        ),
        (
            "BrowserRefresh",
            KeyCode::BrowserRefresh,
            Some((0x0067, true)),
            None,
        ),
        (
            "BrowserSearch",
            KeyCode::BrowserSearch,
            Some((0x0065, true)),
            None,
        ),
        (
            "BrowserFavorites",
            KeyCode::BrowserFavorites,
            Some((0x0066, true)),
            None,
        ),
        (
            "BrowserStop",
            KeyCode::BrowserStop,
            Some((0x0068, true)),
            None,
        ),
        (
            "BrowserHome",
            KeyCode::BrowserHome,
            Some((0x0032, true)),
            None,
        ),
        (
            "MediaPlayPause",
            KeyCode::MediaPlayPause,
            Some((0x0022, true)),
            None,
        ),
        ("MediaStop", KeyCode::MediaStop, Some((0x0024, true)), None),
        (
            "MediaTrackNext",
            KeyCode::MediaTrackNext,
            Some((0x0019, true)),
            None,
        ),
        (
            "MediaTrackPrevious",
            KeyCode::MediaTrackPrevious,
            Some((0x0010, true)),
            None,
        ),
        (
            "MediaSelect",
            KeyCode::MediaSelect,
            Some((0x006D, true)),
            None,
        ),
        (
            "LaunchApp1",
            KeyCode::LaunchApp1,
            Some((0x006B, true)),
            None,
        ),
        (
            "LaunchApp2",
            KeyCode::LaunchApp2,
            Some((0x0021, true)),
            None,
        ),
        (
            "LaunchMail",
            KeyCode::LaunchMail,
            Some((0x006C, true)),
            None,
        ),
    ];

    #[test]
    fn winit_win32_and_appkit_agree_on_shared_physical_keys() {
        let mut disagreements = Vec::new();
        for &(label, winit_code, win32, appkit) in PHYSICAL_KEYS {
            let winit_result = from_winit_code(PhysicalKey::Code(winit_code));
            if let Some((scancode, extended)) = win32 {
                let win32_result = scancode_to_code(scancode, extended);
                if win32_result != winit_result {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} win32={win32_result:?}"
                    ));
                }
            }
            if let Some(keycode) = appkit {
                let appkit_result = keycode_to_code(keycode);
                if appkit_result != winit_result {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} appkit={appkit_result:?}"
                    ));
                }
            }
        }
        assert!(
            disagreements.is_empty(),
            "{} cross-backend disagreement(s):\n{}",
            disagreements.len(),
            disagreements.join("\n")
        );
    }

    /// Every code named in [`PHYSICAL_KEYS`] must itself be a real,
    /// non-`Unidentified` `Code` — guards against a copy/paste `None` that
    /// silently turns a real disagreement into a skipped row.
    #[test]
    fn physical_keys_table_never_expects_unidentified() {
        for &(label, winit_code, _, _) in PHYSICAL_KEYS {
            assert_ne!(
                from_winit_code(PhysicalKey::Code(winit_code)),
                Code::Unidentified,
                "{label}"
            );
        }
    }
}
