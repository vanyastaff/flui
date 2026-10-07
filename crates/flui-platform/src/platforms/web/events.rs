//! DOM event → PlatformInput mapping
//!
//! Registers DOM event listeners on the canvas and converts browser events
//! into FLUI's owned PlatformInput types. Keyboard and wheel translations
//! still use private W3C transport helpers.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use flui_foundation::geometry::{Point, Size};
use flui_platform_api::{
    EventTime,
    pointer::{
        ButtonChange, CancelReason, ContactSize, PenOrientation, PenTool, PointerButton,
        PointerButtons, PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind,
        PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample,
        Pressure, TangentialPressure, Twist,
    },
};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::shared::input_vocabulary::{keyboard_input, pointer_input};
use crate::{shared::WindowCallbacks, traits::PlatformInput};

use super::window::WebWindow;

/// Register all DOM event listeners on the canvas.
///
/// Closures are intentionally leaked via `forget()` — they must live for the
/// lifetime of the page because DOM event listeners hold references to them.
pub fn register_event_listeners(window: &WebWindow) {
    let canvas = window.canvas();
    let callbacks = Arc::clone(window.callbacks());

    register_pointer_events(canvas, &callbacks);
    register_keyboard_events(&callbacks);
    register_focus_events(canvas, &callbacks);
    register_wheel_events(canvas, &callbacks);
    register_context_menu_block(canvas);
    register_layout_events(window);
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
fn register_layout_events(window: &WebWindow) {
    let sync = Arc::new(window.layout_sync());

    {
        let sync = Arc::clone(&sync);
        // The observer passes (entries, observer); neither is needed — the
        // sync re-reads the live box — so the closure takes no arguments,
        // which JavaScript permits.
        let closure = Closure::<dyn FnMut()>::new(move || sync.sync());
        match web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref()) {
            Ok(observer) => {
                observer.observe(window.canvas());
                // The observer lives as long as the page, like every other
                // listener here; dropping the handle does not disconnect it.
                std::mem::forget(observer);
                closure.forget();
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "ResizeObserver unavailable; canvas size follows the window's resize event only"
                );
            }
        }
    }

    if let Some(browser_window) = web_sys::window() {
        let closure =
            Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| sync.sync());
        let _ = browser_window
            .add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

// ==================== Pointer Events ====================

fn register_pointer_events(canvas: &web_sys::HtmlCanvasElement, callbacks: &Arc<WindowCallbacks>) {
    let active = Rc::new(RefCell::new(HashMap::new()));
    // DOM getters and application callbacks can synchronously dispatch another
    // pointer event. Listener captures are immutable; mutable state is scoped
    // inside the active-pointer cell instead of the JavaScript closure.
    // pointerdown
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let capture = canvas.clone();
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            let buttons = pe.buttons();
            active.borrow_mut().insert(id, buttons);
            // Admission precedes application callbacks, which can remove the
            // canvas or synchronously dispatch another terminal event.
            if let Err(error) = capture.set_pointer_capture(id) {
                tracing::debug!(?error, "browser pointer capture was not admitted");
            }
            let input = convert_pointer_down(&pe);
            if let Some(input) = input {
                callbacks.dispatch_input(input);
            }
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // pointermove
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            let buttons = pe.buttons();
            let previous = active
                .borrow_mut()
                .get_mut(&id)
                .map(|held| std::mem::replace(held, buttons));
            let input = convert_pointer_move(&pe, previous);
            if let Some(input) = input {
                callbacks.dispatch_input(input);
            }
        });
        let _ = canvas
            .add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // pointerup
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let capture = canvas.clone();
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            let was_active = active.borrow_mut().remove(&id).is_some();
            if !was_active {
                return;
            }
            release_pointer_capture(&capture, id);
            let input = convert_pointer_up(&pe);
            if let Some(input) = input {
                callbacks.dispatch_input(input);
            }
        });
        let _ =
            canvas.add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // A browser gesture takeover is a terminal event even when no release
    // will follow. Deliver it through the same public input callback.
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let capture = canvas.clone();
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            let was_active = active.borrow_mut().remove(&id).is_some();
            if !was_active {
                return;
            }
            release_pointer_capture(&capture, id);
            if let Some(input) = convert_pointer_cancel(&pe, CancelReason::Platform) {
                callbacks.dispatch_input(input);
            }
        });
        let _ = canvas
            .add_event_listener_with_callback("pointercancel", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            // Up/cancel removes admission before releasing capture. The loss
            // notification following that terminal edge must stay inert.
            let was_active = active.borrow_mut().remove(&id).is_some();
            if was_active {
                if let Some(input) = convert_pointer_cancel(&pe, CancelReason::CaptureLost) {
                    callbacks.dispatch_input(input);
                }
            }
        });
        let _ = canvas.add_event_listener_with_callback(
            "lostpointercapture",
            closure.as_ref().unchecked_ref(),
        );
        closure.forget();
    }
}

