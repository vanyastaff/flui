//! The gallery on the desktop: `cargo run -p flui-interaction-gallery`
//! (add `--features a11y` for Narrator, VoiceOver and Orca).

fn main() {
    // Android loads the library through `android_main`, and the browser
    // through `start`; a binary only runs on the desktop.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    flui::run_app_with_config(
        flui_interaction_gallery::Gallery,
        flui_interaction_gallery::config(),
    );
}
