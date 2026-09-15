//! Winit event conversion to W3C ui-events
//!
//! Converts winit 0.30 events to W3C-compliant PlatformInput types.

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::pointer::{
    PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
    PointerOrientation, PointerState, PointerType, PointerUpdate,
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

/// Convert a winit `KeyboardInput` (event + live modifiers state) to a W3C
/// `KeyboardEvent`, wrapped as a `PlatformInput`.
///
/// The whole event delegates to
/// `ui_events_winit::keyboard::from_winit_keyboard_event` (the same
/// ecosystem bridge Masonry and Xilem use) — code, logical key, location,
/// down/up state, `repeat`, modifiers, and `is_composing: false` (FLUI has
/// no other source for IME-composition state on this path, so this is the
/// same constant the crate itself hard-codes, not a FLUI-specific value
/// lost by delegating). Nothing here hand-assembles a `KeyboardEvent` field
/// any more. `Code::Unidentified` therefore means exactly one thing: winit
/// itself reported `PhysicalKey::Unidentified`, i.e. the OS/backend could
/// not name the physical key — never an incomplete conversion table. See
/// `crates/flui-platform/ARCHITECTURE.md` §Mapping decisions for why the
/// Win32 and AppKit backends keep their own hand-written `Code`/`Key`
/// tables instead of also delegating here, and this module's
/// `keyboard_tests::cross_backend_physical_key_agreement` for what keeps
/// the three in agreement.
pub fn keyboard_event(
    event: winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> PlatformInput {
    PlatformInput::Keyboard(ui_events_winit::keyboard::from_winit_keyboard_event(
        event, modifiers,
    ))
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

// The winit-vs-ui-events-winit keyboard conversion tests live in their own
// file: two cohesive families (completeness of the delegated conversion,
// and cross-backend agreement with the Win32/AppKit hand-written tables)
// that pushed this file past a size where production code sat in its first
// quarter (issue #1092). A sibling module rather than a nested one, so its
// tests sit beside `pointer_translation_tests`/`ime_tests` rather than two
// segments below them — same shape as `platform.rs`'s `real_loop_tests`
// (issue #923).
#[cfg(test)]
#[path = "events/keyboard_tests.rs"]
mod keyboard_tests;