fn release_pointer_capture(canvas: &web_sys::HtmlCanvasElement, pointer: i32) {
    if canvas.has_pointer_capture(pointer)
        && let Err(error) = canvas.release_pointer_capture(pointer)
    {
        tracing::debug!(?error, "browser pointer capture release failed");
    }
}

// ==================== Keyboard Events ====================

fn register_keyboard_events(callbacks: &Arc<WindowCallbacks>) {
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
        closure.forget();
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
        closure.forget();
    }
}

// ==================== Focus Events ====================

fn register_focus_events(canvas: &web_sys::HtmlCanvasElement, callbacks: &Arc<WindowCallbacks>) {
    // focus
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(true);
        });
        let _ = canvas.add_event_listener_with_callback("focus", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // blur
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(false);
        });
        let _ = canvas.add_event_listener_with_callback("blur", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // pointerenter → hover
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_hover_status_change(true);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerenter", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // pointerleave → hover
    {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_hover_status_change(false);
        });
        let _ = canvas
            .add_event_listener_with_callback("pointerleave", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

// ==================== Wheel Events ====================

fn register_wheel_events(canvas: &web_sys::HtmlCanvasElement, callbacks: &Arc<WindowCallbacks>) {
    let callbacks = Arc::clone(callbacks);
    let target = canvas.clone();
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
        let we: web_sys::WheelEvent = e.unchecked_into();
        let input = convert_wheel_event(&we, &target);
        if let Some(input) = input {
            callbacks.dispatch_input(input);
        }
    });

    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false); // Non-passive to allow preventDefault
    let _ = canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        closure.as_ref().unchecked_ref(),
        &options,
    );
    closure.forget();
}

// ==================== Context Menu Block ====================

fn register_context_menu_block(canvas: &web_sys::HtmlCanvasElement) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
    });
    let _ =
        canvas.add_event_listener_with_callback("contextmenu", closure.as_ref().unchecked_ref());
    closure.forget();
}

// ==================== Event Conversion ====================

// CSSOM View defines these offsets as doubles. web-sys exposes the older
// integer signature, which discards subpixel positions before Rust receives
// them. Bind the actual DOM getters with their floating-point result type.
#[wasm_bindgen]
extern "C" {
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
    #[wasm_bindgen(method, catch, js_name = getCoalescedEvents)]
    fn coalesced_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getPredictedEvents)]
    fn predicted_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
}

fn pointer_position(event: &web_sys::MouseEvent) -> dpi::PhysicalPosition<f64> {
    let event: &PreciseMouseEvent = event.unchecked_ref();
    dpi::PhysicalPosition::new(event.offset_x_f64(), event.offset_y_f64())
}

/// MouseEvent implementations can round wheel offsets even when the canvas
/// origin is fractional. Recover its padding-edge position from viewport
/// coordinates only when the whole ancestry has no coordinate transform.
/// A bounding rectangle cannot invert a rotation, skew or perspective;
/// those shapes keep the browser's own local offset instead of guessing.
fn wheel_position(
    event: &web_sys::WheelEvent,
    canvas: &web_sys::HtmlCanvasElement,
) -> dpi::PhysicalPosition<f64> {
    untransformed_wheel_position(event, canvas).unwrap_or_else(|| pointer_position(event))
}

fn untransformed_wheel_position(
    event: &web_sys::WheelEvent,
    canvas: &web_sys::HtmlCanvasElement,
) -> Option<dpi::PhysicalPosition<f64>> {
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
    let source: &PreciseMouseEvent = event.unchecked_ref();
    Some(dpi::PhysicalPosition::new(
        source.client_x_f64() - rect.left() - border_left,
        source.client_y_f64() - rect.top() - border_top,
    ))
}

