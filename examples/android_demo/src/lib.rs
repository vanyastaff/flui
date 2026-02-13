//! FLUI Android Demo — Scene Render
//!
//! Demonstrates the full GPU rendering pipeline on Android:
//! Canvas (draw commands) -> DisplayList -> CanvasLayer -> Scene -> Renderer -> Vulkan -> pixels
//!
//! This is the Android equivalent of `examples/scene_render.rs`.
//!
//! # Build & Run
//!
//! ```bash
//! cargo ndk -t arm64-v8a -o platforms/android/app/src/main/jniLibs build -p flui-android-demo
//! cd platforms/android && ./gradlew assembleDebug
//! adb install -r app/build/outputs/apk/debug/app-debug.apk
//! adb shell am start -n com.vanya.flui.counter/android.app.NativeActivity
//! ```

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use flui_engine::wgpu::Renderer;
use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
use flui_types::geometry::{px, Rect, Size};
use flui_types::painting::Paint;
use flui_types::styling::Color;
use std::sync::Mutex;
use std::time::Duration;

/// Wrapper for raw-window-handle bridging (AndroidApp -> wgpu surface)
///
/// On android-activity 0.6, `AndroidApp` no longer implements `HasWindowHandle`/`HasDisplayHandle`.
/// We construct the raw handles manually from the `NativeWindow` pointer.
struct AndroidWindowHandle {
    app: AndroidApp,
}

impl raw_window_handle::HasWindowHandle for AndroidWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let native_window = self
            .app
            .native_window()
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let ptr = native_window.ptr().cast();
        let handle = raw_window_handle::AndroidNdkWindowHandle::new(ptr);
        let raw = raw_window_handle::RawWindowHandle::AndroidNdk(handle);
        // SAFETY: The ANativeWindow pointer is valid between Resume and Pause lifecycle events.
        #[allow(unsafe_code)]
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

impl raw_window_handle::HasDisplayHandle for AndroidWindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = raw_window_handle::AndroidDisplayHandle::new();
        let raw = raw_window_handle::RawDisplayHandle::Android(handle);
        // SAFETY: The Android display handle is always valid while the app is running.
        #[allow(unsafe_code)]
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }
}

/// Build a scene with colored rectangles to prove the compositor works.
fn build_test_scene(width: f32, height: f32) -> Scene {
    let mut tree = LayerTree::new();

    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    // Background — bright orange (hot-reload test!)
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
        &Paint::fill(Color::rgb(255, 140, 0)),
    );

    // Scale rectangles proportionally to screen size
    let scale_x = width / 800.0;
    let scale_y = height / 600.0;

    // Large red rectangle (top-left area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(50.0 * scale_x),
            px(50.0 * scale_y),
            px(350.0 * scale_x),
            px(250.0 * scale_y),
        ),
        &Paint::fill(Color::RED),
    );

    // Green rectangle (center area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(150.0 * scale_x),
            px(150.0 * scale_y),
            px(500.0 * scale_x),
            px(350.0 * scale_y),
        ),
        &Paint::fill(Color::GREEN),
    );

    // Blue rectangle (bottom-right area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(400.0 * scale_x),
            px(250.0 * scale_y),
            px(700.0 * scale_x),
            px(450.0 * scale_y),
        ),
        &Paint::fill(Color::BLUE),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(300.0 * scale_x),
            px(200.0 * scale_y),
            px(450.0 * scale_x),
            px(300.0 * scale_y),
        ),
        &Paint::fill(Color::WHITE),
    );

    // Yellow rectangle (bottom area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(100.0 * scale_x),
            px(400.0 * scale_y),
            px(600.0 * scale_x),
            px(500.0 * scale_y),
        ),
        &Paint::fill(Color::rgb(255, 200, 0)),
    );

    let root_id = tree.insert(Layer::Canvas(canvas_layer));

    Scene::new(Size::new(px(width), px(height)), tree, Some(root_id), 1)
}

/// Android entry point — called by NativeActivity when the library is loaded
#[no_mangle]
fn android_main(app: AndroidApp) {
    // Initialize logging to Android logcat
    flui_log::Logger::new()
        .with_filter("info,flui_engine=debug,wgpu=warn")
        .with_level(flui_log::Level::DEBUG)
        .init();

    tracing::info!("FLUI Android Demo starting — Scene Render");

    let mut renderer: Option<Mutex<Renderer>> = None;
    let mut running = true;
    let mut resumed = false;
    let mut needs_render = false;

    loop {
        if !running {
            break;
        }

        let mut surface_lost = false;

        // Block indefinitely when idle (no pending render), wake on events.
        // Use 16ms timeout only when we need to render (animation-ready).
        let timeout = if needs_render {
            Some(Duration::from_millis(16))
        } else {
            None // Block until next system event — prevents ANR
        };

        app.poll_events(timeout, |event| match event {
            PollEvent::Main(main_event) => match main_event {
                MainEvent::Resume { .. } => {
                    tracing::info!("Resumed");
                    resumed = true;
                    needs_render = true;
                }
                MainEvent::Pause => {
                    tracing::info!("Paused — dropping renderer surface");
                    resumed = false;
                    surface_lost = true;
                }
                MainEvent::Destroy => {
                    tracing::info!("Destroy — shutting down");
                    running = false;
                }
                MainEvent::WindowResized { .. } => {
                    tracing::info!("Window resized");
                    needs_render = true;
                }
                MainEvent::InputAvailable => {
                    // Drain input queue to keep the input dispatcher happy
                }
                _ => {}
            },
            _ => {}
        });

        // Drain input events to prevent ANR (input dispatcher timeout)
        if let Ok(mut iter) = app.input_events_iter() {
            while iter.next(|_event| InputStatus::Unhandled) {}
        }

        // Drop renderer on pause (surface becomes invalid)
        if surface_lost {
            renderer = None;
            tracing::info!("Renderer dropped (surface lost)");
            continue;
        }

        // Create renderer once native window is available (may lag behind Resume)
        if resumed && renderer.is_none() && app.native_window().is_some() {
            tracing::info!("Native window ready — creating renderer");
            let handle = AndroidWindowHandle { app: app.clone() };
            match pollster::block_on(Renderer::new(&handle)) {
                Ok(mut r) => {
                    if let Some(native_window) = app.native_window() {
                        let w = native_window.width() as u32;
                        let h = native_window.height() as u32;
                        r.resize(w, h);
                        tracing::info!(
                            "GPU: {} ({:?}), surface: {}x{}",
                            r.capabilities().adapter_name,
                            r.capabilities().backend,
                            w,
                            h
                        );
                    }
                    renderer = Some(Mutex::new(r));
                    needs_render = true;
                    tracing::info!("Renderer created successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to create GPU renderer: {:?}", e);
                }
            }
        }

        // Render frame
        if needs_render {
            if let Some(ref renderer_mutex) = renderer {
                if let Some(native_window) = app.native_window() {
                    let w = native_window.width() as f32;
                    let h = native_window.height() as f32;

                    let scene = build_test_scene(w, h);

                    let mut r = renderer_mutex.lock().unwrap();

                    // Handle resize
                    let (cur_w, cur_h) = r.size();
                    if cur_w != w as u32 || cur_h != h as u32 {
                        r.resize(w as u32, h as u32);
                    }

                    match r.render_scene(&scene) {
                        Ok(()) => {
                            tracing::info!("Scene rendered successfully");
                            needs_render = false;
                        }
                        Err(e) => {
                            tracing::error!("render_scene failed: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    tracing::info!("FLUI Android Demo finished");
}
