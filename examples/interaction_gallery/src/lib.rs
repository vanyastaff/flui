//! The interaction gallery: one screen per interaction widget, from one
//! widget tree on every platform.
//!
//! [`Gallery`] is the whole application. `src/main.rs` runs it on the
//! desktop; `android_main` below is what Android's `NativeActivity` calls
//! once it has loaded this crate as a `cdylib`, and `start` is the browser's
//! entry point once `wasm-bindgen` has bound it.
//!
//! Every screen shows a readout of what its last interaction did, and every
//! control publishes a name for assistive technology, so a person, a screen
//! reader and an automation client (`cargo xtask device windows-gallery`)
//! all read the same outcome. Each gesture also has its keyboard and
//! assistive path, which the widgets require through
//! [`GestureAlternatives`](flui::widgets::GestureAlternatives).

mod keyed;
mod painters;
mod screens;
mod shell;
mod style;

use flui::prelude::*;

pub use screens::Screen;

/// The gallery's root view.
#[derive(Clone, Debug, StatelessView)]
pub struct Gallery;

impl StatelessView for Gallery {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        WidgetsApp::new(shell::Shell).text_style(style::body_style())
    }
}

/// The desktop window the gallery opens: wide enough for the sidebar and a
/// screen side by side.
#[must_use]
pub fn config() -> AppConfig {
    AppConfig::new()
        .with_title("FLUI interaction gallery")
        .with_size(1040, 720)
        .with_min_size(360, 480)
}

/// Android's entry point: `android-activity` calls this exported symbol
/// with the system's `AndroidApp`.
#[cfg(target_os = "android")]
#[expect(
    unsafe_code,
    reason = "android-activity finds the entry point by its unmangled symbol"
)]
#[unsafe(no_mangle)]
extern "Rust" fn android_main(app: flui::android_activity::AndroidApp) {
    flui::run_app_android(app, Gallery);
}

/// The browser's entry point: a panic reads as a console error rather than
/// an opaque `unreachable`, then the gallery mounts into the page's
/// `#flui-canvas`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    run_app(Gallery);
}
