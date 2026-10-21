//! Window Features Demo
//!
//! Opens a native window and reports its display and geometry properties.
//!
//! This example shows:
//! - Cross-platform Window trait API
//! - Window creation options (resizable, title, size)
//! - Logical size, physical size, scale, visibility and focus
//!
//! Usage:
//! ```bash
//! cargo run --example window_features
//! ```

use flui_foundation::geometry::Size;
use flui_platform::{WindowOptions, current_platform};

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("FLUI Window Features Demo");
    tracing::info!("=========================");

    let platform = current_platform()?;
    tracing::info!("Platform: {}", platform.name());
    tracing::info!("OS: {}", std::env::consts::OS);

    // Display enumeration and window creation both move inside `on_ready`
    // (ADR-0039 §1): the winit backend can only enumerate real
    // displays and create windows once its event loop is pumping.
    platform.run(Box::new(move |owner| {
        let snapshot = owner.display_snapshot()?;
        let displays = snapshot.displays();
        tracing::info!("Displays:");
        for (i, disp) in displays.iter().enumerate() {
            tracing::info!(
                "  {}. {} - {}x{} @ {:.1}x",
                i + 1,
                disp.name,
                disp.bounds.size.width,
                disp.bounds.size.height,
                disp.scale_factor
            );
        }

        tracing::info!("Creating window...");
        let window_options = WindowOptions::default()
            .with_title("Window Features Demo".to_string())
            .with_size(Size::new(1000.0, 700.0))
            .with_resizable(true)
            .with_visible(true)
            .with_decorated(true)
            .with_min_size(Some(Size::new(600.0, 400.0)))
            .with_max_size(None);

        let window = owner
            .open_window(window_options)
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("window creation is always Ready inside on_ready");
        tracing::info!("Window created!");
        tracing::info!("  Logical size:  {:?}", window.logical_size());
        tracing::info!("  Physical size: {:?}", window.physical_size());
        tracing::info!("  Scale factor:  {:.1}x", window.scale_factor());
        tracing::info!("  Visible:       {}", window.is_visible());
        tracing::info!("  Focused:       {}", window.is_focused());

        tracing::info!("Platform ready, window is open");
        // The platform registry owns the native window after readiness.
        let _window = window;
        Ok(())
    }))?;

    tracing::info!("Demo finished!");
    Ok(())
}
