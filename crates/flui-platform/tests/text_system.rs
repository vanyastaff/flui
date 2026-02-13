//! Text system tests for flui-platform
//!
//! Tests platform text system integration using the new PlatformTextSystem API:
//! - all_font_names(), font_id(), font_metrics(), glyph_for_char(), layout_line()

use flui_platform::current_platform;
use flui_platform::traits::{Font, FontRun, FontStyle, FontWeight};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Initialize tracing for tests
fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

// ============================================================================
// Font enumeration
// ============================================================================

#[test]
fn test_all_font_names() {
    init_tracing();
    tracing::info!("Test: List all font names");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let names = text_system.all_font_names();
    tracing::info!("Found {} font families", names.len());

    assert!(!names.is_empty(), "Should find at least one font");

    #[cfg(windows)]
    assert!(
        names.iter().any(|n| n == "Segoe UI"),
        "Windows should have Segoe UI"
    );

    tracing::info!("PASS: Font enumeration works");
}

// ============================================================================
// Font resolution
// ============================================================================

#[test]
fn test_font_id_resolution() {
    init_tracing();
    tracing::info!("Test: Resolve font descriptor to FontId");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };

    let id = text_system.font_id(&font);
    assert!(id.is_ok(), "Failed to resolve Segoe UI: {:?}", id.err());

    // Same font should return same ID (cached)
    let id2 = text_system.font_id(&font).unwrap();
    assert_eq!(id.unwrap(), id2, "Same font should return same FontId");

    tracing::info!("PASS: Font resolution works");
}

#[test]
fn test_font_not_found() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "NonExistentFontFamily12345".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };

    let result = text_system.font_id(&font);
    assert!(result.is_err(), "Non-existent font should return error");

    tracing::info!("PASS: Non-existent font returns error");
}

// ============================================================================
// Font metrics
// ============================================================================

#[test]
fn test_font_metrics() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();
    let metrics = text_system.font_metrics(id);

    assert!(metrics.units_per_em > 0, "units_per_em should be > 0");
    assert!(metrics.ascent > 0.0, "ascent should be > 0");
    assert!(metrics.descent > 0.0, "descent should be > 0");
    assert!(metrics.cap_height > 0.0, "cap_height should be > 0");
    assert!(metrics.x_height > 0.0, "x_height should be > 0");

    tracing::info!(
        "PASS: Font metrics - em={}, ascent={}, descent={}",
        metrics.units_per_em,
        metrics.ascent,
        metrics.descent
    );
}

// ============================================================================
// Glyph lookup
// ============================================================================

#[test]
fn test_glyph_for_char() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    // ASCII 'A' should have a glyph
    let glyph = text_system.glyph_for_char(id, 'A');
    assert!(glyph.is_some(), "Expected glyph for 'A'");
    assert!(glyph.unwrap().0 > 0, "Glyph ID should be > 0");

    // Space should have a glyph
    let space_glyph = text_system.glyph_for_char(id, ' ');
    assert!(space_glyph.is_some(), "Expected glyph for space");

    tracing::info!("PASS: Glyph lookup works");
}

// ============================================================================
// Text layout
// ============================================================================

#[test]
fn test_layout_line_ascii() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let text = "Hello, World!";
    let layout = text_system.layout_line(
        text,
        16.0,
        &[FontRun {
            font_id: id,
            len: text.len(),
        }],
    );

    assert!(
        layout.width > 0.0,
        "Width should be > 0, got {}",
        layout.width
    );
    assert!(layout.ascent > 0.0, "Ascent should be > 0");
    assert!(layout.descent > 0.0, "Descent should be > 0");
    assert_eq!(layout.len, text.len());
    assert_eq!(layout.font_size, 16.0);

    tracing::info!(
        "PASS: ASCII layout - width={:.1}, ascent={:.1}, descent={:.1}",
        layout.width,
        layout.ascent,
        layout.descent
    );
}

#[test]
fn test_layout_line_empty() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let layout = text_system.layout_line("", 16.0, &[]);
    assert_eq!(layout.width, 0.0);
    assert_eq!(layout.len, 0);

    tracing::info!("PASS: Empty text layout works");
}

#[test]
fn test_layout_line_unicode() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    // Cyrillic
    let text = "Привет мир";
    let layout = text_system.layout_line(
        text,
        16.0,
        &[FontRun {
            font_id: id,
            len: text.len(),
        }],
    );
    assert!(layout.width > 0.0, "Cyrillic layout width should be > 0");

    tracing::info!("PASS: Unicode layout - Cyrillic width={:.1}", layout.width);
}

// ============================================================================
// Performance test
// ============================================================================

#[test]
fn test_text_layout_performance() {
    init_tracing();

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let test_strings = [
        "Short",
        "Medium length text string",
        "This is a much longer text string that contains significantly more characters to measure",
    ];

    for text in test_strings {
        let start = std::time::Instant::now();
        let layout = text_system.layout_line(
            text,
            16.0,
            &[FontRun {
                font_id: id,
                len: text.len(),
            }],
        );
        let elapsed = start.elapsed();

        tracing::info!(
            "Measured '{}' ({} chars, width={:.1}) in {:.3}ms",
            &text.chars().take(20).collect::<String>(),
            text.chars().count(),
            layout.width,
            elapsed.as_secs_f64() * 1000.0
        );

        assert!(
            elapsed.as_millis() < 50,
            "Text layout should be fast (<50ms)"
        );
    }

    tracing::info!("PASS: Text layout performance verified");
}
