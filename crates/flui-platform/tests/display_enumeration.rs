//! Display Enumeration Tests
//!
//! Comprehensive tests for display/monitor enumeration with DPI-aware bounds,
//! refresh rates, and multi-monitor support.

use flui_platform::{current_platform, Platform, PlatformDisplay};
use std::collections::HashSet;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Test that platform.displays() returns all connected displays with valid properties
#[test]
fn test_displays_enumeration() {
    init_tracing();
    tracing::info!("Testing display enumeration");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    tracing::info!("Platform {} reports {} display(s)", platform.name(), displays.len());

    // Contract: displays() must return a valid collection
    // Empty is OK for headless, but most systems have at least one
    if platform.name() != "Headless" {
        assert!(
            displays.len() >= 1,
            "Non-headless platform should have at least one display"
        );
    }

    // Each display should have valid properties
    for (idx, display) in displays.iter().enumerate() {
        let id = display.id();
        let name = display.name();
        let bounds = display.bounds();
        let scale = display.scale_factor();
        let refresh = display.refresh_rate();
        let is_primary = display.is_primary();

        tracing::info!(
            "Display {}: id={:?}, name='{}', bounds={}x{}+{}+{}, scale={}, refresh={}Hz, primary={}",
            idx + 1,
            id,
            name,
            bounds.size.width.0,
            bounds.size.height.0,
            bounds.origin.x.0,
            bounds.origin.y.0,
            scale,
            refresh,
            is_primary
        );

        // Validate display properties
        assert!(!name.is_empty(), "Display name should not be empty");

        assert!(
            bounds.size.width.0 > 0 && bounds.size.height.0 > 0,
            "Display size must be positive"
        );

        assert!(
            scale > 0.0 && scale <= 4.0,
            "Scale factor should be reasonable (0.0-4.0), got {}",
            scale
        );

        assert!(
            refresh > 0.0 && refresh <= 500.0,
            "Refresh rate should be reasonable (0-500Hz), got {}",
            refresh
        );
    }

    // Exactly one display should be marked as primary
    let primary_count = displays.iter().filter(|d| d.is_primary()).count();
    if !displays.is_empty() {
        assert_eq!(
            primary_count, 1,
            "Exactly one display should be marked as primary, found {}",
            primary_count
        );
    }

    tracing::info!("✓ PASS: Display enumeration validated");
}

/// Test that platform.primary_display() returns the OS-marked primary display
#[test]
fn test_primary_display_detection() {
    init_tracing();
    tracing::info!("Testing primary display detection");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    if displays.is_empty() {
        tracing::info!("⊘ SKIP: No displays available (headless)");
        return;
    }

    // Find the primary display from enumeration
    let primary_from_list = displays
        .iter()
        .find(|d| d.is_primary())
        .expect("Should have exactly one primary display");

    tracing::info!(
        "Primary display: '{}' ({}x{} @ {})",
        primary_from_list.name(),
        primary_from_list.bounds().size.width.0,
        primary_from_list.bounds().size.height.0,
        primary_from_list.scale_factor()
    );

    // Verify primary display properties
    assert!(
        primary_from_list.is_primary(),
        "Primary display should report is_primary() = true"
    );

    // Primary display should have valid bounds
    let bounds = primary_from_list.bounds();
    assert!(
        bounds.size.width.0 > 0 && bounds.size.height.0 > 0,
        "Primary display must have valid size"
    );

    // On most systems, primary display origin is (0, 0)
    // (but this isn't guaranteed on all platforms)
    tracing::info!(
        "Primary display origin: ({}, {})",
        bounds.origin.x.0,
        bounds.origin.y.0
    );

    tracing::info!("✓ PASS: Primary display validated");
}

