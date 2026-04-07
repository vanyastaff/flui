//! Visual rendering demo for flui-engine.
//!
//! Renders a grid of colored shapes, tessellated paths (triangle, star,
//! diamond), gradient effects (sweep, linear), and text runs using the
//! flui-engine GPU pipeline directly (no scene/layer tree).
//!
//! Run: `cargo run -p flui-engine --example render_demo`

#![allow(unsafe_code)]

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use flui_engine::batchers::effects::{
    GradientStop, LinearGradientInstance, SweepGradientInstance,
};
use flui_engine::context::gpu_device::GpuDevice;
use flui_engine::context::render_surface::RenderSurface;
use flui_engine::text::cache::TextCacheKey;

use lyon::math::point;
use lyon::path::Path;


/// Application state machine: starts without a window, creates GPU resources
/// once the event loop is active.
struct App {
    /// wgpu instance, created once at startup.
    instance: wgpu::Instance,
    /// Initialised after the window is created.
    state: Option<RenderState>,
}

/// Per-window GPU state.
struct RenderState {
    window: Arc<Window>,
    /// Kept alive so the device outlives the surface.
    #[allow(dead_code)]
    gpu: Arc<GpuDevice>,
    surface: RenderSurface,
}

impl App {
    fn new() -> Self {
        let backends = {
            #[cfg(target_os = "linux")]
            {
                wgpu::Backends::VULKAN
            }
            #[cfg(target_os = "macos")]
            {
                wgpu::Backends::METAL
            }
            #[cfg(target_os = "windows")]
            {
                wgpu::Backends::DX12
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                wgpu::Backends::all()
            }
        };

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        Self {
            instance,
            state: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = WindowAttributes::default()
            .with_title("flui-engine render demo")
            .with_inner_size(winit::dpi::LogicalSize::new(800u32, 600u32));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        // SAFETY: the window Arc is kept alive in RenderState for the
        // lifetime of the surface, ensuring the raw window handle is valid.
        match unsafe { self.create_render_state(&window) } {
            Ok(state) => {
                self.state = Some(state);
            }
            Err(e) => {
                tracing::error!("failed to initialise GPU: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                let scale = state.window.scale_factor() as f32;
                state.surface.resize(new_size.width, new_size.height, scale);
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = render_frame(&mut state.surface) {
                    tracing::warn!("frame error (will retry): {e}");
                    // On surface-lost, resize to trigger reconfigure.
                    let size = state.window.inner_size();
                    let scale = state.window.scale_factor() as f32;
                    state.surface.resize(size.width, size.height, scale);
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Create the GPU device and render surface for a window.
    ///
    /// # Safety
    ///
    /// The returned `RenderState` borrows from `window` via raw handles;
    /// the caller must keep the `Window` alive for the lifetime of the state.
    unsafe fn create_render_state(
        &self,
        window: &Arc<Window>,
    ) -> Result<RenderState, Box<dyn std::error::Error>> {
        // Step 1: create a temporary wgpu surface for adapter selection.
        let temp_surface = self
            .instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(window.as_ref())?)?;

        // Step 2: create GpuDevice using the surface for adapter compatibility.
        let gpu = Arc::new(GpuDevice::new_with_surface(&self.instance, &temp_surface)?);

        // Step 3: create the RenderSurface (creates its own internal surface).
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let surface = RenderSurface::new(
            Arc::clone(&gpu),
            &self.instance,
            window.as_ref(),
            size.width,
            size.height,
            scale,
        )?;

        Ok(RenderState {
            window: Arc::clone(window),
            gpu,
            surface,
        })
    }
}

/// Draw one frame showcasing shapes, paths, gradients, and text.
fn render_frame(surface: &mut RenderSurface) -> Result<(), Box<dyn std::error::Error>> {
    let mut frame = surface.begin_frame()?;
    let batchers = frame.batchers_mut();

    // -- Grid of rounded rectangles ------------------------------------------
    let cols = 10;
    let rows = 8;
    for row in 0..rows {
        for col in 0..cols {
            let x = 20.0 + col as f32 * 75.0;
            let y = 20.0 + row as f32 * 55.0;
            let r = col as f32 / cols as f32;
            let g = row as f32 / rows as f32;
            batchers.shapes.add_rect(
                x,
                y,
                60.0,
                40.0,
                [r, g, 0.5, 1.0],     // colour: gradient across grid
                [8.0, 8.0, 8.0, 8.0], // per-corner radii
                [1.0, 1.0, 0.0, 0.0], // identity transform
            );
        }
    }

    // -- Row of circles at the bottom ----------------------------------------
    for i in 0..5 {
        let cx = 100.0 + i as f32 * 150.0;
        batchers.shapes.add_circle(
            cx,
            500.0,
            30.0,
            [0.2, 0.6, 1.0, 1.0], // blue-ish
            [1.0, 1.0, 0.0, 0.0], // identity transform
        );
    }

    // -- A few arcs for variety ----------------------------------------------
    for i in 0..3 {
        let cx = 150.0 + i as f32 * 250.0;
        batchers.shapes.add_arc(
            cx,
            500.0,
            25.0,
            0.0,
            std::f32::consts::PI * 1.5,
            [1.0, 0.4, 0.1, 1.0], // orange
        );
    }

    // -- Ovals ---------------------------------------------------------------
    batchers.shapes.add_oval(
        620.0,
        480.0,
        60.0,
        30.0,
        [0.6, 0.2, 0.8, 1.0], // purple
        [1.0, 1.0, 0.0, 0.0], // identity transform
    );

    // -- Lines ---------------------------------------------------------------
    for i in 0..4 {
        let y = 470.0 + i as f32 * 12.0;
        let hue = i as f32 / 4.0;
        batchers.shapes.add_line(
            620.0,
            y,
            780.0,
            y,
            [hue, 1.0 - hue, 0.5, 1.0],
            2.0,
        );
    }

    // -- Tessellated path: triangle via PathBatcher --------------------------
    {
        let mut builder = Path::builder();
        builder.begin(point(50.0, 560.0));
        builder.line_to(point(90.0, 590.0));
        builder.line_to(point(10.0, 590.0));
        builder.close();
        let triangle = builder.build();
        batchers.paths.add_fill(&triangle, [0.9, 0.2, 0.3, 1.0]); // red triangle
    }

    // -- Tessellated path: star outline via PathBatcher ----------------------
    {
        let cx = 170.0_f32;
        let cy = 575.0_f32;
        let outer_r = 25.0_f32;
        let inner_r = 10.0_f32;
        let points = 5;
        let mut builder = Path::builder();
        for i in 0..(points * 2) {
            let angle =
                -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / points as f32;
            let r = if i % 2 == 0 { outer_r } else { inner_r };
            let px = cx + r * angle.cos();
            let py = cy + r * angle.sin();
            if i == 0 {
                builder.begin(point(px, py));
            } else {
                builder.line_to(point(px, py));
            }
        }
        builder.close();
        let star = builder.build();
        batchers
            .paths
            .add_stroke(&star, [1.0, 0.8, 0.0, 1.0], 2.0); // gold star outline
    }

    // -- Tessellated path: diamond via add_vertices --------------------------
    {
        let verts: &[[f32; 2]] = &[
            [270.0, 555.0], // top
            [295.0, 575.0], // right
            [270.0, 595.0], // bottom
            [245.0, 575.0], // left
        ];
        let indices: &[u32] = &[0, 1, 2, 0, 2, 3];
        batchers
            .paths
            .add_vertices(verts, None, indices, [0.2, 0.8, 0.4, 1.0]); // green diamond
    }

    // -- Sweep gradient effect -----------------------------------------------
    batchers.effects.add_sweep_gradient(SweepGradientInstance {
        bounds: [350.0, 545.0, 60.0, 60.0],
        center: [380.0, 575.0],
        start_angle: 0.0,
        end_angle: std::f32::consts::TAU,
        stops: vec![
            GradientStop {
                color: [1.0, 0.0, 0.0, 1.0],
                position: 0.0,
                _padding: [0.0; 3],
            },
            GradientStop {
                color: [0.0, 1.0, 0.0, 1.0],
                position: 0.33,
                _padding: [0.0; 3],
            },
            GradientStop {
                color: [0.0, 0.0, 1.0, 1.0],
                position: 0.66,
                _padding: [0.0; 3],
            },
            GradientStop {
                color: [1.0, 0.0, 0.0, 1.0],
                position: 1.0,
                _padding: [0.0; 3],
            },
        ],
        corner_radii: [0.0; 4],
        transform: [1.0, 0.0, 0.0, 1.0],
    });

    // -- Linear gradient effect ----------------------------------------------
    batchers
        .effects
        .add_linear_gradient(LinearGradientInstance {
            bounds: [430.0, 545.0, 120.0, 40.0],
            start: [430.0, 565.0],
            end: [550.0, 565.0],
            stops: vec![
                GradientStop {
                    color: [0.0, 0.5, 1.0, 1.0],
                    position: 0.0,
                    _padding: [0.0; 3],
                },
                GradientStop {
                    color: [1.0, 0.5, 0.0, 1.0],
                    position: 1.0,
                    _padding: [0.0; 3],
                },
            ],
            corner_radii: [10.0, 10.0, 10.0, 10.0],
            transform: [1.0, 0.0, 0.0, 1.0],
        });

    // -- Text runs via TextBatcher ------------------------------------------
    {
        let key = TextCacheKey::new("Hello FLUI Engine!", 32.0, "sans-serif", 400);
        batchers.text.add_run(
            key,
            "Hello FLUI Engine!".into(),
            "sans-serif".into(),
            [50.0, 470.0],
            [0.0, 0.0, 0.0, 1.0],
            None,
        );

        let key2 = TextCacheKey::new("GPU-accelerated text rendering", 18.0, "sans-serif", 400);
        batchers.text.add_run(
            key2,
            "GPU-accelerated text rendering".into(),
            "sans-serif".into(),
            [50.0, 510.0],
            [0.3, 0.3, 0.3, 1.0],
            None,
        );

        let key3 = TextCacheKey::new("Bold text sample", 24.0, "sans-serif", 700);
        batchers.text.add_run(
            key3,
            "Bold text sample".into(),
            "sans-serif".into(),
            [50.0, 540.0],
            [0.1, 0.3, 0.7, 1.0],
            None,
        );
    }

    frame.finish()?;
    Ok(())
}

fn main() {
    // Initialise tracing so GPU/engine spans are visible.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new();
    if let Err(e) = event_loop.run_app(&mut app) {
        tracing::error!("event loop exited with error: {e}");
    }
}
