//! Browser display probe: runs the Web platform and logs the display snapshot
//! it reports, then a new line each time the snapshot changes.
//!
//! Build for wasm32-unknown-unknown, generate the page with wasm-bindgen and
//! serve it; `browser_display_probe/README.md` has the commands. Lines go to the
//! page (`#display-log`) and the browser console, so a run can be read or
//! copied from either.
//!
//! The probe is a manual contract, not a test: what it shows is the browser's
//! own behaviour (page zoom, a window dragged to another monitor, a 120 Hz
//! display, Window Management access), which no controlled boundary stands in
//! for.

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::Cell;
    use std::fmt::Write as _;
    use std::rc::Rc;

    use flui_platform::{DisplaySnapshot, Platform, WindowOpen, WindowOptions};
    use wasm_bindgen::prelude::*;

    /// Append a line to the page's log and the console.
    fn log(line: &str) {
        web_sys::console::log_1(&JsValue::from_str(line));
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let element = document.get_element_by_id("display-log").or_else(|| {
            let element = document.create_element("pre").ok()?;
            element.set_id("display-log");
            document.body()?.append_child(&element).ok()?;
            Some(element)
        });
        if let Some(element) = element {
            let previous = element.text_content().unwrap_or_default();
            element.set_text_content(Some(&format!("{previous}{line}\n")));
        }
    }

    fn now_ms() -> f64 {
        web_sys::window()
            .and_then(|window| window.performance())
            .map_or(0.0, |performance| performance.now())
    }

    /// One snapshot as text: its revision, the window's display, and every
    /// display with the fields a run checks.
    fn describe(snapshot: &DisplaySnapshot, window_display: Option<u64>) -> String {
        let mut text = format!(
            "[{:>9.1} ms] snapshot rev {} window_display={}",
            now_ms(),
            snapshot.revision().get(),
            window_display.map_or_else(|| "none".to_owned(), |id| id.to_string()),
        );
        for display in snapshot.displays() {
            let period = display.refresh_period.map_or_else(
                || "unknown".to_owned(),
                |period| {
                    let seconds = period.get().as_secs_f64();
                    format!("{:.3} ms ({:.2} Hz)", seconds * 1000.0, 1.0 / seconds)
                },
            );
            let _ = write!(
                text,
                "\n    display id={} \"{}\"{} bounds={}x{} at ({}, {}) scale={} period={}",
                display.id.get(),
                display.name,
                if display.is_primary { " primary" } else { "" },
                display.bounds.size.width,
                display.bounds.size.height,
                display.bounds.origin.x,
                display.bounds.origin.y,
                display.scale_factor,
                period,
            );
        }
        text
    }

    /// A value for the owner's wake hook, which is typed `Send`.
    struct OnThePageThread<T>(T);

    // SAFETY: wasm32-unknown-unknown runs the page on one thread and the wake
    // hook is called on it, so the value is never reachable from another.
    #[expect(
        unsafe_code,
        reason = "the wake hook is typed Send; wasm32 has one thread"
    )]
    unsafe impl<T> Send for OnThePageThread<T> {}

    impl<T> OnThePageThread<T> {
        fn get(&self) -> &T {
            &self.0
        }
    }

    /// Start the platform, open the canvas window and log the snapshot now and
    /// after every owner wake that changed it.
    #[wasm_bindgen]
    pub fn display_probe() -> Result<(), JsValue> {
        let platform = flui_platform::WebPlatform::new()
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Box::new(platform)
            .run(Box::new(move |owner| {
                let WindowOpen::Ready(window) = owner.open_window(WindowOptions::default())? else {
                    return Err("Web owner opening must be immediate".into());
                };
                let owner = Rc::new(owner);
                let last = Cell::new(u64::MAX);
                let report = {
                    let owner = Rc::clone(&owner);
                    move || match owner.display_snapshot() {
                        Ok(snapshot) => {
                            let revision = snapshot.revision().get();
                            if last.replace(revision) != revision {
                                let id = window.display_id().map(flui_platform::DisplayId::get);
                                log(&describe(&snapshot, id));
                            }
                        }
                        Err(error) => log(&format!("display_snapshot failed: {error}")),
                    }
                };
                report();
                let hook = OnThePageThread(report);
                owner
                    .on_wake(Box::new(move || (hook.get())()))
                    .map_err(|error| error.to_string())?;
                // The owner lives for the page: the wake hook reaches it.
                std::mem::forget(owner);
                Ok(())
            }))
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

fn main() {}
