//! Hello World - Minimal FLUI application

use flui_platform::{WindowOptions, current_platform};
use flui_types::geometry::{Size, px};

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("FLUI Hello World!");
    tracing::info!("Platform: {}", std::env::consts::OS);

    let platform = current_platform().expect("Failed to initialize platform");
    tracing::info!("Platform initialized: {:?}", platform.name());

    // Display enumeration and window creation both happen inside `on_ready`
    // rather than before `run()`: the winit backend can only enumerate real
    // displays and create windows once its event loop is pumping (calling
    // either earlier would either report zero displays or hang forever
    // waiting for a loop that hasn't started).
    platform.run(Box::new(|platform| {
        let displays = platform.displays();
        tracing::info!("Found {} display(s):", displays.len());
        for (i, disp) in displays.iter().enumerate() {
            tracing::info!(
                "  Display {}: {} ({}x{} @ {:.1}x scale)",
                i + 1,
                disp.name(),
                disp.bounds().size.width,
                disp.bounds().size.height,
                disp.scale_factor()
            );
        }

        tracing::info!("Creating window...");

        let window_options = WindowOptions {
            title: "Hello FLUI!".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            resizable: true,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
        };

        let window = platform
            .open_window(window_options)
            .expect("Failed to create window");
        tracing::info!("Window created successfully!");
        tracing::info!("   Logical size: {:?}", window.logical_size());
        tracing::info!("   Physical size: {:?}", window.physical_size());
        tracing::info!("   Scale factor: {:.1}x", window.scale_factor());

        tracing::info!("Platform ready, window is open");
        // The platform's own window registry — not this handle — keeps the
        // OS window alive; dropping `window` here just releases this handle.
        let _window = window;
    }));

    tracing::info!("Application finished!");
}
