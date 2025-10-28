//! Full pipeline example - Scene -> Compositor -> Painter
//!
//! This example demonstrates the complete rendering pipeline:
//! 1. Build a scene with layers
//! 2. Composite the scene
//! 3. Paint to egui
//!
//! This showcases the typed architecture from idea.md

use flui_engine::{
    Scene, Compositor, CompositorOptions,
    PictureLayer, OpacityLayer, TransformLayer, ContainerLayer,
    Paint,
};
#[cfg(feature = "egui")]
use flui_engine::backends::egui::EguiPainter;
use flui_types::{Size, Rect, Point, Offset};

/// Build a complex scene with multiple layers
fn build_demo_scene(viewport_size: Size) -> Scene {
    let mut scene = Scene::new(viewport_size);

    // Layer 1: Background
    let mut background = PictureLayer::new();
    background.draw_rect(
        Rect::from_xywh(0.0, 0.0, viewport_size.width, viewport_size.height),
        Paint {
            color: [0.95, 0.95, 0.98, 1.0], // Light gray background
            ..Default::default()
        }
    );
    scene.add_layer(Box::new(background));

    // Layer 2: Red square with opacity
    let mut red_square = PictureLayer::new();
    red_square.draw_rect(
        Rect::from_xywh(50.0, 50.0, 100.0, 100.0),
        Paint {
            color: [1.0, 0.0, 0.0, 1.0], // Red
            ..Default::default()
        }
    );
    let red_with_opacity = OpacityLayer::new(Box::new(red_square), 0.7);
    scene.add_layer(Box::new(red_with_opacity));

    // Layer 3: Blue circle with transform
    let mut blue_circle = PictureLayer::new();
    blue_circle.draw_circle(
        Point::new(50.0, 50.0), // Center relative to transform
        40.0,
        Paint {
            color: [0.0, 0.5, 1.0, 1.0], // Blue
            ..Default::default()
        }
    );
    let blue_transformed = TransformLayer::translate(
        Box::new(blue_circle),
        Offset::new(200.0, 100.0)
    );
    scene.add_layer(Box::new(blue_transformed));

    // Layer 4: Green outline rectangle
    let mut green_outline = PictureLayer::new();
    green_outline.draw_rect(
        Rect::from_xywh(100.0, 200.0, 150.0, 80.0),
        Paint {
            color: [0.0, 0.8, 0.2, 1.0], // Green
            stroke_width: 3.0,
            ..Default::default()
        }
    );
    scene.add_layer(Box::new(green_outline));

    // Layer 5: Complex composition - container with multiple children
    let mut container = ContainerLayer::new();

    // Child 1: Small yellow square
    let mut yellow_square = PictureLayer::new();
    yellow_square.draw_rect(
        Rect::from_xywh(350.0, 50.0, 60.0, 60.0),
        Paint {
            color: [1.0, 0.9, 0.0, 1.0], // Yellow
            ..Default::default()
        }
    );
    container.add_child(Box::new(yellow_square));

    // Child 2: Overlapping purple circle
    let mut purple_circle = PictureLayer::new();
    purple_circle.draw_circle(
        Point::new(380.0, 80.0),
        30.0,
        Paint {
            color: [0.7, 0.0, 0.7, 1.0], // Purple
            ..Default::default()
        }
    );
    container.add_child(Box::new(purple_circle));

    scene.add_layer(Box::new(container));

    scene
}

#[cfg(feature = "egui")]
fn main() {
    use eframe::egui;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("FLUI Engine - Full Pipeline Demo"),
        ..Default::default()
    };

    eframe::run_simple_native("FLUI Engine Demo", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let viewport_size = Size::new(
                ui.available_width(),
                ui.available_height()
            );

            // Build scene
            let mut scene = build_demo_scene(viewport_size);

            // Setup compositor
            let mut compositor = Compositor::with_options(CompositorOptions {
                enable_culling: true,
                viewport: Rect::from_xywh(0.0, 0.0, viewport_size.width, viewport_size.height),
                debug_mode: false,
                track_performance: true,
            });

            // Get egui painter
            let painter = ui.painter();

            // Create our painter wrapper
            let mut egui_painter = EguiPainter::new(painter);

            // Composite!
            compositor.composite(&scene, &mut egui_painter);

            // Update frame
            scene.next_frame();

            // Show stats
            ui.label(format!(
                "Frame: {} | Layers painted: {} | Time: {:?}",
                scene.metadata().frame_number,
                compositor.stats().layers_painted,
                compositor.stats().composition_time
            ));

            ui.label(format!(
                "Scene bounds: {:?}",
                scene.content_bounds()
            ));

            ui.label(format!(
                "Layers in scene: {}",
                scene.layer_count()
            ));

            // Request repaint for animation
            ctx.request_repaint();
        });
    }).expect("Failed to run egui app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    println!("This example requires the 'egui' feature to be enabled.");
    println!("Run with: cargo run --example full_pipeline --features egui");
}
