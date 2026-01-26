//! Display Enumeration Example
//!
//! Demonstrates how to enumerate displays (monitors) and query their properties.
//!
//! Run with: `cargo run -p flui-platform --example displays`

use flui_platform::{current_platform, PlatformDisplay};
use tracing_subscriber;

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("🖥️  Display Enumeration Example");
    tracing::info!("════════════════════════════════════════");

    // Get platform
    let platform = current_platform()?;
    tracing::info!("Platform: {}", platform.name());

    // Enumerate displays
    let displays = platform.displays();
    tracing::info!("Found {} display(s)\n", displays.len());

    if displays.is_empty() {
        tracing::warn!("No displays found (headless mode?)");
        return Ok(());
    }

    // Display detailed information for each monitor
    for (idx, display) in displays.iter().enumerate() {
        let display_num = idx + 1;
        let id = display.id();
        let name = display.name();
        let is_primary = display.is_primary();
        let bounds = display.bounds();
        let usable_bounds = display.usable_bounds();
        let scale = display.scale_factor();
        let refresh = display.refresh_rate();
        let logical_size = display.logical_size();

        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!(
            "Display {}: {}{}",
            display_num,
            name,
            if is_primary { " (PRIMARY)" } else { "" }
        );
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("  ID: {:?}", id);

        // Physical (device pixel) information
        tracing::info!("  Physical Resolution: {}x{} pixels", bounds.size.width.0, bounds.size.height.0);
        tracing::info!(
            "  Physical Position: ({}, {})",
            bounds.origin.x.0,
            bounds.origin.y.0
        );

        // Logical (DPI-independent) information
        tracing::info!(
            "  Logical Size: {:.0}x{:.0} pt",
            logical_size.width.0,
            logical_size.height.0
        );

        // DPI scaling
        tracing::info!("  Scale Factor: {}x", scale);
        if scale >= 2.0 {
            tracing::info!("    → HiDPI/Retina display");
        } else if scale >= 1.5 {
            tracing::info!("    → High DPI display");
        } else {
            tracing::info!("    → Standard DPI display");
        }

        // Effective DPI calculation
        let effective_dpi = if cfg!(target_os = "windows") {
            96.0 * scale
        } else {
            72.0 * scale
        };
        tracing::info!("    → Effective DPI: {:.0}", effective_dpi);

        // Refresh rate
        tracing::info!("  Refresh Rate: {:.0} Hz", refresh);

        // Usable area (excluding taskbar/menu bar)
        let taskbar_width = bounds.size.width.0 - usable_bounds.size.width.0;
        let taskbar_height = bounds.size.height.0 - usable_bounds.size.height.0;

        if taskbar_width > 0 || taskbar_height > 0 {
            tracing::info!(
                "  Usable Area: {}x{} pixels",
                usable_bounds.size.width.0,
                usable_bounds.size.height.0
            );
            tracing::info!(
                "    → System UI takes {}x{} pixels",
                taskbar_width,
                taskbar_height
            );
        } else {
            tracing::info!("  Usable Area: Same as physical resolution");
        }

        // Display capabilities
        let diagonal_inches = calculate_diagonal_inches(
            bounds.size.width.0 as f64,
            bounds.size.height.0 as f64,
            effective_dpi,
        );
        tracing::info!("  Estimated Size: {:.1}\"", diagonal_inches);

        let aspect_ratio = bounds.size.width.0 as f64 / bounds.size.height.0 as f64;
        tracing::info!("  Aspect Ratio: {:.2}:1", aspect_ratio);

        // Common aspect ratios
        if (aspect_ratio - 16.0 / 9.0).abs() < 0.01 {
            tracing::info!("    → 16:9 (widescreen)");
        } else if (aspect_ratio - 16.0 / 10.0).abs() < 0.01 {
            tracing::info!("    → 16:10 (widescreen)");
        } else if (aspect_ratio - 4.0 / 3.0).abs() < 0.01 {
            tracing::info!("    → 4:3 (standard)");
        } else if (aspect_ratio - 21.0 / 9.0).abs() < 0.01 {
            tracing::info!("    → 21:9 (ultrawide)");
        }

        println!(); // Blank line between displays
    }

    // Multi-monitor configuration analysis
    if displays.len() > 1 {
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("Multi-Monitor Configuration");
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Calculate total desktop area
        let (min_x, max_x, min_y, max_y) = displays.iter().fold(
            (i32::MAX, i32::MIN, i32::MAX, i32::MIN),
            |(min_x, max_x, min_y, max_y), d| {
                let bounds = d.bounds();
                (
                    min_x.min(bounds.origin.x.0),
                    max_x.max(bounds.origin.x.0 + bounds.size.width.0),
                    min_y.min(bounds.origin.y.0),
                    max_y.max(bounds.origin.y.0 + bounds.size.height.0),
                )
            },
        );

        let total_width = max_x - min_x;
        let total_height = max_y - min_y;

        tracing::info!("  Total Desktop Area: {}x{} pixels", total_width, total_height);
        tracing::info!("  Bounds: ({}, {}) to ({}, {})", min_x, min_y, max_x, max_y);

        // Analyze arrangement
        let horizontal_arrangement = displays.iter().all(|d| {
            let bounds = d.bounds();
            bounds.origin.y.0 == 0
        });

        let vertical_arrangement = displays.iter().all(|d| {
            let bounds = d.bounds();
            bounds.origin.x.0 == 0
        });

        if horizontal_arrangement {
            tracing::info!("  Arrangement: Horizontal (side-by-side)");
        } else if vertical_arrangement {
            tracing::info!("  Arrangement: Vertical (stacked)");
        } else {
            tracing::info!("  Arrangement: Mixed/Custom");
        }

        // Check for scale factor differences
        let scales: Vec<_> = displays.iter().map(|d| d.scale_factor()).collect();
        let min_scale = scales.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_scale = scales.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        if (max_scale - min_scale).abs() > 0.1 {
            tracing::warn!(
                "  ⚠️  Mixed DPI setup detected ({}x to {}x)",
                min_scale,
                max_scale
            );
            tracing::warn!("    → Windows may appear at different sizes when moved between displays");
            tracing::warn!("    → Consider handling WindowEvent::ScaleFactorChanged");
        }
    }

    tracing::info!("════════════════════════════════════════");
    tracing::info!("✓ Display enumeration complete");

    Ok(())
}

/// Calculate estimated diagonal size in inches
fn calculate_diagonal_inches(width_px: f64, height_px: f64, dpi: f64) -> f64 {
    let width_inches = width_px / dpi;
    let height_inches = height_px / dpi;
    (width_inches * width_inches + height_inches * height_inches).sqrt()
}
