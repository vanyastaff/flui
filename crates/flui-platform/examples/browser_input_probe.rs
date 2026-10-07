//! Real browser dispatch coverage for the web platform's public seams.
//! Build for wasm32-unknown-unknown and load wasm-bindgen's web output in a
//! fresh page for each exported probe. The host runner supplies actual DOM events.

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::sync::Arc;

    use flui_platform::platforms::web::WebPlatform;
    use flui_platform::{DispatchEventResult, Platform, WindowOptions};
    use flui_platform_api::PlatformInput;
    use flui_platform_api::pointer::{
        ButtonChange, PointerButton, PointerButtons, PointerEvent, PointerSample,
    };
    use wasm_bindgen::prelude::*;

    fn publish(name: &str, value: &str) {
        let document = web_sys::window()
            .expect("browser window")
            .document()
            .expect("document");
        let element = document.get_element_by_id(name).unwrap_or_else(|| {
            let element = document.create_element("pre").expect("result element");
            element.set_id(name);
            document
                .body()
                .expect("body")
                .append_child(&element)
                .expect("append result");
            element
        });
        element.set_text_content(Some(value));
    }

    fn sample_json(sample: &PointerSample) -> String {
        let number =
            |value: Option<f64>| value.map_or_else(|| "null".to_owned(), |value| value.to_string());
        let point = sample.position.get();
        format!(
            r#"{{"time":{},"x":{},"y":{},"pressure":{},"tangential":{},"altitude":{},"azimuth":{},"twist":{},"width":{},"height":{}}}"#,
            sample.time.as_nanos(),
            point.x,
            point.y,
            number(sample.pressure.map(|value| f64::from(value.get()))),
            number(
                sample
                    .tangential_pressure
                    .map(|value| f64::from(value.get()))
            ),
            number(sample.orientation.map(|value| value.altitude())),
            number(sample.orientation.map(|value| value.azimuth())),
            number(sample.twist.map(|value| value.radians())),
            number(sample.contact_size.map(|value| value.get().width)),
            number(sample.contact_size.map(|value| value.get().height)),
        )
    }

    #[wasm_bindgen]
    pub fn input_probe() -> Result<(), JsValue> {
        let platform = WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let window = platform
            .open_window(WindowOptions::default())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        window.on_input(Box::new(|input| {
            if let PlatformInput::Pointer(event) = input {
                if let PointerEvent::Move(event) = &event {
                    let readings = |samples: &[PointerSample]| samples.iter().map(sample_json).collect::<Vec<_>>().join(",");
                    publish("samples", &format!(
                        r#"{{"kind":"{:?}","role":"{:?}","current":{},"coalesced":[{}],"predicted":[{}]}}"#,
                        event.pointer.kind, event.pointer.role, sample_json(event.current()),
                        readings(event.coalesced()), readings(event.predicted()),
                    ));
                }
                if let PointerEvent::Cancel(event) = &event {
                    publish("cancel-reason", &format!("{:?}", event.reason));
                }
                let (kind, position, buttons) = match &event {
                    PointerEvent::Down(event) => {
                        ("down", Some(event.sample.position), event.buttons())
                    }
                    PointerEvent::Up(event) => ("up", Some(event.sample.position), event.buttons()),
                    PointerEvent::ButtonChange(ButtonChange::Pressed(event)) => ("button-press", Some(event.sample.position), event.buttons()),
                    PointerEvent::ButtonChange(ButtonChange::Released(event)) => ("button-release", Some(event.sample.position), event.buttons()),
                    PointerEvent::Move(event) => {
                        ("move", Some(event.current().position), event.buttons)
                    }
                    PointerEvent::Scroll(event) => {
                        ("scroll", Some(event.position), PointerButtons::NONE)
                    }
                    PointerEvent::Cancel(_) => ("cancel", None, PointerButtons::NONE),
                    _ => ("other", None, PointerButtons::NONE),
                };
                let document = web_sys::window()
                    .expect("browser window")
                    .document()
                    .expect("document");
                let previous = document
                    .get_element_by_id("sequence")
                    .and_then(|node| node.text_content())
                    .unwrap_or_default();
                publish("sequence", &format!("{previous}{kind},"));
                if kind == "cancel" {
                    publish("cancel-event", "delivered");
                }
                if let Some(position) = position {
                    let point = position.get();
                    publish(
                        kind,
                        &format!(
                            r#"{{"x":{},"y":{},"x1":{},"x2":{},"primary":{},"secondary":{}}}"#,
                            point.x,
                            point.y,
                            buttons.contains(PointerButton::BACK),
                            buttons.contains(PointerButton::FORWARD),
                            buttons.contains(PointerButton::PRIMARY),
                            buttons.contains(PointerButton::SECONDARY),
                        ),
                    );
                }
            }
            DispatchEventResult::default()
        }));
        // WebWindow has no Drop cleanup: register_event_listeners deliberately
        // retains each DOM listener and its Arc<WindowCallbacks> until page close.
        // Thus the public input handler remains installed after these local owners
        // retire; a fresh page is the fixture's lifetime boundary.
        publish("ready", "input");
        Ok(())
    }

    #[wasm_bindgen]
    pub fn display_probe() -> Result<String, JsValue> {
        let platform = WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let display = platform.primary_display().expect("browser screen");
        let bounds = display.bounds();
        Ok(format!(
            r#"{{"width":{},"height":{},"scale":{}}}"#,
            bounds.size.width,
            bounds.size.height,
            display.scale_factor()
        ))
    }

    #[wasm_bindgen]
    pub fn created_callback_probe() -> Result<(), JsValue> {
        let platform =
            Arc::new(WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?);
        let reenter = Arc::clone(&platform);
        platform.on_window_event(Box::new(move |_| {
            drop(reenter.clipboard());
            publish("created-reentered", "returned");
        }));
        let _window = platform
            .open_window(WindowOptions::default())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        // Break the fixture's callback -> platform ownership after proving reentry.
        platform.on_window_event(Box::new(|_| {}));
        publish("created-finished", "returned");
        Ok(())
    }

    #[wasm_bindgen]
    pub fn quit_callback_probe() -> Result<(), JsValue> {
        let platform =
            Arc::new(WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?);
        let reenter = Arc::clone(&platform);
        platform.on_quit(Box::new(move || {
            drop(reenter.clipboard());
            publish("quit-reentered", "returned");
        }));
        platform.quit();
        publish("quit-finished", "returned");
        Ok(())
    }
}

fn main() {}
