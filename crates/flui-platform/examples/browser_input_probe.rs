//! Real browser dispatch coverage for the web platform's public seams.
//! Build for wasm32-unknown-unknown and load wasm-bindgen's web output in a
//! fresh page for each exported probe. The host runner supplies actual DOM events.

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Weak},
    };

    use flui_platform::platforms::web::WebPlatform;
    use flui_platform::{DispatchEventResult, HostWindow, Platform, WindowOptions};
    use flui_platform_api::PlatformInput;
    use flui_platform_api::pointer::{
        ButtonChange, PenOrientation, PointerButton, PointerButtons, PointerEvent, PointerSample,
        Twist,
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
            number(sample.orientation.and_then(PenOrientation::altitude)),
            number(sample.orientation.and_then(PenOrientation::azimuth)),
            number(sample.twist.map(Twist::radians)),
            number(sample.contact_size.map(|value| value.get().width)),
            number(sample.contact_size.map(|value| value.get().height)),
        )
    }

    fn append_result(name: &str, value: &str) {
        let previous = web_sys::window()
            .expect("browser window")
            .document()
            .expect("document")
            .get_element_by_id(name)
            .and_then(|node| node.text_content())
            .unwrap_or_default();
        publish(name, &format!("{previous}{value},"));
    }

    fn json_string(value: &str) -> String {
        js_sys::JSON::stringify(&JsValue::from_str(value))
            .expect("JSON string serialization")
            .as_string()
            .expect("serialized JSON string")
    }

    fn record_modifiers(label: &str, input: &PlatformInput) {
        use flui_platform_api::keyboard::Modifiers;

        let (kind, key, modifiers) = match input {
            PlatformInput::Keyboard(event) => {
                let key = match &event.key {
                    flui_platform_api::keyboard::Key::Named(key) => key.as_str(),
                    flui_platform_api::keyboard::Key::Character(text) => text.as_str(),
                    _ => "unknown",
                };
                ("keyboard", key, event.modifiers)
            }
            PlatformInput::Pointer(PointerEvent::Down(event)) => ("down", "", event.modifiers),
            PlatformInput::Pointer(PointerEvent::Up(event)) => ("up", "", event.modifiers),
            PlatformInput::Pointer(PointerEvent::Move(event)) => ("move", "", event.modifiers),
            PlatformInput::Pointer(PointerEvent::Scroll(event)) => ("scroll", "", event.modifiers),
            PlatformInput::Pointer(PointerEvent::ButtonChange(ButtonChange::Pressed(event))) => {
                ("button-press", "", event.modifiers)
            }
            PlatformInput::Pointer(PointerEvent::ButtonChange(ButtonChange::Released(event))) => {
                ("button-release", "", event.modifiers)
            }
            _ => return,
        };
        let flags = [
            ("shift", Modifiers::SHIFT),
            ("control", Modifiers::CONTROL),
            ("alt", Modifiers::ALT),
            ("meta", Modifiers::META),
            ("alt_graph", Modifiers::ALT_GRAPH),
            ("caps_lock", Modifiers::CAPS_LOCK),
            ("num_lock", Modifiers::NUM_LOCK),
            ("scroll_lock", Modifiers::SCROLL_LOCK),
            ("fn", Modifiers::FN),
            ("fn_lock", Modifiers::FN_LOCK),
            ("symbol", Modifiers::SYMBOL),
            ("symbol_lock", Modifiers::SYMBOL_LOCK),
        ]
        .map(|(name, flag)| format!(r#""{name}":{}"#, modifiers.contains(flag)))
        .join(",");
        append_result(
            &format!("modifier-records-{label}"),
            &format!(
                r#"{{"kind":"{kind}","key":{},"modifiers":{{{flags}}}}}"#,
                json_string(key)
            ),
        );
    }

    /// Keeps the actual platform and public window handle alive between DOM steps.
    #[wasm_bindgen]
    pub struct BrowserWindowProbe {
        window: Arc<dyn HostWindow>,
        platform: Arc<WebPlatform>,
        capture: Weak<()>,
    }

    impl std::fmt::Debug for BrowserWindowProbe {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("BrowserWindowProbe")
                .finish_non_exhaustive()
        }
    }

    #[wasm_bindgen]
    impl BrowserWindowProbe {
        pub fn snapshot(&self) -> String {
            format!(
                r#"{{"focused":{},"active":{},"visible":{},"capture_alive":{},"platform_active":{},"surface_available":{}}}"#,
                self.window.is_focused(),
                self.window.is_active(),
                self.window.is_visible(),
                self.capture.upgrade().is_some(),
                self.platform.active_window().is_some(),
                self.window.window_handle().is_ok(),
            )
        }

        pub fn close(&self) {
            self.window.close();
        }

        pub fn reopen(
            &self,
            label: String,
            propagate: bool,
            prevent_default: bool,
        ) -> Result<BrowserWindowProbe, JsValue> {
            make_keyboard_probe(
                Arc::clone(&self.platform),
                label,
                propagate,
                prevent_default,
            )
        }

        /// Offering a callback to a closed handle must retire its captures now.
        pub fn install_late_input(&self) -> bool {
            let capture = Arc::new(());
            let witness = Arc::downgrade(&capture);
            self.window.on_input(Box::new(move |_| {
                let _ = &capture;
                publish("late-input", "unexpected delivery");
                DispatchEventResult::default()
            }));
            witness.upgrade().is_some()
        }

        pub fn close_on(&self, source: &str) {
            let window = Arc::downgrade(&self.window);
            match source {
                "input" => self.window.on_input(Box::new(move |_| {
                    append_result("close-prefix", "input");
                    if let Some(window) = window.upgrade() {
                        window.close();
                    }
                    append_result("close-prefix", "returned");
                    DispatchEventResult::default()
                })),
                "focus" => self.window.on_active_status_change(Box::new(move |_| {
                    append_result("close-prefix", "focus");
                    if let Some(window) = window.upgrade() {
                        window.close();
                    }
                    append_result("close-prefix", "returned");
                })),
                "resize" => self.window.on_resize(Box::new(move |_, _| {
                    append_result("close-prefix", "resize");
                    if let Some(window) = window.upgrade() {
                        window.close();
                    }
                    append_result("close-prefix", "returned");
                })),
                "visibility" => self.window.on_visibility_status_change(Box::new(move |_| {
                    append_result("close-prefix", "visibility");
                    if let Some(window) = window.upgrade() {
                        window.close();
                    }
                    append_result("close-prefix", "returned");
                })),
                _ => panic!("unknown close probe source"),
            }
            self.window
                .on_close(Box::new(|| append_result("close-prefix", "closed")));
        }
    }

    #[wasm_bindgen]
    pub fn keyboard_window_probe(
        label: String,
        propagate: bool,
        prevent_default: bool,
    ) -> Result<BrowserWindowProbe, JsValue> {
        let platform =
            Arc::new(WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?);
        make_keyboard_probe(platform, label, propagate, prevent_default)
    }

    fn make_keyboard_probe(
        platform: Arc<WebPlatform>,
        label: String,
        propagate: bool,
        prevent_default: bool,
    ) -> Result<BrowserWindowProbe, JsValue> {
        let created = format!("created-{label}");
        platform.on_window_event(Box::new(move |event| {
            if matches!(event, flui_platform::WindowEvent::Created(_)) {
                append_result(&created, "created");
            }
        }));
        let window = platform
            .open_window(WindowOptions::default())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let capture = Arc::new(());
        let witness = Arc::downgrade(&capture);
        let key_output = format!("focus-keys-{label}");
        let modifier_label = label.clone();
        window.on_input(Box::new(move |input| {
            let _ = &capture;
            record_modifiers(&modifier_label, &input);
            if let PlatformInput::Keyboard(event) = input {
                let key = match &event.key {
                    flui_platform_api::keyboard::Key::Named(key) => key.as_str(),
                    flui_platform_api::keyboard::Key::Character(text) => text.as_str(),
                    _ => "unknown",
                };
                append_result(&key_output, key);
            }
            DispatchEventResult::resolved(propagate, prevent_default)
        }));
        let observation = Arc::downgrade(&window);
        let focus_output = format!("focus-state-{label}");
        window.on_active_status_change(Box::new(move |active| {
            if let Some(window) = observation.upgrade() {
                append_result(
                    &focus_output,
                    &format!("{active}:{}:{}", window.is_focused(), window.is_active()),
                );
            }
        }));
        let observation = Arc::downgrade(&window);
        let visibility_output = format!("visibility-state-{label}");
        window.on_visibility_status_change(Box::new(move |visible| {
            if let Some(window) = observation.upgrade() {
                append_result(
                    &visibility_output,
                    &format!("{visible}:{}", window.is_visible()),
                );
            }
        }));
        Ok(BrowserWindowProbe {
            window,
            platform,
            capture: witness,
        })
    }

    /// Executes the real platform RAF path without initializing a GPU renderer.
    #[wasm_bindgen]
    pub struct FrameWindowProbe {
        owner: flui_platform::OwnerPlatform,
        window: Arc<dyn HostWindow>,
    }

    impl std::fmt::Debug for FrameWindowProbe {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("FrameWindowProbe")
                .finish_non_exhaustive()
        }
    }

    fn arm_frame(window: &Arc<dyn HostWindow>, label: String) {
        let weak = Arc::downgrade(window);
        window.on_request_frame(Box::new(move || {
            if weak.upgrade().is_some() {
                append_result("frame-callbacks", &label);
            }
        }));
    }

    #[wasm_bindgen]
    impl FrameWindowProbe {
        pub fn close(&self) {
            self.window.close();
        }

        pub fn reopen(&mut self, label: String) -> Result<(), JsValue> {
            self.window.close();
            let flui_platform::WindowOpen::Ready(window) = self
                .owner
                .open_window(WindowOptions::default())
                .map_err(|error| JsValue::from_str(&error.to_string()))?
            else {
                return Err(JsValue::from_str("Web owner opening must be immediate"));
            };
            arm_frame(&window, label);
            self.window = window;
            Ok(())
        }

        pub fn platform_active(&self) -> bool {
            self.owner.active_window().is_some()
        }
    }

    impl Drop for FrameWindowProbe {
        fn drop(&mut self) {
            self.window.close();
            self.owner.quit();
        }
    }

    #[wasm_bindgen]
    pub fn frame_retirement_probe(label: String) -> Result<FrameWindowProbe, JsValue> {
        let result = Rc::new(RefCell::new(None));
        let installed = Rc::clone(&result);
        let platform = WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        Box::new(platform)
            .run(Box::new(move |owner| {
                owner.shared().on_window_event(Box::new(|event| {
                    if matches!(event, flui_platform::WindowEvent::RedrawRequested { .. }) {
                        append_result("frame-redraws", "redraw");
                    }
                }));
                let flui_platform::WindowOpen::Ready(window) =
                    owner.open_window(WindowOptions::default())?
                else {
                    return Err("Web owner opening must be immediate".into());
                };
                arm_frame(&window, label);
                *installed.borrow_mut() = Some(FrameWindowProbe { owner, window });
                Ok(())
            }))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let installed = result
            .borrow_mut()
            .take()
            .expect("Web ready installed probe synchronously");
        Ok(installed)
    }

    #[wasm_bindgen]
    pub fn input_probe() -> Result<BrowserWindowProbe, JsValue> {
        let platform =
            Arc::new(WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?);
        let window = platform
            .open_window(WindowOptions::default())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let capture = Arc::new(());
        let witness = Arc::downgrade(&capture);
        window.on_input(Box::new(move |input| {
            let _ = &capture;
            record_modifiers("input", &input);
            if let PlatformInput::Keyboard(event) = &input {
                let (key, key_kind) = match &event.key {
                    flui_platform_api::keyboard::Key::Named(key) => (key.as_str(), "Named"),
                    flui_platform_api::keyboard::Key::Character(text) => (text.as_str(), "Character"),
                    _ => ("unknown", "Unknown"),
                };
                append_result("keyboard-sequence", key);
                publish("keyboard", &format!(
                    r#"{{"key":{},"key_kind":"{}","code":{},"time":{},"state":"{:?}","location":"{:?}","repeat":"{:?}","composition":"{:?}","shift":{},"control":{},"alt":{},"meta":{},"alt_graph":{},"caps_lock":{},"num_lock":{},"scroll_lock":{},"fn":{},"fn_lock":{},"symbol":{},"symbol_lock":{}}}"#,
                    json_string(key), key_kind, json_string(event.code.as_str()), event.time.as_nanos(), event.state(), event.location,
                    event.repeat(), event.composition,
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::SHIFT),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::CONTROL),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::ALT),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::META),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::ALT_GRAPH),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::CAPS_LOCK),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::NUM_LOCK),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::SCROLL_LOCK),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::FN),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::FN_LOCK),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::SYMBOL),
                    event.modifiers.contains(flui_platform_api::keyboard::Modifiers::SYMBOL_LOCK),
                ));
            }
            if let PlatformInput::Pointer(event) = input {
                if let PointerEvent::Scroll(event) = &event {
                    append_result("wheel-sequence", &event.delta.x().to_string());
                    publish("scroll-data", &format!(
                        r#"{{"unit":"{:?}","x":{},"y":{},"precision":"{:?}","shift":{}}}"#,
                        event.delta.unit(), event.delta.x(), event.delta.y(), event.precision,
                        event.modifiers.contains(flui_platform_api::keyboard::Modifiers::SHIFT),
                    ));
                }
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
        publish("ready", "input");
        Ok(BrowserWindowProbe {
            window,
            platform,
            capture: witness,
        })
    }

    #[wasm_bindgen]
    pub fn display_probe() -> Result<String, JsValue> {
        let platform = WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let snapshot = platform
            .display_snapshot()
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let display = snapshot.primary().expect("browser screen");
        Ok(format!(
            r#"{{"width":{},"height":{},"scale":{}}}"#,
            display.bounds.size.width, display.bounds.size.height, display.scale_factor
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
