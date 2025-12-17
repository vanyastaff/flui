//! Counter Demo - FLUI application with GPU-accelerated window
//!
//! Demonstrates rendering using LayerTree with PerformanceOverlay.

use flui_app::AppConfig;
use flui_engine::wgpu::SceneRenderer;
use flui_layer::{
    CanvasLayer, Layer, LayerTree, OffsetLayer, PerformanceOverlayLayer, PerformanceOverlayOption,
    PerformanceStats, Scene,
};
use flui_painting::{Canvas, Paint};
use flui_types::geometry::{Offset, Rect, Size};
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

/// Create the main content canvas (Hello World)
fn create_content_canvas(frame_count: u64, width: u32, height: u32) -> Canvas {
    let mut canvas = Canvas::new();

    // Dark background
    let bg_color = Color::rgba(30, 30, 40, 255);
    let viewport = Rect::from_xywh(0.0, 0.0, width as f32, height as f32);
    canvas.draw_rect(viewport, &Paint::fill(bg_color));

    // Draw "Hello World" centered with animated color
    let hue = (frame_count % 360) as f32;
    let r = ((hue * 0.017).sin() * 127.0 + 128.0) as u8;
    let g = (((hue + 120.0) * 0.017).sin() * 127.0 + 128.0) as u8;
    let b = (((hue + 240.0) * 0.017).sin() * 127.0 + 128.0) as u8;
    let text_color = Color::rgba(r, g, b, 255);

    // Large "Hello World" text
    let hello_style = TextStyle::new().with_font_size(72.0).with_color(text_color);

    // Center the text (approximate)
    let text_x = (width as f32 - 400.0) / 2.0;
    let text_y = height as f32 / 2.0;

    canvas.draw_text(
        "Hello World!",
        Offset::new(text_x, text_y),
        &hello_style,
        &Paint::fill(text_color),
    );

    // Subtitle
    let subtitle_style = TextStyle::new()
        .with_font_size(24.0)
        .with_color(Color::rgba(150, 150, 160, 255));

    canvas.draw_text(
        "FLUI Framework - GPU Accelerated",
        Offset::new(text_x - 20.0, text_y + 60.0),
        &subtitle_style,
        &Paint::fill(Color::rgba(150, 150, 160, 255)),
    );

    canvas
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<SceneRenderer>,
    config: AppConfig,
    frame_count: u64,
    /// Performance statistics tracker
    perf_stats: PerformanceStats,
}

impl App {
    fn new(config: AppConfig) -> Self {
        Self {
            window: None,
            renderer: None,
            config,
            frame_count: 0,
            perf_stats: PerformanceStats::default(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(
                self.config.size.width as u32,
                self.config.size.height as u32,
            ));

        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        let size = window.inner_size();
        tracing::info!(
            "Window created: {:?} ({}x{})",
            window.id(),
            size.width,
            size.height
        );

        // Create SceneRenderer with the window
        let renderer = pollster::block_on(async {
            SceneRenderer::with_window(window.clone(), size.width, size.height)
                .await
                .expect("Failed to create SceneRenderer")
        });
        tracing::info!("SceneRenderer initialized");

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);
                    tracing::debug!("Resized to {}x{}", new_size.width, new_size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                // Record frame timing
                self.perf_stats.record_frame();
                self.frame_count += 1;

                // Get size from window
                let size = self.window.as_ref().map(|w| w.inner_size());

                if let (Some(renderer), Some(size)) = (&mut self.renderer, size) {
                    // Build LayerTree with proper hierarchy:
                    // root (OffsetLayer)
                    //   └── content (CanvasLayer)
                    //   └── overlay (PerformanceOverlayLayer)
                    let mut tree = LayerTree::new();

                    // Root container layer
                    let root_layer = Layer::Offset(OffsetLayer::zero());
                    let root_id = tree.insert(root_layer);
                    tree.set_root(Some(root_id));

                    // Content layer (child of root)
                    let content_canvas =
                        create_content_canvas(self.frame_count, size.width, size.height);
                    let content_layer = Layer::Canvas(CanvasLayer::from_canvas(content_canvas));
                    let content_id = tree.insert(content_layer);
                    tree.add_child(root_id, content_id);

                    // Performance overlay layer (child of root, renders on top)
                    let overlay_rect = Rect::from_xywh(8.0, 8.0, 110.0, 40.0);
                    let mut overlay = PerformanceOverlayLayer::new(
                        overlay_rect,
                        PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
                            | PerformanceOverlayOption::DISPLAY_ENGINE_STATISTICS,
                    );
                    overlay.update_stats(&self.perf_stats);
                    let overlay_layer = Layer::PerformanceOverlay(overlay);
                    let overlay_id = tree.insert(overlay_layer);
                    tree.add_child(root_id, overlay_id);

                    // Create scene and render
                    let scene = Scene::new(
                        Size::new(size.width as f32, size.height as f32),
                        tree,
                        Some(root_id),
                        self.frame_count,
                    );

                    match renderer.render_scene(&scene) {
                        Ok(()) => {
                            if self.frame_count % 60 == 0 {
                                tracing::debug!(
                                    "Frame {} rendered ({:.1} FPS)",
                                    self.frame_count,
                                    self.perf_stats.fps()
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Render error: {:?}", e);
                        }
                    }
                }

                // Request next frame
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "info,counter=debug,flui_engine=info,wgpu=warn"
                    .parse()
                    .unwrap()
            }),
        )
        .init();

    tracing::info!("Starting Counter Demo with GPU rendering and Performance Overlay");

    // Create app configuration
    let config = AppConfig::new()
        .with_title("FLUI Counter - GPU Accelerated")
        .with_size(800, 600);

    // Run event loop
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(config);
    event_loop.run_app(&mut app).unwrap();
}