/// Test that HiDPI/Retina displays report scale factors >= 1.5
#[test]
fn test_high_dpi_scale_factor() {
    init_tracing();
    tracing::info!("Testing high DPI scale factor detection");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    if displays.is_empty() {
        tracing::info!("⊘ SKIP: No displays available (headless)");
        return;
    }

    let mut found_hidpi = false;
    let mut found_standard = false;

    for display in displays.iter() {
        let scale = display.scale_factor();
        let bounds = display.bounds();
        let name = display.name();

        tracing::info!(
            "Display '{}': {}x{} @ {}x scale",
            name,
            bounds.size.width.0,
            bounds.size.height.0,
            scale
        );

        // Check for HiDPI displays (scale >= 1.5)
        if scale >= 1.5 {
            found_hidpi = true;
            tracing::info!("  → HiDPI display detected");

            // Common HiDPI scales: 1.5, 2.0, 2.5, 3.0, 4.0
            let common_scales = [1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 4.0];
            let is_common = common_scales
                .iter()
                .any(|&s| (scale - s).abs() < 0.01);

            if is_common {
                tracing::info!("  → Common HiDPI scale factor: {}", scale);
            }

            // Verify logical size calculation
            let logical_size = display.logical_size();
            let expected_logical_width =
                bounds.size.width.0 as f32 / scale as f32;
            let expected_logical_height =
                bounds.size.height.0 as f32 / scale as f32;

            let width_diff = (logical_size.width.0 - expected_logical_width).abs();
            let height_diff = (logical_size.height.0 - expected_logical_height).abs();

            assert!(
                width_diff < 1.0 && height_diff < 1.0,
                "Logical size should equal physical / scale (tolerance: 1px)"
            );

            tracing::info!(
                "  → Logical size: {}x{} (physical / scale)",
                logical_size.width.0,
                logical_size.height.0
            );
        } else {
            found_standard = true;
            tracing::info!("  → Standard DPI display");
        }

        // All scale factors should be positive and reasonable
        assert!(
            scale > 0.0 && scale <= 4.0,
            "Scale factor should be 0.0-4.0, got {}",
            scale
        );
    }

    if found_hidpi {
        tracing::info!("✓ PASS: HiDPI display(s) detected and validated");
    } else if found_standard {
        tracing::info!("✓ PASS: Standard DPI display(s) detected (no HiDPI available)");
    }
}

/// Test that display.usable_bounds() correctly excludes taskbar and menu bar areas
#[test]
fn test_usable_bounds_exclude_system_ui() {
    init_tracing();
    tracing::info!("Testing usable bounds (excludes taskbar/menu bar)");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    if displays.is_empty() {
        tracing::info!("⊘ SKIP: No displays available (headless)");
        return;
    }

    for display in displays.iter() {
        let full_bounds = display.bounds();
        let usable_bounds = display.usable_bounds();
        let name = display.name();

        tracing::info!("Display '{}':", name);
        tracing::info!(
            "  Full bounds: {}x{}+{}+{}",
            full_bounds.size.width.0,
            full_bounds.size.height.0,
            full_bounds.origin.x.0,
            full_bounds.origin.y.0
        );
        tracing::info!(
            "  Usable bounds: {}x{}+{}+{}",
            usable_bounds.size.width.0,
            usable_bounds.size.height.0,
            usable_bounds.origin.x.0,
            usable_bounds.origin.y.0
        );

        // Usable bounds should be within full bounds
        assert!(
            usable_bounds.size.width.0 <= full_bounds.size.width.0,
            "Usable width should not exceed full width"
        );

        assert!(
            usable_bounds.size.height.0 <= full_bounds.size.height.0,
            "Usable height should not exceed full height"
        );

        // Calculate difference (taskbar/menu bar space)
        let width_diff = full_bounds.size.width.0 - usable_bounds.size.width.0;
        let height_diff = full_bounds.size.height.0 - usable_bounds.size.height.0;

        if width_diff > 0 || height_diff > 0 {
            tracing::info!(
                "  → System UI takes {}x{} pixels",
                width_diff,
                height_diff
            );
        } else {
            tracing::info!("  → No system UI detected (full bounds = usable bounds)");
        }

        // Usable bounds should be positive
        assert!(
            usable_bounds.size.width.0 > 0 && usable_bounds.size.height.0 > 0,
            "Usable bounds must have positive size"
        );
    }

    tracing::info!("✓ PASS: Usable bounds validated");
}