fn make_pointer_info(pe: &web_sys::PointerEvent) -> Option<PointerInfo> {
    let id = PointerId::try_from(u64::try_from(pe.pointer_id()).ok()?).ok()?;
    let kind = match pe.pointer_type().as_str() {
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
    };
    let role = if pe.is_primary() {
        PointerRole::Primary
    } else {
        PointerRole::Additional
    };
    Some(PointerInfo::new(id, kind).with_role(role))
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
fn upstream_buttons_from_mask(mask: u16) -> ui_events::pointer::PointerButtons {
    use ui_events::pointer::{PointerButton, PointerButtons};

    let mut buttons = PointerButtons::default();
    if mask & 0x01 != 0 {
        buttons.insert(PointerButton::Primary);
    }
    if mask & 0x02 != 0 {
        buttons.insert(PointerButton::Secondary);
    }
    if mask & 0x04 != 0 {
        buttons.insert(PointerButton::Auxiliary);
    }
    if mask & 0x08 != 0 {
        buttons.insert(PointerButton::X1);
    }
    if mask & 0x10 != 0 {
        buttons.insert(PointerButton::X2);
    }
    buttons
}

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

fn pointer_time(pe: &web_sys::PointerEvent) -> EventTime {
    EventTime::from_nanos((pe.time_stamp() * 1_000_000.0) as u64)
}

fn pointer_sample(pe: &web_sys::PointerEvent) -> Option<PointerSample> {
    let point = pointer_position(pe);
    let position = PointerPosition::try_new(Point::new(point.x, point.y)).ok()?;
    let mut sample = PointerSample::new(pointer_time(pe), position);
    // DOM has no per-device sensor capability query. Keep its reported touch
    // and pen pressure, including zero, without treating a mouse fallback as force.
    if matches!(pe.pointer_type().as_str(), "touch" | "pen") {
        sample.pressure = Pressure::saturating(pe.pressure()).ok();
        let extended: &ExtendedPointerEvent = pe.unchecked_ref();
        sample.contact_size =
            ContactSize::try_new(Size::new(extended.width_f64(), extended.height_f64())).ok();
        if pe.pointer_type() == "pen" {
            sample.tangential_pressure =
                TangentialPressure::saturating(pe.tangential_pressure()).ok();
            let altitude = extended
                .altitude_angle()
                .ok()
                .and_then(|value| PenOrientation::try_altitude(value).ok());
            let azimuth = extended
                .azimuth_angle()
                .ok()
                .and_then(|value| PenOrientation::try_azimuth(value).ok());
            sample.orientation = match (altitude, azimuth) {
                (Some(altitude), Some(azimuth)) => altitude
                    .altitude()
                    .zip(azimuth.azimuth())
                    .and_then(|(altitude, azimuth)| {
                        PenOrientation::try_new(altitude, azimuth).ok()
                    }),
                (altitude, azimuth) => altitude.or(azimuth),
            };
            sample.twist = extended
                .twist_degrees()
                .ok()
                .and_then(|degrees| Twist::try_new(degrees.to_radians()).ok());
        }
    }
    Some(sample)
}

fn pointer_modifiers(pe: &web_sys::PointerEvent) -> flui_platform_api::keyboard::Modifiers {
    crate::shared::input_vocabulary::modifiers(extract_modifiers_from_mouse(pe))
}

fn convert_pointer_down(pe: &web_sys::PointerEvent) -> Option<PlatformInput> {
    let pointer = make_pointer_info(pe)?;
    let button = map_button(pe.button())?;
    let held = buttons_from_mask(pe.buttons());
    let press = PointerPress::new(pointer, button, held, pointer_sample(pe)?)
        .with_modifiers(pointer_modifiers(pe));
    Some(PlatformInput::Pointer(if held.without(button).is_empty() {
        PointerEvent::Down(press)
    } else {
        PointerEvent::ButtonChange(ButtonChange::Pressed(press))
    }))
}

fn convert_pointer_up(pe: &web_sys::PointerEvent) -> Option<PlatformInput> {
    let pointer = make_pointer_info(pe)?;
    let Some(sample) = pointer_sample(pe) else {
        return convert_pointer_cancel(pe, CancelReason::InvalidInput);
    };
    let button = map_button(pe.button())?;
    let held = buttons_from_mask(pe.buttons());
    let release =
        PointerRelease::new(pointer, button, held, sample).with_modifiers(pointer_modifiers(pe));
    Some(PlatformInput::Pointer(if held.without(button).is_empty() {
        PointerEvent::Up(release)
    } else {
        PointerEvent::ButtonChange(ButtonChange::Released(release))
    }))
}

fn convert_pointer_cancel(
    pe: &web_sys::PointerEvent,
    reason: CancelReason,
) -> Option<PlatformInput> {
    Some(PlatformInput::Pointer(PointerEvent::Cancel(
        PointerCancel::new(make_pointer_info(pe)?, pointer_time(pe), reason),
    )))
}

fn pointer_samples(
    pe: &web_sys::PointerEvent,
    values: Result<js_sys::Array, JsValue>,
) -> Vec<PointerSample> {
    let Ok(values) = values else {
        return Vec::new();
    };
    let kind = pe.pointer_type();
    values
        .iter()
        .filter_map(|value| value.dyn_into::<web_sys::PointerEvent>().ok())
        .filter(|sample| sample.pointer_id() == pe.pointer_id() && sample.pointer_type() == kind)
        .filter_map(|sample| pointer_sample(&sample))
        .collect()
}

fn convert_pointer_move(
    pe: &web_sys::PointerEvent,
    previous: Option<u16>,
) -> Option<PlatformInput> {
    let pointer = make_pointer_info(pe)?;
    let sample = pointer_sample(pe)?;
    let held = buttons_from_mask(pe.buttons());
    let modifiers = pointer_modifiers(pe);
    if let Some(previous) = previous
        && let Some(button) = map_button(pe.button())
        && buttons_from_mask(previous).contains(button) != held.contains(button)
    {
        let change = if held.contains(button) {
            ButtonChange::Pressed(
                PointerPress::new(pointer, button, held, sample).with_modifiers(modifiers),
            )
        } else {
            ButtonChange::Released(
                PointerRelease::new(pointer, button, held, sample).with_modifiers(modifiers),
            )
        };
        return Some(PlatformInput::Pointer(PointerEvent::ButtonChange(change)));
    }
    let extended: &ExtendedPointerEvent = pe.unchecked_ref();
    let event = PointerMove::new(pointer, held, sample)
        .with_modifiers(modifiers)
        .with_coalesced(pointer_samples(pe, extended.coalesced_events()))
        .with_predicted(pointer_samples(pe, extended.predicted_events()));
    Some(PlatformInput::Pointer(PointerEvent::Move(event)))
}

/// Convert a DOM `WheelEvent` to a W3C `PointerEvent::Scroll`.
///
/// The DOM already speaks the cross-backend wheel convention (positive =
/// content scrolls down/right; `DOM_DELTA_PIXEL` deltas are CSS = logical
/// pixels), so `from_web` is a passthrough that only maps `deltaMode` onto
/// the `ScrollDelta` variants — kept in `crate::shared::scroll` so this
/// boundary appears in the same sign/unit table (and executing contract
/// tests) as the backends that DO have to flip or rescale.
fn convert_wheel_event(
    we: &web_sys::WheelEvent,
    canvas: &web_sys::HtmlCanvasElement,
) -> Option<PlatformInput> {
    use ui_events::pointer::{
        PointerEvent, PointerInfo, PointerOrientation, PointerScrollEvent, PointerState,
        PointerType,
    };

    let delta = crate::shared::scroll::from_web(we.delta_mode(), we.delta_x(), we.delta_y());

    let modifiers = extract_modifiers_from_mouse(we);

    pointer_input(
        PointerEvent::Scroll(PointerScrollEvent {
            pointer: PointerInfo {
                pointer_id: None,
                persistent_device_id: None,
                pointer_type: PointerType::Mouse,
            },
            delta,
            state: PointerState {
                time: (we.time_stamp() * 1_000_000.0) as u64,
                position: wheel_position(we, canvas),
                buttons: upstream_buttons_from_mask(we.buttons()),
                modifiers,
                count: 0,
                contact_geometry: dpi::PhysicalSize::new(1.0, 1.0),
                orientation: PointerOrientation::default(),
                pressure: 0.0,
                tangential_pressure: 0.0,
                scale_factor: web_sys::window().map_or(1.0, |w| w.device_pixel_ratio()),
            },
        }),
        (we.time_stamp() * 1_000_000.0) as u64,
    )
}

fn convert_keyboard_event(
    ke: &web_sys::KeyboardEvent,
    state: keyboard_types::KeyState,
) -> PlatformInput {
    let mut modifiers = keyboard_types::Modifiers::empty();
    if ke.shift_key() {
        modifiers |= keyboard_types::Modifiers::SHIFT;
    }
    if ke.ctrl_key() {
        modifiers |= keyboard_types::Modifiers::CONTROL;
    }
    if ke.alt_key() {
        modifiers |= keyboard_types::Modifiers::ALT;
    }
    if ke.meta_key() {
        modifiers |= keyboard_types::Modifiers::META;
    }

    let key = map_key_value(&ke.key());

    let location = match ke.location() {
        1 => keyboard_types::Location::Left,
        2 => keyboard_types::Location::Right,
        3 => keyboard_types::Location::Numpad,
        // 0 is DOM_KEY_LOCATION_STANDARD; an unrecognised value is treated
        // the same way rather than dropped.
        _ => keyboard_types::Location::Standard,
    };

    keyboard_input(
        ui_events::keyboard::KeyboardEvent {
            state,
            key,
            code: ke
                .code()
                .parse()
                .unwrap_or(keyboard_types::Code::Unidentified),
            location,
            modifiers,
            repeat: ke.repeat(),
            is_composing: ke.is_composing(),
        },
        (ke.time_stamp() * 1_000_000.0) as u64,
    )
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
        " " => Key::Character(" ".into()),
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
        // Any string that is a single character (including multi-byte Unicode like Cyrillic)
        s if s.chars().count() == 1 => Key::Character(s.into()),
        // Multi-char strings that aren't named keys (e.g. "Dead", "Unidentified")
        _ => Key::Named(NamedKey::Unidentified),
    }
}
