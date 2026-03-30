//! DOM event → PlatformInput mapping
//!
//! Registers DOM event listeners on the canvas and converts browser events
//! to FLUI's W3C-based PlatformInput types.

use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{
    shared::WindowCallbacks,
    traits::PlatformInput,
};

use super::window::WebWindow;

/// Register all DOM event listeners on the canvas
pub fn register_event_listeners(window: &WebWindow) {
    let canvas = window.canvas();
    let callbacks = Arc::clone(window.callbacks());

    register_pointer_events(canvas, &callbacks, window);
    register_keyboard_events(&callbacks, window);
    register_focus_events(canvas, window);
    register_wheel_events(canvas, &callbacks, window);
    register_context_menu_block(canvas, window);
}

// ==================== Pointer Events ====================

fn register_pointer_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    window: &WebWindow,
) {
    // pointerdown
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let input = convert_pointer_down(&pe);
            callbacks.dispatch_input(input);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // pointermove
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let input = convert_pointer_move(&pe);
            callbacks.dispatch_input(input);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // pointerup
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let input = convert_pointer_up(&pe);
            callbacks.dispatch_input(input);
        });
        let _ =
            canvas.add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }
}

// ==================== Keyboard Events ====================

fn register_keyboard_events(callbacks: &Arc<WindowCallbacks>, window: &WebWindow) {
    let browser_window = web_sys::window().expect("no global window");

    // keydown
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let ke: web_sys::KeyboardEvent = e.unchecked_into();
            let input = convert_keyboard_event(&ke, keyboard_types::KeyState::Down);
            callbacks.dispatch_input(input);
        });
        let _ = browser_window
            .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // keyup
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let ke: web_sys::KeyboardEvent = e.unchecked_into();
            let input = convert_keyboard_event(&ke, keyboard_types::KeyState::Up);
            callbacks.dispatch_input(input);
        });
        let _ = browser_window
            .add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }
}

// ==================== Focus Events ====================

fn register_focus_events(canvas: &web_sys::HtmlCanvasElement, window: &WebWindow) {
    // focus
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(true);
        });
        let _ = canvas.add_event_listener_with_callback("focus", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // blur
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(false);
        });
        let _ = canvas.add_event_listener_with_callback("blur", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // pointerenter → hover
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_hover_status_change(true);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerenter", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }

    // pointerleave → hover
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_hover_status_change(false);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerleave", closure.as_ref().unchecked_ref());
        window.keep_closure(closure);
    }
}

// ==================== Wheel Events ====================

fn register_wheel_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    window: &WebWindow,
) {
    let callbacks = Arc::clone(callbacks);
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
        let we: web_sys::WheelEvent = e.unchecked_into();
        let input = convert_wheel_event(&we);
        callbacks.dispatch_input(input);
    });

    let mut options = web_sys::AddEventListenerOptions::new();
    options.passive(false); // Non-passive to allow preventDefault
    let _ = canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        closure.as_ref().unchecked_ref(),
        &options,
    );
    window.keep_closure(closure);
}

// ==================== Context Menu Block ====================

fn register_context_menu_block(canvas: &web_sys::HtmlCanvasElement, window: &WebWindow) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
    });
    let _ =
        canvas.add_event_listener_with_callback("contextmenu", closure.as_ref().unchecked_ref());
    window.keep_closure(closure);
}

// ==================== Event Conversion ====================

fn make_pointer_info(pe: &web_sys::PointerEvent) -> ui_events::pointer::PointerInfo {
    use ui_events::pointer::{PointerId, PointerInfo, PointerType};

    let pointer_type = match pe.pointer_type().as_str() {
        "mouse" => PointerType::Mouse,
        "pen" => PointerType::Pen,
        "touch" => PointerType::Touch,
        _ => PointerType::Unknown,
    };

    PointerInfo {
        pointer_id: PointerId::new(pe.pointer_id() as u64),
        persistent_device_id: None,
        pointer_type,
    }
}