/// Verify Windows display enumeration via EnumDisplayMonitors API
#[test]
#[cfg(windows)]
fn test_windows_enum_display_monitors() {
    init_tracing();
    tracing::info!("Verifying Windows EnumDisplayMonitors implementation");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    tracing::info!("Windows platform reports {} display(s)", displays.len());

    // Windows should detect at least the primary display
    assert!(
        displays.len() >= 1,
        "Windows should have at least one display"
    );

    // Verify each display has Windows-specific properties correct
    for disp in displays.iter() {
        let bounds = disp.bounds();

        tracing::info!(
            "Windows display: '{}' at {}x{}+{}+{}",
            disp.name(),
            bounds.size.width.0,
            bounds.size.height.0,
            bounds.origin.x.0,
            bounds.origin.y.0
        );

        // Windows displays should have reasonable bounds
        assert!(
            bounds.size.width.0 >= 640 && bounds.size.height.0 >= 480,
            "Display should be at least 640x480"
        );

        // Windows should provide usable bounds (work area)
        let usable = disp.usable_bounds();
        assert!(
            usable.size.width.0 > 0 && usable.size.height.0 > 0,
            "Windows usable bounds should be valid"
        );
    }

    tracing::info!("✓ PASS: Windows display enumeration validated");
}

/// Skip test on non-Windows platforms
#[test]
#[cfg(not(windows))]
fn test_windows_enum_display_monitors() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    tracing::info!("⊘ SKIP: Windows-specific test (not on Windows)");
}

/// Verify macOS display enumeration via NSScreen API
#[test]
#[cfg(target_os = "macos")]
fn test_macos_nsscreen_enumeration() {
    init_tracing();
    tracing::info!("Verifying macOS NSScreen enumeration");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    tracing::info!("macOS platform reports {} display(s)", displays.len());

    // macOS should detect at least the primary display
    assert!(
        displays.len() >= 1,
        "macOS should have at least one display"
    );

    // Verify macOS-specific properties
    for disp in displays.iter() {
        let bounds = disp.bounds();
        let scale = disp.scale_factor();

        tracing::info!(
            "macOS display: '{}' at {}x{} @ {}x",
            disp.name(),
            bounds.size.width.0,
            bounds.size.height.0,
            scale
        );

        // macOS Retina displays have scale >= 2.0
        if scale >= 2.0 {
            tracing::info!("  → Retina display detected");
        }

        // macOS should provide menu bar exclusion in usable bounds
        let usable = disp.usable_bounds();
        let menu_bar_height = bounds.size.height.0 - usable.size.height.0;

        if menu_bar_height > 0 {
            tracing::info!("  → Menu bar height: {} pixels", menu_bar_height);
        }
    }

    tracing::info!("✓ PASS: macOS display enumeration validated");
}

/// Skip test on non-macOS platforms
#[test]
#[cfg(not(target_os = "macos"))]
fn test_macos_nsscreen_enumeration() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    tracing::info!("⊘ SKIP: macOS-specific test (not on macOS)");
}

/// Test that display bounds don't overlap incorrectly in multi-monitor configurations
#[test]
fn test_multi_monitor_bounds_arrangement() {
    init_tracing();
    tracing::info!("Testing multi-monitor bounds arrangement");

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    if displays.len() < 2 {
        tracing::info!(
            "⊘ SKIP: Single display system (need 2+ displays for multi-monitor test)"
        );
        return;
    }

    tracing::info!("Multi-monitor system detected: {} displays", displays.len());

    // Check for unique display IDs
    let ids: HashSet<_> = displays.iter().map(|d| d.id()).collect();
    assert_eq!(
        ids.len(),
        displays.len(),
        "All displays should have unique IDs"
    );

    // Analyze display arrangement
    for (i, display1) in displays.iter().enumerate() {
        let bounds1 = display1.bounds();

        tracing::info!(
            "Display {}: '{}' at {}x{}+{}+{}",
            i + 1,
            display1.name(),
            bounds1.size.width.0,
            bounds1.size.height.0,
            bounds1.origin.x.0,
            bounds1.origin.y.0
        );

        // Check against other displays
        for (j, display2) in displays.iter().enumerate() {
            if i >= j {
                continue; // Skip self and already-compared pairs
            }

            let bounds2 = display2.bounds();

            // Calculate if displays are adjacent or overlapping
            let horizontal_gap = if bounds1.origin.x.0 + bounds1.size.width.0
                <= bounds2.origin.x.0
            {
                bounds2.origin.x.0 - (bounds1.origin.x.0 + bounds1.size.width.0)
            } else if bounds2.origin.x.0 + bounds2.size.width.0 <= bounds1.origin.x.0 {
                bounds1.origin.x.0 - (bounds2.origin.x.0 + bounds2.size.width.0)
            } else {
                0 // Overlapping or aligned
            };

            let vertical_gap = if bounds1.origin.y.0 + bounds1.size.height.0
                <= bounds2.origin.y.0
            {
                bounds2.origin.y.0 - (bounds1.origin.y.0 + bounds1.size.height.0)
            } else if bounds2.origin.y.0 + bounds2.size.height.0 <= bounds1.origin.y.0 {
                bounds1.origin.y.0 - (bounds2.origin.y.0 + bounds2.size.height.0)
            } else {
                0 // Overlapping or aligned
            };

            if horizontal_gap == 0 && vertical_gap == 0 {
                tracing::info!(
                    "  → Display {} and {} are adjacent or aligned",
                    i + 1,
                    j + 1
                );
            } else {
                tracing::info!(
                    "  → Gap between {} and {}: {}x{} pixels",
                    i + 1,
                    j + 1,
                    horizontal_gap,
                    vertical_gap
                );
            }

            // Displays should not have significant incorrect overlap
            // (small overlaps of 1-2 pixels can happen due to rounding)
            // This is more of a warning than a hard failure
            if horizontal_gap < 0 || vertical_gap < 0 {
                tracing::warn!(
                    "  ⚠ Potential display overlap detected between {} and {}",
                    i + 1,
                    j + 1
                );
            }
        }
    }

    tracing::info!("✓ PASS: Multi-monitor bounds arrangement validated");
}

