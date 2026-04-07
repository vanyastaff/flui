//! Comprehensive feature showcase for flui-engine.
//!
//! Displays ALL engine rendering capabilities in a clean, organized layout:
//! rounded rectangles, circles, ovals, arcs, lines, tessellated paths,
//! linear/radial/sweep gradients, shadows, and text rendering.
//!
//! Run: `cargo run -p flui-engine --example render_demo`

#![allow(unsafe_code)]

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use flui_engine::batchers::effects::{
    GradientStop, LinearGradientInstance, RadialGradientInstance, ShadowInstance,
    SweepGradientInstance,
};
use flui_engine::context::gpu_device::GpuDevice;
use flui_engine::context::render_surface::RenderSurface;
use flui_engine::text::cache::TextCacheKey;

use lyon::math::point;
use lyon::path::Path;

// ---------------------------------------------------------------------------
// Color palette
// ---------------------------------------------------------------------------

const BLUE: [f32; 4] = [0.13, 0.59, 0.95, 1.0];
const RED: [f32; 4] = [0.91, 0.30, 0.24, 1.0];
const GREEN: [f32; 4] = [0.18, 0.80, 0.44, 1.0];
const PURPLE: [f32; 4] = [0.61, 0.15, 0.69, 1.0];
const ORANGE: [f32; 4] = [1.0, 0.60, 0.0, 1.0];
const TEAL: [f32; 4] = [0.0, 0.74, 0.83, 1.0];
const LABEL_GRAY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const IDENTITY: [f32; 4] = [1.0, 1.0, 0.0, 0.0];

// Layout constants
const LEFT_MARGIN: f32 = 30.0;
const LABEL_TO_CONTENT_GAP: f32 = 10.0;
const SECTION_GAP: f32 = 25.0;
const LABEL_SIZE: f32 = 14.0;
const LABEL_HEIGHT: f32 = 16.0;

/// Application state machine.
struct App {
    instance: wgpu::Instance,
    state: Option<RenderState>,
}

/// Per-window GPU state.
struct RenderState {
    window: Arc<Window>,
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
            .with_title("flui-engine Feature Showcase")
            .with_inner_size(winit::dpi::LogicalSize::new(1200u32, 900u32));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

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
    /// # Safety
    ///
    /// The returned `RenderState` borrows from `window` via raw handles;
    /// the caller must keep the `Window` alive for the lifetime of the state.
    unsafe fn create_render_state(
        &self,
        window: &Arc<Window>,
    ) -> Result<RenderState, Box<dyn std::error::Error>> {
        let temp_surface = self
            .instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(window.as_ref())?)?;