fn make_pointer_state(pe: &web_sys::PointerEvent) -> ui_events::pointer::PointerState {
    use dpi::{PhysicalPosition, PhysicalSize};
    use ui_events::pointer::{PointerButtons, PointerOrientation, PointerState};

    let modifiers = extract_modifiers_from_mouse(pe);

    PointerState {
        time: 0, // Browser doesn't expose nanosecond timestamps easily
        position: PhysicalPosition::new(pe.offset_x() as f64, pe.offset_y() as f64),
        buttons: PointerButtons::new(pe.buttons() as u32),
        modifiers,
        count: 0,
        contact_geometry: PhysicalSize::new(
            pe.width().max(1) as f64,
            pe.height().max(1) as f64,
        ),
        orientation: PointerOrientation::default(),
        pressure: pe.pressure(),
        tangential_pressure: pe.tangential_pressure(),
        scale_factor: web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0),
    }
}

fn map_button(button: i16) -> Option<ui_events::pointer::PointerButton> {
    use ui_events::pointer::PointerButton;

    match button {
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Auxiliary),
        2 => Some(PointerButton::Secondary),
        3 => Some(PointerButton::X1),
        4 => Some(PointerButton::X2),
        _ => None,
    }
}

fn convert_pointer_down(pe: &web_sys::PointerEvent) -> PlatformInput {
    use ui_events::pointer::{PointerButtonEvent, PointerEvent};

    PlatformInput::Pointer(PointerEvent::Down(PointerButtonEvent {
        button: map_button(pe.button()),
        pointer: make_pointer_info(pe),
        state: make_pointer_state(pe),
    }))
}

fn convert_pointer_up(pe: &web_sys::PointerEvent) -> PlatformInput {
    use ui_events::pointer::{PointerButtonEvent, PointerEvent};

    PlatformInput::Pointer(PointerEvent::Up(PointerButtonEvent {
        button: map_button(pe.button()),
        pointer: make_pointer_info(pe),
        state: make_pointer_state(pe),
    }))
}

fn convert_pointer_move(pe: &web_sys::PointerEvent) -> PlatformInput {
    use ui_events::pointer::{PointerEvent, PointerUpdate};

    PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
        pointer: make_pointer_info(pe),
        current: make_pointer_state(pe),
        coalesced: Vec::new(),
        predicted: Vec::new(),
    }))
}

fn convert_wheel_event(we: &web_sys::WheelEvent) -> PlatformInput {
    use dpi::PhysicalPosition;
    use ui_events::pointer::{PointerEvent, PointerInfo, PointerScrollEvent, PointerState, PointerType};
    use ui_events::ScrollDelta;

    let delta = match we.delta_mode() {
        // DOM_DELTA_PIXEL = 0
        0 => ScrollDelta::PixelDelta(PhysicalPosition::new(
            we.delta_x(),
            we.delta_y(),
        )),
        // DOM_DELTA_LINE = 1
        1 => ScrollDelta::LineDelta(we.delta_x() as f32, we.delta_y() as f32),
        // DOM_DELTA_PAGE = 2
        2 => ScrollDelta::PageDelta(we.delta_x() as f32, we.delta_y() as f32),
        _ => ScrollDelta::PixelDelta(PhysicalPosition::new(we.delta_x(), we.delta_y())),
    };

    let modifiers = extract_modifiers_from_mouse(we);

    PlatformInput::Pointer(PointerEvent::Scroll(PointerScrollEvent {
        pointer: PointerInfo {
            pointer_id: None,
            persistent_device_id: None,
            pointer_type: PointerType::Mouse,
        },
        delta,
        state: PointerState {
            time: 0,
            position: PhysicalPosition::new(we.offset_x() as f64, we.offset_y() as f64),
            buttons: Default::default(),
            modifiers,
            count: 0,
            contact_geometry: dpi::PhysicalSize::new(1.0, 1.0),
            orientation: Default::default(),
            pressure: 0.0,
            tangential_pressure: 0.0,
            scale_factor: web_sys::window()
                .map(|w| w.device_pixel_ratio())
                .unwrap_or(1.0),
        },
    }))
}