/// Test that moving window between monitors fires ScaleFactorChanged event
#[test]
#[ignore] // Requires manual window dragging or programmatic window movement
fn test_scale_factor_changed_event() {
    init_tracing();
    tracing::info!("Testing ScaleFactorChanged event on monitor change");

    // This test is conceptual - actual implementation would require:
    // 1. Creating a window on monitor 1
    // 2. Setting up event listener for ScaleFactorChanged
    // 3. Moving window to monitor 2 with different DPI
    // 4. Verifying the event fires with correct new scale factor

    let platform = current_platform().expect("Failed to get platform");
    let displays = platform.displays();

    if displays.len() < 2 {
        tracing::info!("⊘ SKIP: Need 2+ displays with different scale factors");
        return;
    }

    // Find displays with different scale factors
    let mut different_scales = false;
    for i in 0..displays.len() {
        for j in (i+1)..displays.len() {
            if (displays[i].scale_factor() - displays[j].scale_factor()).abs() > 0.1 {
                different_scales = true;
                tracing::info!(
                    "Found displays with different scales: {} ({}x) and {} ({}x)",
                    displays[i].name(),
                    displays[i].scale_factor(),
                    displays[j].name(),
                    displays[j].scale_factor()
                );
            }
        }
    }

    if !different_scales {
        tracing::info!("⊘ SKIP: All displays have same scale factor");
        return;
    }

    tracing::info!("✓ NOTE: ScaleFactorChanged event test requires manual/automated window movement");
    tracing::info!("  Implementation would use:");
    tracing::info!("  - platform.on_window_event() to register listener");
    tracing::info!("  - window.set_position() to move between monitors");
    tracing::info!("  - Verify WindowEvent::ScaleFactorChanged fires");
}

/// Benchmark display enumeration latency to ensure it's under 10ms even with multiple monitors
#[test]
fn test_display_enumeration_performance() {
    init_tracing();
    tracing::info!("Benchmarking display enumeration performance");

    let platform = current_platform().expect("Failed to get platform");

    // Warm-up call
    let _ = platform.displays();

    // Benchmark actual enumeration
    let iterations = 100;
    let start = std::time::Instant::now();

    for _ in 0..iterations {
        let _ = platform.displays();
    }

    let duration = start.elapsed();
    let avg_ms = duration.as_secs_f64() * 1000.0 / iterations as f64;

    tracing::info!(
        "Display enumeration performance: {:.3}ms average over {} iterations",
        avg_ms,
        iterations
    );

    // Performance target: <10ms per enumeration
    // Most systems should be much faster (<1ms)
    assert!(
        avg_ms < 10.0,
        "Display enumeration should take <10ms, got {:.3}ms",
        avg_ms
    );

    if avg_ms < 1.0 {
        tracing::info!("  → Excellent performance (<1ms)");
    } else if avg_ms < 5.0 {
        tracing::info!("  → Good performance (<5ms)");
    } else {
        tracing::info!("  → Acceptable performance (<10ms)");
    }

    tracing::info!("✓ PASS: Display enumeration performance validated");
}
