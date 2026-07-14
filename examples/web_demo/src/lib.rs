//! FLUI Web Demo
//!
//! Demonstrates the web platform running in a browser.
//! Build with: `wasm-pack build --target web --out-dir pkg`
//! Serve with: `python -m http.server 8080` and open index.html

use wasm_bindgen::prelude::*;

use flui_platform::traits::{DispatchEventResult, PlatformInput};

#[wasm_bindgen(start)]
pub fn start() {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();

    // Log to browser console
    web_sys::console::log_1(&"FLUI Web Demo starting...".into());

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

    web_sys::console::log_1(&format!("Platform: {}", platform.name()).into());

    // Create canvas window before running the event loop
    let options = flui_platform::WindowOptions {
        title: "FLUI Web Demo".to_string(),
        size: flui_types::geometry::Size::new(
            flui_types::geometry::px(800.0),
            flui_types::geometry::px(600.0),
        ),
        ..Default::default()
    };

    let window = platform
        .open_window(options)
        .expect("Failed to create canvas");

    web_sys::console::log_1(
        &format!(
            "Window created: {}x{} (scale: {})",
            window.logical_size().width.0,
            window.logical_size().height.0,
            window.scale_factor()
        )
        .into(),
    );

    // Register input callback -- log events to console
    window.on_input(Box::new(|input: PlatformInput| {
        match &input {
            PlatformInput::Pointer(pe) => {
                use ui_events::pointer::PointerEvent;
                match pe {
                    PointerEvent::Down(e) => {
                        web_sys::console::log_1(
                            &format!(
                                "Pointer Down: button={:?} pos=({:.0}, {:.0})",
                                e.button, e.state.position.x, e.state.position.y
                            )
                            .into(),
                        );
                    }
                    PointerEvent::Up(e) => {
                        web_sys::console::log_1(
                            &format!(
                                "Pointer Up: button={:?} pos=({:.0}, {:.0})",
                                e.button, e.state.position.x, e.state.position.y
                            )
                            .into(),
                        );
                    }
                    PointerEvent::Move(_) => {
                        // Skip move events to avoid console spam
                    }
                    PointerEvent::Scroll(e) => {
                        web_sys::console::log_1(&format!("Scroll: delta={:?}", e.delta).into());
                    }
                    _ => {}
                }
            }
            PlatformInput::Keyboard(ke) => {
                web_sys::console::log_1(
                    &format!("Key {:?}: {:?} (code={:?})", ke.state, ke.key, ke.code).into(),
                );
            }
        }
        DispatchEventResult::resolved(false, true)
    }));

    // Register frame callback
    window.on_request_frame(Box::new(|| {
        // Frame rendered -- no-op for now (no GPU renderer in this demo)
    }));

    // Register resize callback
    window.on_resize(Box::new(|size, scale| {
        web_sys::console::log_1(
            &format!(
                "Resize: {}x{} (scale: {scale})",
                size.width.0, size.height.0
            )
            .into(),
        );
    }));

    web_sys::console::log_1(&"FLUI Web Demo ready! Try clicking and typing on the canvas.".into());

    platform.run(Box::new(move |_platform| {
        // Keep window alive via closure capture
        let _window = window;
    }));
}
