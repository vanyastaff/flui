//! DOM event → PlatformInput mapping
//!
//! Registers DOM event listeners on the canvas and converts browser events
//! into FLUI's owned PlatformInput types.

use std::{rc::Rc, sync::Arc};

use flui_foundation::geometry::{Point, Size};
use flui_platform_api::{
    EventTime,
    keyboard::{
        Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
    },
    pointer::{
        ButtonChange, CancelReason, ContactSize, PenOrientation, PenTool, PointerButton,
        PointerButtons, PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind,
        PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample,
        Pressure, ScrollEvent, TangentialPressure, Twist,
    },
};
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
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let canvas = window.canvas();
    let callbacks = Arc::clone(window.callbacks());

    register_pointer_events(canvas, &callbacks, registration)?;
    register_keyboard_events(canvas, &callbacks, registration)?;
    let focus = register_focus_events(window, registration)?;
    let visibility = Rc::new(window.visibility_sync(Rc::clone(&registration.accepting)));
    let observer = Rc::clone(&visibility);
    registration.listen(
        visibility.document.as_ref(),
        "visibilitychange",
        move |_| {
            observer.sync();
        },
    )?;
    register_wheel_events(canvas, &callbacks, registration)?;
    register_context_menu_block(canvas, registration)?;
    register_layout_events(window, registration)?;
    register_appearance_events(&callbacks, registration)?;
    register_document_events(window, registration)?;
    register_graphics_events(window, registration)?;
    // Host registration calls can change native state before its listeners
    // exist. Reconcile after successful installation, before Created.
    focus.sync();
    visibility.sync();
    Ok(())
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

// ==================== Page lifecycle and fullscreen ====================

/// Report what the page does to the window it holds.
///
/// - `freeze` and `resume` (Page Lifecycle): a frozen page runs no script at
///   all, so the window is `Suspended` from the last event it can handle until
///   `resume`. A browser without the API never raises them and the window stays
///   `Running`.
/// - `fullscreenchange`: the browser answers `requestFullscreen`, Escape and
///   the user's own exit with this one event, so the published state is read
///   again here and nowhere else.
fn register_document_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let document = window
        .canvas()
        .owner_document()
        .expect("BUG: window construction verified the canvas document");
    let execution = Rc::new(window.execution_sync());
    for (kind, state) in [
        ("freeze", WindowExecutionState::Suspended),
        ("resume", WindowExecutionState::Running),
    ] {
        let execution = Rc::clone(&execution);
        registration.listen(document.as_ref(), kind, move |_| execution.set(state))?;
    }
    let publisher = window.publisher();
    registration.listen(document.as_ref(), "fullscreenchange", move |_| {
        publisher.publish();
    })
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
fn register_graphics_events(
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
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let active = Rc::clone(&registration.pointers);
    // DOM getters and application callbacks can synchronously dispatch another
    // pointer event. Listener captures are immutable; mutable state is scoped
    // inside the active-pointer cell instead of the JavaScript closure.
    // pointerdown
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let capture = canvas.clone();
        let accepting = Rc::clone(&registration.accepting);
        let custody = registration.capture_owner();
        registration.listen(canvas.as_ref(), "pointerdown", move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let buttons = pe.buttons();
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
            let input = convert_pointer_down(&pe);
            if accepting.get()
                && let Some(input) = input
            {
                callbacks.dispatch_input(input);
            }
        })?;
    }

    // pointermove
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
        let accepting = Rc::clone(&registration.accepting);
        registration.listen(canvas.as_ref(), "pointermove", move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            let id = pe.pointer_id();
            if !accepting.get() {
                return;
            }
            let buttons = pe.buttons();
            if !accepting.get() {
                return;
            }
            let previous = active
                .borrow_mut()
                .get_mut(&id)
                .map(|held| std::mem::replace(held, buttons));
            let input = convert_pointer_move(&pe, previous);
            if accepting.get()
                && let Some(input) = input
            {
                callbacks.dispatch_input(input);
            }
        })?;
    }

    // pointerup
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
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
            let input = convert_pointer_up(&pe);
            if accepting.get()
                && let Some(input) = input
            {
                callbacks.dispatch_input(input);
            }
        })?;
    }

    // A browser gesture takeover is a terminal event even when no release
    // will follow. Deliver it through the same public input callback.
    {
        let callbacks = Arc::clone(callbacks);
        let active = Rc::clone(&active);
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
                let input = convert_pointer_cancel(&pe, CancelReason::Platform);
                if accepting.get()
                    && let Some(input) = input
                {
                    callbacks.dispatch_input(input);
                }
            },
        )?;
    }

    {
        let callbacks = Arc::clone(callbacks);
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
                let input = convert_pointer_cancel(&pe, CancelReason::CaptureLost);
                if accepting.get()
                    && let Some(input) = input
                {
                    callbacks.dispatch_input(input);
                }
            },
        )?;
    }
    Ok(())
}

