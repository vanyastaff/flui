//! Real browser dispatch coverage for the web platform's public seams.
//! Build for wasm32-unknown-unknown and load wasm-bindgen's web output in a
//! fresh page for each exported probe. The host runner supplies actual DOM events.

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::sync::Arc;

    use flui_platform::platforms::web::WebPlatform;
    use flui_platform::{DispatchEventResult, Platform, WindowOptions};
    use flui_platform_api::PlatformInput;
    use ui_events::pointer::{PointerButton, PointerEvent};
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

    #[wasm_bindgen]
    pub fn input_probe() -> Result<(), JsValue> {
        let platform = WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let window = platform
            .open_window(WindowOptions::default())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        window.on_input(Box::new(|input| {
            if let PlatformInput::Pointer(event) = input {
                let (kind, state) = match &event {
                    PointerEvent::Down(event) => ("down", Some(&event.state)),
                    PointerEvent::Up(event) => ("up", Some(&event.state)),
                    PointerEvent::Move(event) => ("move", Some(&event.current)),
                    PointerEvent::Scroll(event) => ("scroll", Some(&event.state)),
                    PointerEvent::Cancel(_) => ("cancel", None),
                    _ => ("other", None),
                };
                let document = web_sys::window().expect("browser window").document().expect("document");
                let previous = document.get_element_by_id("sequence").and_then(|node| node.text_content()).unwrap_or_default();
                publish("sequence", &format!("{previous}{kind},"));
                if kind == "cancel" {
                    publish("cancel-event", "delivered");
                }
                if let Some(state) = state {
                    // FLUI's contract is logical even though ui-events names this
                    // storage PhysicalPosition; consumers read these values directly.
                    publish(
                        kind,
                        &format!(
                            r#"{{"x":{},"y":{},"scale":{},"x1":{},"x2":{}}}"#,
                            state.position.x,
                            state.position.y,
                            state.scale_factor,
                            state.buttons.contains(PointerButton::X1),
                            state.buttons.contains(PointerButton::X2),
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