        let gpu = Arc::new(GpuDevice::new_with_surface(&self.instance, &temp_surface)?);

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_stop(color: [f32; 4], position: f32) -> GradientStop {
    GradientStop {
        color,
        position,
        _padding: [0.0; 3],
    }
}

fn add_label(
    batchers: &mut flui_engine::Batchers,
    text: &str,
    x: f32,
    y: f32,
) {
    let key = TextCacheKey::new(text, LABEL_SIZE, "sans-serif", 400);
    batchers.text.add_run(
        key,
        text.into(),
        "sans-serif".into(),
        [x, y],
        LABEL_GRAY,
        None,
    );
}

// ---------------------------------------------------------------------------
// Main render function
// ---------------------------------------------------------------------------

/// Draw one frame showcasing all engine capabilities.
fn render_frame(surface: &mut RenderSurface) -> Result<(), Box<dyn std::error::Error>> {
    let mut frame = surface.begin_frame()?;
    let batchers = frame.batchers_mut();

    let mut y = 15.0;

    // ===== Row 1: Rounded Rectangles ========================================
    add_label(batchers, "Rounded Rectangles", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let radii_values: [f32; 8] = [0.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 50.0];
    let rect_colors: [[f32; 4]; 8] = [
        BLUE, RED, GREEN, PURPLE, ORANGE, TEAL,
        [0.95, 0.77, 0.06, 1.0], // yellow
        [0.56, 0.27, 0.68, 1.0], // deep purple
    ];
    let rect_w = 120.0;
    let rect_h = 50.0;
    let rect_spacing = 135.0;
    for i in 0..8 {
        let x = LEFT_MARGIN + i as f32 * rect_spacing;
        let r = radii_values[i];
        // For the last one, make it fully round (radius = half height)
        let r_val = if i == 7 { rect_h / 2.0 } else { r };
        batchers.shapes.add_rect(
            x, y, rect_w, rect_h,
            rect_colors[i],
            [r_val; 4],
            IDENTITY,
        );
    }
    y += rect_h + SECTION_GAP;

    // ===== Row 2: Circles and Ovals =========================================
    add_label(batchers, "Circles & Ovals", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let circle_sizes: [f32; 5] = [15.0, 20.0, 25.0, 30.0, 22.0];
    let circle_colors: [[f32; 4]; 5] = [BLUE, RED, GREEN, PURPLE, ORANGE];
    let mut cx = LEFT_MARGIN + 30.0;
    for i in 0..5 {
        batchers.shapes.add_circle(
            cx, y + 30.0,
            circle_sizes[i],
            circle_colors[i],
            IDENTITY,
        );
        cx += circle_sizes[i] * 2.0 + 25.0;
    }

    // Ovals with different aspect ratios
    let oval_specs: [(f32, f32); 3] = [(80.0, 40.0), (50.0, 70.0), (100.0, 35.0)];
    let oval_colors: [[f32; 4]; 3] = [TEAL, PURPLE, ORANGE];
    cx += 20.0;
    for i in 0..3 {
        let (w, h) = oval_specs[i];
        batchers.shapes.add_oval(
            cx, y + 30.0 - h / 2.0,
            w, h,
            oval_colors[i],
            IDENTITY,
        );
        cx += w + 25.0;
    }
    y += 65.0 + SECTION_GAP;

    // ===== Row 3: Arcs ======================================================
    add_label(batchers, "Arcs", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let sweep_degrees: [f32; 6] = [30.0, 90.0, 180.0, 270.0, 330.0, 360.0];
    let arc_colors: [[f32; 4]; 6] = [BLUE, RED, GREEN, PURPLE, ORANGE, TEAL];
    let arc_r = 25.0;
    for i in 0..6 {
        let acx = LEFT_MARGIN + 40.0 + i as f32 * 100.0;
        let sweep_rad = sweep_degrees[i] * std::f32::consts::PI / 180.0;
        batchers.shapes.add_arc(
            acx, y + arc_r,
            arc_r,
            0.0,
            sweep_rad,
            arc_colors[i],
        );
    }
    y += arc_r * 2.0 + SECTION_GAP;

    // ===== Row 4: Lines =====================================================
    add_label(batchers, "Lines", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let line_base_y = y;
    // Horizontal lines - different widths (drawn as stroked paths)
    let h_widths: [f32; 3] = [1.0, 2.0, 4.0];
    let h_colors: [[f32; 4]; 3] = [BLUE, RED, GREEN];
    for i in 0..3 {
        let ly = line_base_y + i as f32 * 15.0;
        let mut builder = Path::builder();
        builder.begin(point(LEFT_MARGIN, ly));
        builder.line_to(point(LEFT_MARGIN + 200.0, ly));
        builder.end(false);
        let path = builder.build();
        batchers.paths.add_stroke(&path, h_colors[i], h_widths[i]);
    }

    // Vertical lines
    let v_colors: [[f32; 4]; 3] = [PURPLE, ORANGE, TEAL];
    for i in 0..3 {
        let lx = LEFT_MARGIN + 250.0 + i as f32 * 30.0;
        let mut builder = Path::builder();
        builder.begin(point(lx, line_base_y));
        builder.line_to(point(lx, line_base_y + 40.0));
        builder.end(false);
        let path = builder.build();
        batchers.paths.add_stroke(&path, v_colors[i], h_widths[i]);
    }

    // Diagonal lines
    let d_colors: [[f32; 4]; 3] = [RED, GREEN, BLUE];
    for i in 0..3 {
        let lx = LEFT_MARGIN + 380.0 + i as f32 * 80.0;
        let mut builder = Path::builder();
        builder.begin(point(lx, line_base_y));
        builder.line_to(point(lx + 60.0, line_base_y + 40.0));
        builder.end(false);
        let path = builder.build();
        batchers.paths.add_stroke(&path, d_colors[i], h_widths[i]);
    }
    y += 45.0 + SECTION_GAP;

    // ===== Row 5: Paths (Lyon Tessellation) =================================
    add_label(batchers, "Paths (Lyon Tessellation)", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let path_base_y = y;

    // Filled triangle
    {
        let tx = LEFT_MARGIN + 30.0;
        let ty = path_base_y;
        let mut builder = Path::builder();
        builder.begin(point(tx, ty));
        builder.line_to(point(tx + 50.0, ty + 50.0));
        builder.line_to(point(tx - 25.0, ty + 50.0));
        builder.close();
        let tri = builder.build();
        batchers.paths.add_fill(&tri, RED);
    }

    // Filled pentagon
    {
        let pcx = LEFT_MARGIN + 160.0;
        let pcy = path_base_y + 25.0;
        let pr = 28.0;
        let mut builder = Path::builder();
        for i in 0..5 {
            let angle = -std::f32::consts::FRAC_PI_2
                + 2.0 * std::f32::consts::PI * i as f32 / 5.0;
            let px = pcx + pr * angle.cos();
            let py = pcy + pr * angle.sin();
            if i == 0 {
                builder.begin(point(px, py));
            } else {
                builder.line_to(point(px, py));
            }
        }
        builder.close();
        let pent = builder.build();
        batchers.paths.add_fill(&pent, GREEN);
    }

    // Filled 5-point star
    {
        let scx = LEFT_MARGIN + 290.0;
        let scy = path_base_y + 25.0;
        let outer_r = 28.0;
        let inner_r = 12.0;
        let pts = 5;
        let mut builder = Path::builder();
        for i in 0..(pts * 2) {
            let angle = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::PI * i as f32 / pts as f32;
            let r = if i % 2 == 0 { outer_r } else { inner_r };
            let px = scx + r * angle.cos();
            let py = scy + r * angle.sin();
            if i == 0 {
                builder.begin(point(px, py));
            } else {
                builder.line_to(point(px, py));
            }
        }
        builder.close();
        let star = builder.build();
        batchers.paths.add_fill(&star, ORANGE);
    }

    // Stroked hexagon
    {
        let hcx = LEFT_MARGIN + 420.0;
        let hcy = path_base_y + 25.0;
        let hr = 28.0;
        let mut builder = Path::builder();
        for i in 0..6 {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / 6.0;
            let px = hcx + hr * angle.cos();
            let py = hcy + hr * angle.sin();
            if i == 0 {
                builder.begin(point(px, py));
            } else {
                builder.line_to(point(px, py));
            }
        }
        builder.close();
        let hex = builder.build();
        batchers.paths.add_stroke(&hex, PURPLE, 3.0);
    }

    // Stroked bezier curve
    {
        let bx = LEFT_MARGIN + 520.0;
        let by = path_base_y + 45.0;
        let mut builder = Path::builder();
        builder.begin(point(bx, by));
        builder.cubic_bezier_to(
            point(bx + 40.0, by - 60.0),
            point(bx + 80.0, by + 40.0),
            point(bx + 120.0, by - 10.0),
        );
        builder.end(false); // open path (not closed)
        let curve = builder.build();
        batchers.paths.add_stroke(&curve, TEAL, 2.5);
    }

    y += 60.0 + SECTION_GAP;

    // ===== Row 6: Linear Gradients ==========================================
    add_label(batchers, "Linear Gradients", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let grad_w = 160.0;
    let grad_h = 50.0;
    let grad_spacing = 180.0;

    // Horizontal gradient (red -> blue)
    {
        let gx = LEFT_MARGIN;
        batchers.effects.add_linear_gradient(LinearGradientInstance {
            bounds: [gx, y, grad_w, grad_h],
            start: [gx, y + grad_h / 2.0],
            end: [gx + grad_w, y + grad_h / 2.0],
            stops: vec![make_stop(RED, 0.0), make_stop(BLUE, 1.0)],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    // Vertical gradient (green -> yellow)
    {
        let gx = LEFT_MARGIN + grad_spacing;
        let yellow: [f32; 4] = [0.95, 0.77, 0.06, 1.0];
        batchers.effects.add_linear_gradient(LinearGradientInstance {
            bounds: [gx, y, grad_w, grad_h],
            start: [gx + grad_w / 2.0, y],
            end: [gx + grad_w / 2.0, y + grad_h],
            stops: vec![make_stop(GREEN, 0.0), make_stop(yellow, 1.0)],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    // Diagonal gradient (purple -> orange) with rounded corners
    {
        let gx = LEFT_MARGIN + grad_spacing * 2.0;
        batchers.effects.add_linear_gradient(LinearGradientInstance {
            bounds: [gx, y, grad_w, grad_h],
            start: [gx, y],
            end: [gx + grad_w, y + grad_h],
            stops: vec![make_stop(PURPLE, 0.0), make_stop(ORANGE, 1.0)],
            corner_radii: [12.0; 4],
            transform: IDENTITY,
        });
    }

    // Multi-stop gradient with rounded corners
    {
        let gx = LEFT_MARGIN + grad_spacing * 3.0;
        batchers.effects.add_linear_gradient(LinearGradientInstance {
            bounds: [gx, y, grad_w, grad_h],
            start: [gx, y + grad_h / 2.0],
            end: [gx + grad_w, y + grad_h / 2.0],
            stops: vec![
                make_stop(RED, 0.0),
                make_stop(ORANGE, 0.25),
                make_stop(GREEN, 0.5),
                make_stop(BLUE, 0.75),
                make_stop(PURPLE, 1.0),
            ],
            corner_radii: [8.0; 4],
            transform: IDENTITY,
        });
    }

    y += grad_h + SECTION_GAP;

    // ===== Row 7: Radial Gradients ==========================================
    add_label(batchers, "Radial Gradients", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let radial_size = 70.0;
    let radial_spacing = 120.0;

    // Center-out gradient (white -> dark blue)
    {
        let gx = LEFT_MARGIN;
        let dark_blue: [f32; 4] = [0.05, 0.10, 0.40, 1.0];
        batchers.effects.add_radial_gradient(RadialGradientInstance {
            bounds: [gx, y, radial_size, radial_size],
            center: [gx + radial_size / 2.0, y + radial_size / 2.0],
            radius: radial_size / 2.0,
            stops: vec![make_stop(WHITE, 0.0), make_stop(dark_blue, 1.0)],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    // Off-center gradient
    {
        let gx = LEFT_MARGIN + radial_spacing;
        batchers.effects.add_radial_gradient(RadialGradientInstance {
            bounds: [gx, y, radial_size, radial_size],
            center: [gx + radial_size * 0.3, y + radial_size * 0.3],
            radius: radial_size * 0.6,
            stops: vec![make_stop(WHITE, 0.0), make_stop(RED, 1.0)],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    // Multi-stop rainbow radial
    {
        let gx = LEFT_MARGIN + radial_spacing * 2.0;
        batchers.effects.add_radial_gradient(RadialGradientInstance {
            bounds: [gx, y, radial_size, radial_size],
            center: [gx + radial_size / 2.0, y + radial_size / 2.0],
            radius: radial_size / 2.0,
            stops: vec![
                make_stop(RED, 0.0),
                make_stop(ORANGE, 0.2),
                make_stop([0.95, 0.77, 0.06, 1.0], 0.35),
                make_stop(GREEN, 0.5),
                make_stop(BLUE, 0.7),
                make_stop(PURPLE, 1.0),
            ],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    y += radial_size + SECTION_GAP;

    // ===== Row 8: Sweep Gradients ===========================================
    add_label(batchers, "Sweep Gradients", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let sweep_size = 70.0;

    // Full rainbow sweep (0 -> 360)
    {
        let gx = LEFT_MARGIN;
        batchers.effects.add_sweep_gradient(SweepGradientInstance {
            bounds: [gx, y, sweep_size, sweep_size],
            center: [gx + sweep_size / 2.0, y + sweep_size / 2.0],
            start_angle: 0.0,
            end_angle: std::f32::consts::TAU,
            stops: vec![
                make_stop(RED, 0.0),
                make_stop(ORANGE, 0.17),
                make_stop([0.95, 0.77, 0.06, 1.0], 0.33),
                make_stop(GREEN, 0.50),
                make_stop(BLUE, 0.67),
                make_stop(PURPLE, 0.83),
                make_stop(RED, 1.0),
            ],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    // Partial sweep (90 -> 270)
    {
        let gx = LEFT_MARGIN + 120.0;
        batchers.effects.add_sweep_gradient(SweepGradientInstance {
            bounds: [gx, y, sweep_size, sweep_size],
            center: [gx + sweep_size / 2.0, y + sweep_size / 2.0],
            start_angle: std::f32::consts::FRAC_PI_2,
            end_angle: std::f32::consts::FRAC_PI_2 * 3.0,
            stops: vec![
                make_stop(TEAL, 0.0),
                make_stop(PURPLE, 0.5),
                make_stop(ORANGE, 1.0),
            ],
            corner_radii: [0.0; 4],
            transform: IDENTITY,
        });
    }

    y += sweep_size + SECTION_GAP;

    // ===== Row 9: Shadows ===================================================
    add_label(batchers, "Shadows", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    let shadow_rect_w = 120.0;
    let shadow_rect_h = 50.0;

    // Soft shadow
    {
        let sx = LEFT_MARGIN;
        batchers.effects.add_shadow(ShadowInstance {
            bounds: [sx, y, shadow_rect_w, shadow_rect_h],
            color: [0.0, 0.0, 0.0, 0.35],
            offset: [4.0, 4.0],
            blur_radius: 12.0,
            spread: 2.0,
        });
        batchers.shapes.add_rect(
            sx, y, shadow_rect_w, shadow_rect_h,
            WHITE,
            [8.0; 4],
            IDENTITY,
        );
    }

    // Hard shadow
    {
        let sx = LEFT_MARGIN + 180.0;
        batchers.effects.add_shadow(ShadowInstance {
            bounds: [sx, y, shadow_rect_w, shadow_rect_h],
            color: [0.0, 0.0, 0.0, 0.6],
            offset: [3.0, 3.0],
            blur_radius: 1.0,
            spread: 0.0,
        });
        batchers.shapes.add_rect(
            sx, y, shadow_rect_w, shadow_rect_h,
            WHITE,
            [4.0; 4],
            IDENTITY,
        );
    }

    // Colored shadow (blue)
    {
        let sx = LEFT_MARGIN + 360.0;
        batchers.effects.add_shadow(ShadowInstance {
            bounds: [sx, y, shadow_rect_w, shadow_rect_h],
            color: [0.13, 0.59, 0.95, 0.45],
            offset: [0.0, 6.0],
            blur_radius: 16.0,
            spread: 4.0,
        });
        batchers.shapes.add_rect(
            sx, y, shadow_rect_w, shadow_rect_h,
            WHITE,
            [12.0; 4],
            IDENTITY,
        );
    }

    // Colored shadow (red)
    {
        let sx = LEFT_MARGIN + 540.0;
        batchers.effects.add_shadow(ShadowInstance {
            bounds: [sx, y, shadow_rect_w, shadow_rect_h],
            color: [0.91, 0.30, 0.24, 0.40],
            offset: [5.0, 5.0],
            blur_radius: 10.0,
            spread: 2.0,
        });
        batchers.shapes.add_rect(
            sx, y, shadow_rect_w, shadow_rect_h,
            WHITE,
            [8.0; 4],
            IDENTITY,
        );
    }

    y += shadow_rect_h + 10.0 + SECTION_GAP;

    // ===== Row 10: Text Rendering ===========================================
    add_label(batchers, "Text Rendering", LEFT_MARGIN, y);
    y += LABEL_HEIGHT + LABEL_TO_CONTENT_GAP;

    // Large title (32px)
    {
        let text = "flui-engine Feature Showcase";
        let key = TextCacheKey::new(text, 32.0, "sans-serif", 400);
        batchers.text.add_run(
            key,
            text.into(),
            "sans-serif".into(),
            [LEFT_MARGIN, y],
            BLACK,
            None,
        );
    }
    y += 38.0;

    // Medium body text (18px)
    {
        let text = "GPU-accelerated rendering with wgpu - shapes, paths, gradients, shadows, and text";
        let key = TextCacheKey::new(text, 18.0, "sans-serif", 400);
        batchers.text.add_run(
            key,
            text.into(),
            "sans-serif".into(),
            [LEFT_MARGIN, y],
            [0.3, 0.3, 0.3, 1.0],
            None,
        );
    }
    y += 24.0;

    // Small caption (12px)
    {
        let text = "Caption text at 12px - fine details and metadata";
        let key = TextCacheKey::new(text, 12.0, "sans-serif", 400);
        batchers.text.add_run(
            key,
            text.into(),
            "sans-serif".into(),
            [LEFT_MARGIN, y],
            [0.5, 0.5, 0.5, 1.0],
            None,
        );
    }
    y += 18.0;

    // Bold text (700 weight)
    {
        let text = "Bold text at 700 weight";
        let key = TextCacheKey::new(text, 22.0, "sans-serif", 700);
        batchers.text.add_run(
            key,
            text.into(),
            "sans-serif".into(),
            [LEFT_MARGIN, y],
            BLUE,
            None,
        );
    }

    // Colored text samples on the right side
    {
        let colored_texts: &[(&str, [f32; 4])] = &[
            ("Red text", RED),
            ("Green text", GREEN),
            ("Purple text", PURPLE),
            ("Orange text", ORANGE),
        ];
        let mut ty = y - 42.0;
        for (text, color) in colored_texts {
            let key = TextCacheKey::new(text, 16.0, "sans-serif", 400);
            batchers.text.add_run(
                key,
                (*text).into(),
                "sans-serif".into(),
                [LEFT_MARGIN + 600.0, ty],
                *color,
                None,
            );
            ty += 22.0;
        }
    }

    frame.finish()?;
    Ok(())
}

fn main() {
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
