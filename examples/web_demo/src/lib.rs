//! FLUI Web Demo
//!
//! Demonstrates the web platform running in a browser.
//! Build with: `wasm-pack build --target web --out-dir pkg`
//! Serve with: `python -m http.server 8080` and open index.html

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::rc::Weak;

use wasm_bindgen::prelude::*;

use flui_foundation::geometry::WindowMetrics;
use flui_native::HostWindow;
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange,
    WindowHandler,
};

/// The canvas window's handler: logs every input event and each new drawable
/// size to the console.
#[derive(Default)]
struct ConsoleLog {
    /// The window, to read its snapshot when it changes; set once it opened.
    window: RefCell<Option<Weak<dyn HostWindow>>>,
    /// The drawable size logged last.
    logged: Cell<Option<WindowMetrics>>,
}

fn log(line: &str) {
    web_sys::console::log_1(&line.into());
}

impl WindowHandler for ConsoleLog {
    fn input(&self, input: PlatformInput) -> DispatchEventResult {
        match &input {
            PlatformInput::Pointer(pe) => {
                use flui_platform::pointer::PointerEvent;
                match pe {
                    PointerEvent::Down(e) => log(&format!(
                        "Pointer Down: button={:?} pos=({:.0}, {:.0})",
                        e.button(),
                        e.sample.position.get().x,
                        e.sample.position.get().y
                    )),
                    PointerEvent::Up(e) => log(&format!(
                        "Pointer Up: button={:?} pos=({:.0}, {:.0})",
                        e.button(),
                        e.sample.position.get().x,
                        e.sample.position.get().y
                    )),
                    PointerEvent::Scroll(e) => log(&format!("Scroll: delta={:?}", e.delta)),
                    // Move events are skipped to avoid console spam; the
                    // remaining variants are not interesting to this demo.
                    _ => {}
                }
            }
            // The web backend reports each pointer on its own.
            PlatformInput::PointerFrame(_) => {}
            PlatformInput::Keyboard(ke) => log(&format!(
                "Key {:?}: {:?} (code={:?})",
                ke.state(),
                ke.key,
                ke.code
            )),
            PlatformInput::Ime(ime_event) => log(&format!("IME: {ime_event:?}")),
            // The web backend has no data-transfer transport yet (ADR-0038
            // §8), so this arm can only log.
            PlatformInput::DragDrop(drag_drop_event) => {
                log(&format!("DragDrop: {drag_drop_event:?}"));
            }
            PlatformInput::Capabilities(capabilities) => {
                log(&format!("Input capabilities: {capabilities:?}"));
            }
        }
        DispatchEventResult::resolved(false, true)
    }

    fn state_changed(&self) {
        let window = self.window.borrow().as_ref().and_then(Weak::upgrade);
        let Some(window) = window else {
            return;
        };
        let metrics = window.state().metrics;
        if self.logged.replace(metrics) == metrics {
            return;
        }
        if let Some(metrics) = metrics {
            let size = metrics.physical();
            let scale = metrics.scale().get();
            log(&format!(
                "Resize: {}x{} (scale: {scale})",
                size.width, size.height
            ));
        }
    }

    // No GPU renderer in this demo: a frame draws nothing.
    fn frame_requested(&self, _request: FrameRequest) -> FrameOutcome {
        FrameOutcome::Declined
    }

    fn close_requested(&self) -> CloseDecision {
        CloseDecision::Close
    }

    fn closed(&self) {
        log("Window closed");
    }

    // Nothing renders into the canvas's surface.
    fn surface_changed(&self, _change: SurfaceChange) {}
}

/// Entry point invoked by the generated JS glue on module load: brings up the
/// web platform, logs every input event it dispatches, and requests frames.
#[wasm_bindgen(start)]
pub fn start() {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();

    log("FLUI Web Demo starting...");

    let platform = flui_native::current_platform().expect("Failed to initialize web platform");

    log(&format!("Platform: {}", platform.name()));

    // Create canvas window before running the event loop
    let options = flui_platform::WindowOptions::default()
        .with_title("FLUI Web Demo".to_string())
        .with_size(flui_foundation::geometry::Size::new(800.0, 600.0));

    let handler = Rc::new(ConsoleLog::default());
    let window = platform
        .open_window(options, handler.clone())
        .expect("Failed to create canvas");
    *handler.window.borrow_mut() = Some(Rc::downgrade(&window));

    let opened = window.state();
    handler.logged.set(opened.metrics);
    log(&format!(
        "Window created: {}x{} (scale: {})",
        opened.content_size.width,
        opened.content_size.height,
        opened.metrics.map_or(1.0, |metrics| metrics.scale().get())
    ));

    log("FLUI Web Demo ready! Try clicking and typing on the canvas.");

    // On the web `run` returns as soon as the bootstrap has, while the page
    // keeps running. The platform keeps its handler until it quits, so the
    // handler owns the window: dropping the window retires the canvas, and
    // with it the window's handler.
    platform
        .run(Rc::new(Page {
            window: RefCell::new(Some(window)),
            owner: RefCell::new(None),
        }))
        .expect("platform event loop exited with an error");
}

/// The page's application handler: it keeps the owner capability and the
/// canvas window for the page's life, and lets the window go when the page
/// quits. The page hears no other system event.
struct Page {
    window: RefCell<Option<Rc<dyn HostWindow>>>,
    owner: RefCell<Option<flui_native::OwnerPlatform>>,
}

impl flui_native::AppHandler for Page {
    fn ready(&self, owner: flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError> {
        self.owner.replace(Some(owner));
        Ok(())
    }

    fn quit_requested(&self, _: &flui_native::OwnerPlatform) {
        let window = self.window.borrow_mut().take();
        drop(window);
    }

    fn reopen(&self, _: &flui_native::OwnerPlatform) {}

    fn session_end(
        &self,
        _: &flui_native::OwnerPlatform,
        _: flui_platform::SessionEndPhase,
    ) -> flui_platform::SessionEndAnswer {
        flui_platform::SessionEndAnswer::Proceed
    }

    fn host_notice(&self, _: &flui_native::OwnerPlatform, _: flui_platform::SystemNotice) {}

    fn exit_policy(&self, _: &flui_native::OwnerPlatform) -> flui_native::ExitAnswer {
        // The page ends with its tab.
        flui_native::ExitAnswer::HostDecides
    }

    fn wake_deadline(&self, _: &flui_native::OwnerPlatform) -> Option<web_time::Instant> {
        None
    }
}
