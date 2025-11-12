//! Simple Counter Demo for FLUI Android
//!
//! Minimal working example that compiles for Android

use flui_app::run_app;
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct SimpleCounter;

impl View for SimpleCounter {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Simple placeholder - just a container
        (RenderContainer::default(), None::<AnyElement>)
    }
}

// Desktop entry point
#[cfg(not(target_os = "android"))]
fn main() {
    run_app(SimpleCounter);
}

// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    log::info!("Starting FLUI Simple Counter on Android");

    use winit::platform::android::EventLoopBuilderExtAndroid;
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.with_android_app(app);

    run_app(SimpleCounter);
}

// Placeholder RenderContainer
#[derive(Debug, Default)]
struct RenderContainer;

impl flui_core::render::LeafRender for RenderContainer {
    type Metadata = ();

    fn layout(&mut self, constraints: flui_types::BoxConstraints) -> flui_types::Size {
        constraints.max_size()
    }

    fn paint(&self, _offset: flui_types::Offset) -> flui_core::render::BoxedLayer {
        Box::new(flui_core::layer::PictureLayer::new())
    }
}
