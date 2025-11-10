//! WebAssembly support
//!
//! This module provides WebAssembly-specific implementations and utilities
//! for running Flui applications in web browsers.

use crate::app::FluiApp;
use flui_core::view::AnyView;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use winit::window::Window;

/// Initialize panic hook for better error messages in browser console
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Async version of FluiApp::new for WebAssembly
///
/// In WebAssembly, we can't use pollster::block_on because the browser's
/// event loop doesn't support blocking. Instead, we use async/await.
pub async fn new_async(root_view: Box<dyn AnyView>, window: Arc<Window>) -> FluiApp {
    // Create wgpu instance
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        #[cfg(target_arch = "wasm32")]
        backends: wgpu::Backends::GL | wgpu::Backends::BROWSER_WEBGPU,
        #[cfg(not(target_arch = "wasm32"))]
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    // Create surface
    let surface = instance
        .create_surface(Arc::clone(&window))
        .expect("Failed to create surface");

    // Request adapter (async)
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("Failed to find adapter");

    // Request device and queue (async)
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            #[cfg(target_arch = "wasm32")]
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            #[cfg(not(target_arch = "wasm32"))]
            required_limits: wgpu::Limits::default(),
            label: None,
            memory_hints: wgpu::MemoryHints::default(),
            trace: Default::default(),
        })
        .await
        .expect("Failed to create device");

    // Get window size
    let size = window.inner_size();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface.get_capabilities(&adapter).formats[0],
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // Create GPU painter
    let painter = flui_engine::painter::WgpuPainter::new(
        device.clone(),
        queue.clone(),
        config.format,
        (config.width, config.height),
    );

    // Note: We're duplicating the initialization logic from FluiApp::new
    // This is necessary because FluiApp::new uses pollster::block_on which doesn't work in WASM
    crate::app::FluiApp::from_components(
        root_view, instance, surface, device, queue, config, window, painter,
    )
}

/// Run a Flui app in the browser
///
/// This is the WebAssembly entry point. Call this from your wasm_bindgen start function.
///
/// # Example
///
/// ```rust,ignore
/// use wasm_bindgen::prelude::*;
///
/// #[wasm_bindgen(start)]
/// pub fn main() {
///     flui_app::wasm::run_in_browser(Box::new(MyApp));
/// }
/// ```
#[wasm_bindgen]
pub async fn run_in_browser_impl(root_view: Box<dyn AnyView>) {
    init_panic_hook();

    // Initialize logging for browser console
    #[cfg(target_arch = "wasm32")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(|| Box::new(std::io::stderr()) as Box<dyn std::io::Write>),
            )
            .init();
    }

    tracing::info!("Starting Flui app in browser...");

    // Get canvas from DOM
    let web_window = web_sys::window().expect("Failed to get window");
    let document = web_window.document().expect("Failed to get document");
    let canvas = document
        .get_element_by_id("flui-canvas")
        .expect("Failed to find canvas with id 'flui-canvas'")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("Element is not a canvas");

    // Set canvas size
    canvas.set_width(800);
    canvas.set_height(600);

    use winit::platform::web::WindowExtWebSys;

    // Create event loop and window
    let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let window_attributes = Window::default_attributes()
        .with_title("Flui WebAssembly App")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
        .with_canvas(Some(canvas));

    let window = Arc::new(
        event_loop
            .create_window(window_attributes)
            .expect("Failed to create window"),
    );

    // Create Flui app (async)
    let flui_app = new_async(root_view, window.clone()).await;

    // Create app state
    let mut app_state = WasmAppState {
        flui_app: Some(flui_app),
        window: Some(window.clone()),
    };

    // Request initial redraw
    window.request_redraw();

    // Run event loop
    event_loop
        .run_app(&mut app_state)
        .expect("Failed to run event loop");
}

/// Application state for WASM event loop
struct WasmAppState {
    flui_app: Option<FluiApp>,
    window: Option<Arc<Window>>,
}

impl winit::application::ApplicationHandler for WasmAppState {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Already initialized
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Dispatch to FluiApp callbacks
        if let Some(app) = &mut self.flui_app {
            app.handle_window_event(&event);
        }

        // Handle internal events
        match event {
            winit::event::WindowEvent::CloseRequested => {
                tracing::info!("Close requested");
                if let Some(app) = &mut self.flui_app {
                    app.cleanup();
                }
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(physical_size) => {
                if let Some(app) = &mut self.flui_app {
                    app.resize(physical_size.width, physical_size.height);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                if let Some(app) = &mut self.flui_app {
                    let needs_redraw = app.update();
                    if needs_redraw {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Don't request redraw here - only when needed
    }
}