fn convert_keyboard_event(
    ke: &web_sys::KeyboardEvent,
    state: keyboard_types::KeyState,
) -> PlatformInput {
    use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};

    let mut modifiers = Modifiers::empty();
    if ke.shift_key() {
        modifiers |= Modifiers::SHIFT;
    }
    if ke.ctrl_key() {
        modifiers |= Modifiers::CONTROL;
    }
    if ke.alt_key() {
        modifiers |= Modifiers::ALT;
    }
    if ke.meta_key() {
        modifiers |= Modifiers::META;
    }

    let key = map_key_value(&ke.key());

    let location = match ke.location() {
        0 => Location::Standard,
        1 => Location::Left,
        2 => Location::Right,
        3 => Location::Numpad,
        _ => Location::Standard,
    };

    PlatformInput::Keyboard(ui_events::keyboard::KeyboardEvent {
        state,
        key,
        code: Code::Unidentified,
        location,
        modifiers,
        repeat: ke.repeat(),
        is_composing: ke.is_composing(),
    })
}

// ==================== Helpers ====================

fn extract_modifiers_from_mouse(e: &web_sys::MouseEvent) -> keyboard_types::Modifiers {
    let mut modifiers = keyboard_types::Modifiers::empty();
    if e.shift_key() {
        modifiers |= keyboard_types::Modifiers::SHIFT;
    }
    if e.ctrl_key() {
        modifiers |= keyboard_types::Modifiers::CONTROL;
    }
    if e.alt_key() {
        modifiers |= keyboard_types::Modifiers::ALT;
    }
    if e.meta_key() {
        modifiers |= keyboard_types::Modifiers::META;
    }
    modifiers
}

/// Map DOM `KeyboardEvent.key` to `keyboard_types::Key`
fn map_key_value(key: &str) -> keyboard_types::Key {
    use keyboard_types::{Key, NamedKey};

    match key {
        "Enter" => Key::Named(NamedKey::Enter),
        "Tab" => Key::Named(NamedKey::Tab),
        "Backspace" => Key::Named(NamedKey::Backspace),
        "Escape" => Key::Named(NamedKey::Escape),
        "ArrowUp" => Key::Named(NamedKey::ArrowUp),
        "ArrowDown" => Key::Named(NamedKey::ArrowDown),
        "ArrowLeft" => Key::Named(NamedKey::ArrowLeft),
        "ArrowRight" => Key::Named(NamedKey::ArrowRight),
        "Shift" => Key::Named(NamedKey::Shift),
        "Control" => Key::Named(NamedKey::Control),
        "Alt" => Key::Named(NamedKey::Alt),
        "Meta" => Key::Named(NamedKey::Meta),
        "Delete" => Key::Named(NamedKey::Delete),
        "Insert" => Key::Named(NamedKey::Insert),
        "Home" => Key::Named(NamedKey::Home),
        "End" => Key::Named(NamedKey::End),
        "PageUp" => Key::Named(NamedKey::PageUp),
        "PageDown" => Key::Named(NamedKey::PageDown),
        " " => Key::Named(NamedKey::Space),
        "F1" => Key::Named(NamedKey::F1),
        "F2" => Key::Named(NamedKey::F2),
        "F3" => Key::Named(NamedKey::F3),
        "F4" => Key::Named(NamedKey::F4),
        "F5" => Key::Named(NamedKey::F5),
        "F6" => Key::Named(NamedKey::F6),
        "F7" => Key::Named(NamedKey::F7),
        "F8" => Key::Named(NamedKey::F8),
        "F9" => Key::Named(NamedKey::F9),
        "F10" => Key::Named(NamedKey::F10),
        "F11" => Key::Named(NamedKey::F11),
        "F12" => Key::Named(NamedKey::F12),
        "CapsLock" => Key::Named(NamedKey::CapsLock),
        "NumLock" => Key::Named(NamedKey::NumLock),
        "ScrollLock" => Key::Named(NamedKey::ScrollLock),
        s if s.len() == 1 => Key::Character(s.into()),
        _ => Key::Unidentified,
    }
}
