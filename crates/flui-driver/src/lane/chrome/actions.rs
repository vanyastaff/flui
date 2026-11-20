//! An OS event as the source list of one WebDriver BiDi `input.performActions` command.
//!
//! Each event is one command of one action, so every pointer move is a single step of zero
//! duration to its destination and every pause is the lane's own wait between commands: Chrome
//! interpolates a move that lasts, which made recordings differ between runs (ADR-0238 §11), and
//! a command of one instant action leaves it nothing to interpolate. Chrome keeps each source's
//! state (a button down, a key held) between commands until `input.releaseActions`, so a press in
//! one command and its release in a later one are one gesture.
//!
//! Positions arrive in the lane's screen device pixels, which for a page are the viewport's
//! device pixels: a move goes out in CSS pixels, the device pixel divided by the device pixel
//! ratio, so it lands on the pixel it names. A wheel's position and distances are whole CSS
//! pixels, which BiDi's scroll action takes; a distance rounds to the nearest.

use flui_protocol::input::{PointerButton, PointerDevice};
use serde_json::{Value, json};

use super::keys;
use crate::coordinates::DevicePoint;
use crate::error::{Error, Result};
use crate::steps::OsEvent;

/// The id of the key source every key and every typed text goes through.
pub const KEYBOARD: &str = "flui-driver.keyboard";

/// The id of the wheel source every scroll goes through.
pub const WHEEL: &str = "flui-driver.wheel";

/// The `actions` of the `input.performActions` command that sends `event` into a page whose device
/// pixel ratio is `device_pixel_ratio`; `None` for an event no command carries (a pause, which the
/// lane waits out, and a composition, which goes through the DevTools input method).
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a ratio that is not a positive number, or a scroll past what a
/// BiDi integer holds; [`Error::NotSupported`] for a key WebDriver names no value for, text with a
/// control character no key types, or a button BiDi has no number for.
pub fn perform_actions(event: &OsEvent, device_pixel_ratio: f64) -> Result<Option<Value>> {
    if !(device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0) {
        return Err(Error::invalid_argument(format!(
            "the page's device pixel ratio {device_pixel_ratio} is not a positive number"
        )));
    }
    let css = |point: DevicePoint| {
        (
            number(f64::from(point.x) / device_pixel_ratio),
            number(f64::from(point.y) / device_pixel_ratio),
        )
    };
    Ok(Some(match event {
        OsEvent::PointerMove {
            pointer,
            device,
            to,
        } => {
            let (x, y) = css(*to);
            pointer_source(
                pointer,
                *device,
                json!({ "type": "pointerMove", "x": x, "y": y, "duration": 0, "origin": "viewport" }),
            )
        }
        OsEvent::PointerDown {
            pointer,
            device,
            button,
        } => pointer_source(
            pointer,
            *device,
            json!({ "type": "pointerDown", "button": button_number(*button)? }),
        ),
        OsEvent::PointerUp {
            pointer,
            device,
            button,
        } => pointer_source(
            pointer,
            *device,
            json!({ "type": "pointerUp", "button": button_number(*button)? }),
        ),
        OsEvent::Scroll {
            at,
            delta_x,
            delta_y,
        } => {
            let whole = |device: i32| whole_css(f64::from(device), device_pixel_ratio);
            json!([{
                "type": "wheel",
                "id": WHEEL,
                "actions": [{
                    "type": "scroll",
                    "x": floor_css(at.x, device_pixel_ratio)?,
                    "y": floor_css(at.y, device_pixel_ratio)?,
                    "deltaX": whole(*delta_x)?,
                    "deltaY": whole(*delta_y)?,
                    "duration": 0,
                    "origin": "viewport",
                }],
            }])
        }
        OsEvent::KeyDown(name) => key_source(&[json!({ "type": "keyDown", "value": key(name)? })]),
        OsEvent::KeyUp(name) => key_source(&[json!({ "type": "keyUp", "value": key(name)? })]),
        OsEvent::Text(text) => {
            let typed = keys::text_keys(text).map_err(|control| {
                Error::not_supported(
                    "os_text_control_character",
                    format!(
                        "no key types the control character U+{:04X}",
                        u32::from(control)
                    ),
                )
            })?;
            let strokes: Vec<Value> = typed
                .iter()
                .flat_map(|value| {
                    [
                        json!({ "type": "keyDown", "value": value }),
                        json!({ "type": "keyUp", "value": value }),
                    ]
                })
                .collect();
            key_source(&strokes)
        }
        _ => return Ok(None),
    }))
}

/// The key value of `name`.
///
/// # Errors
///
/// [`Error::NotSupported`] naming `os_key_<name>` for one WebDriver names no value for.
pub fn key(name: &str) -> Result<String> {
    keys::key_value(name).ok_or_else(|| {
        Error::not_supported(
            format!("os_key_{name}"),
            format!("WebDriver has no key value for {name:?}"),
        )
    })
}

/// The number BiDi gives `button`.
///
/// # Errors
///
/// [`Error::NotSupported`] for a button it numbers none for.
pub fn button_number(button: PointerButton) -> Result<u8> {
    Ok(match button {
        PointerButton::Primary => 0,
        PointerButton::Middle => 1,
        PointerButton::Secondary => 2,
        PointerButton::Back => 3,
        PointerButton::Forward => 4,
        other => {
            return Err(Error::not_supported(
                format!("os_button_{}", other.name()),
                format!("WebDriver BiDi numbers no {} button", other.name()),
            ));
        }
    })
}

/// The BiDi `pointerType` of `device`.
fn pointer_type(device: PointerDevice) -> &'static str {
    match device {
        PointerDevice::Touch => "touch",
        PointerDevice::Pen => "pen",
        _ => "mouse",
    }
}

fn pointer_source(id: &str, device: PointerDevice, action: Value) -> Value {
    json!([{
        "type": "pointer",
        "id": id,
        "parameters": { "pointerType": pointer_type(device) },
        "actions": [action],
    }])
}

fn key_source(actions: &[Value]) -> Value {
    json!([{ "type": "key", "id": KEYBOARD, "actions": actions }])
}

/// `value` as a JSON number, an integer when it is whole: the form a page reads back exactly.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number inside the range an f64 holds exactly"
        )]
        return Value::from(value as i64);
    }
    Value::from(value)
}

/// The whole CSS pixel the device pixel `device` falls in.
fn floor_css(device: i32, ratio: f64) -> Result<i64> {
    whole(f64::from(device) / ratio, f64::floor)
}

/// The whole CSS pixels nearest to `device` device pixels.
fn whole_css(device: f64, ratio: f64) -> Result<i64> {
    whole(device / ratio, f64::round)
}

fn whole(css: f64, to_whole: fn(f64) -> f64) -> Result<i64> {
    let rounded = to_whole(css);
    // BiDi's integers are JavaScript's safe integers.
    if !(rounded.is_finite() && rounded.abs() <= 9_007_199_254_740_991.0) {
        return Err(Error::invalid_argument(format!(
            "{css} CSS pixels is past what a BiDi integer holds"
        )));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked inside JavaScript's safe integers"
    )]
    Ok(rounded as i64)
}