// ==================== Keyboard Events ====================

fn register_keyboard_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
        let callbacks = Arc::clone(callbacks);
        let target = canvas.clone();
        let accepting = Rc::clone(&registration.accepting);
        registration.listen(canvas.as_ref(), kind, move |event| {
            if !effective_focus(&target) || event.target().as_ref() != Some(target.as_ref()) {
                return;
            }
            if !accepting.get() {
                return;
            }
            let input = convert_keyboard_event(event.unchecked_ref(), state);
            // DOM getters can close this registration before callback admission.
            if !accepting.get() {
                return;
            }
            let result = callbacks.dispatch_input(input);
            if !result.propagate {
                event.stop_propagation();
            }
            if result.default_prevented {
                event.prevent_default();
            }
        })?;
    }
    Ok(())
}

// ==================== Focus Events ====================

fn register_focus_events(
    window: &WebWindow,
    registration: &mut Registration,
) -> Result<Rc<super::window::FocusSync>, crate::OpenWindowError> {
    let focus = Rc::new(window.focus_sync());
    let browser = web_sys::window().expect("BUG: construction verified browser");
    for target in [window.canvas().as_ref(), browser.as_ref()] {
        for kind in ["focus", "blur"] {
            let focus = Rc::clone(&focus);
            registration.listen(target, kind, move |_| focus.sync())?;
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

fn register_wheel_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    let callbacks = Arc::clone(callbacks);
    let target = canvas.clone();
    registration.listen(canvas.as_ref(), "wheel", move |e: web_sys::Event| {
        e.prevent_default();
        let we: web_sys::WheelEvent = e.unchecked_into();
        let input = convert_wheel_event(&we, &target);
        if let Some(input) = input {
            callbacks.dispatch_input(input);
        }
    })
}

// ==================== Context Menu Block ====================

fn register_context_menu_block(
    canvas: &web_sys::HtmlCanvasElement,
    registration: &mut Registration,
) -> Result<(), crate::OpenWindowError> {
    registration.listen(canvas.as_ref(), "contextmenu", move |e: web_sys::Event| {
        e.prevent_default();
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
    #[wasm_bindgen(method, catch, js_name = getCoalescedEvents)]
    fn coalesced_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getPredictedEvents)]
    fn predicted_events(this: &ExtendedPointerEvent) -> Result<js_sys::Array, JsValue>;
}

fn pointer_position(event: &web_sys::MouseEvent) -> Point {
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
    Some(Point::new(
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
    let position = PointerPosition::try_new(point).ok()?;
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
    extract_modifiers_from_mouse(pe)
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
    let delta =
        crate::shared::scroll::from_web(we.delta_mode(), we.delta_x(), we.delta_y()).ok()?;
    let position = PointerPosition::try_new(wheel_position(we, canvas)).ok()?;
    let time = EventTime::from_nanos((we.time_stamp() * 1_000_000.0) as u64);
    let modifiers = extract_modifiers_from_mouse(we);
    // DOM wheel events have no pointer identity. Keep the documented primary
    // mouse fallback rather than guessing a hardware source or scroll phase.
    let pointer = PointerInfo::new(
        PointerId::try_from(1_u64).expect("BUG: the primary mouse identity is nonzero"),
        PointerKind::Mouse,
    )
    .with_role(PointerRole::Primary);
    Some(PlatformInput::Pointer(PointerEvent::Scroll(
        ScrollEvent::new(pointer, time, position, delta).with_modifiers(modifiers),
    )))
}

fn convert_keyboard_event(ke: &web_sys::KeyboardEvent, state: KeyState) -> PlatformInput {
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
        .with_composition(if ke.is_composing() {
            ImeComposition::Active
        } else {
            ImeComposition::Inactive
        }),
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
