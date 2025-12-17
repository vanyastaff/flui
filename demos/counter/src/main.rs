//! Counter Demo - FLUI application with GPU-accelerated window
//!
//! Demonstrates rendering using Canvas -> CanvasLayer -> SceneRenderer pipeline.

use flui_app::AppConfig;
use flui_engine::wgpu::SceneRenderer;
use flui_layer::{CanvasLayer, Layer};
use flui_painting::{Canvas, Paint};
use flui_types::geometry::{Offset, Rect};
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

/// Create a canvas with drawing commands for a frame
fn create_frame_canvas(frame_count: u64, width: u32, height: u32) -> Canvas {
    let mut canvas = Canvas::new();

    // Animated background color
    let t = (frame_count as f32 * 0.01).sin() * 0.5 + 0.5;
    let bg_color = Color::rgba(
        (30.0 + t * 20.0) as u8,
        (40.0 + t * 30.0) as u8,
        (60.0 + t * 40.0) as u8,
        255,
    );

    // Draw background
    let viewport = Rect::from_xywh(0.0, 0.0, width as f32, height as f32);
    canvas.draw_rect(viewport, &Paint::fill(bg_color));

    // Draw animated centered rectangle
    let rect_size = 200.0;
    let x = (width as f32 - rect_size) / 2.0;
    let y = (height as f32 - rect_size) / 2.0;
    let rect = Rect::from_xywh(x, y, rect_size, rect_size);

    // Animate color using hue
    let hue = (frame_count % 360) as f32;
    let r = ((hue * 0.017).sin() * 127.0 + 128.0) as u8;
    let g = (((hue + 120.0) * 0.017).sin() * 127.0 + 128.0) as u8;
    let b = (((hue + 240.0) * 0.017).sin() * 127.0 + 128.0) as u8;
    let box_color = Color::rgba(r, g, b, 255);

    canvas.draw_rect(rect, &Paint::fill(box_color));

    // Draw frame counter text
    let text = format!("Frame: {}", frame_count);
    let text_style = TextStyle::new()
        .with_font_size(24.0)
        .with_color(Color::WHITE);
    canvas.draw_text(
        &text,
        Offset::new(20.0, 30.0),
        &text_style,
        &Paint::fill(Color::WHITE),
    );

    canvas
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<SceneRenderer>,
    config: AppConfig,
    frame_count: u64,
}

impl App {
    fn new(config: AppConfig) -> Self {
        Self {
            window: None,
            renderer: None,
            config,
            frame_count: 0,
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
                self.frame_count += 1;

                // Get size from window first
                let size = self.window.as_ref().map(|w| w.inner_size());

                if let (Some(renderer), Some(size)) = (&mut self.renderer, size) {
                    // Create canvas with drawing commands
                    let canvas = create_frame_canvas(self.frame_count, size.width, size.height);

                    // Wrap in CanvasLayer and Layer
                    let canvas_layer = CanvasLayer::from_canvas(canvas);
                    let layer = Layer::Canvas(canvas_layer);

                    // Render
                    match renderer.render(&layer) {
                        Ok(()) => {
                            if self.frame_count % 60 == 0 {
                                tracing::debug!("Frame {} rendered", self.frame_count);
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

    tracing::info!("Starting Counter Demo with GPU rendering");

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
