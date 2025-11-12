//! Minimal FLUI example for Android using Empty widget

use flui_app::run_app;
use flui_widgets::basic::Empty;

// Desktop entry point
#[cfg(not(target_os = "android"))]
fn main() {
    run_app(Empty);
}

// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    use log::LevelFilter;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(LevelFilter::Info),
    );

    use winit::platform::android::EventLoopBuilderExtAndroid;
    use winit::event_loop::EventLoop;

    let mut event_loop_builder = EventLoop::builder();
    event_loop_builder.with_android_app(app);

    run_app(Empty);
}
