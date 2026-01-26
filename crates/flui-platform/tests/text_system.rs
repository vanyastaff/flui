//! Text system tests for flui-platform (Phase 4: T025-T028)
//!
//! Tests platform text system integration:
//! - T025: Load default font family
//! - T026: Measure ASCII text bounds
//! - T027: Measure text with emoji/CJK characters
//! - T028: Font fallback when family doesn't exist
//!
//! # MVP Status
//!
//! These tests verify the **stub implementation** that returns reasonable defaults.
//! Phase 2 will add real DirectWrite/Core Text integration.

use flui_platform::{current_platform, Platform};
use flui_types::geometry::px;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Initialize tracing for tests
fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

// ============================================================================
// T025: Load default font family returns platform font
// ============================================================================

#[test]
fn test_default_font_family() {
    init_tracing();
    tracing::info!("Test T025: Load default font family");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let default_font = text_system.default_font_family();
    tracing::info!("Platform: {}, Default font: {}", platform.name(), default_font);

    // Verify platform-specific default fonts
    #[cfg(windows)]
    assert_eq!(default_font, "Segoe UI", "Windows should use Segoe UI");

    #[cfg(target_os = "macos")]
    assert_eq!(default_font, "SF Pro Text", "macOS should use SF Pro Text");

    #[cfg(target_os = "linux")]
    assert_eq!(default_font, "Ubuntu", "Linux should use Ubuntu");

    // Verify font name is not empty
    assert!(!default_font.is_empty(), "Default font name should not be empty");

    tracing::info!("✓ T025: Default font family verified");
}

// ============================================================================
// T026: Measure text bounds for ASCII string with font size 16pt
// ============================================================================

#[test]
fn test_measure_ascii_text() {
    init_tracing();
    tracing::info!("Test T026: Measure ASCII text bounds");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let text = "Hello, World!";
    let font_family = text_system.default_font_family();
    let font_size = 16.0;

    let bounds = text_system.measure_text(text, &font_family, font_size);

    tracing::info!(
        "Text: '{}', Font: {}, Size: {}pt",
        text,
        font_family,
        font_size
    );
    tracing::info!(
        "Bounds: width={:.2}px, height={:.2}px",
        bounds.width(),
        bounds.height()
    );

    // Verify bounds are reasonable (MVP approximation)
    // 13 characters * 16pt * 0.6 ≈ 124.8px width
    // 16pt * 1.2 ≈ 19.2px height
    assert!(
        bounds.width() > px(100.0),
        "Text width should be > 100px, got {}px",
        bounds.width()
    );
    assert!(
        bounds.width() < px(200.0),
        "Text width should be < 200px, got {}px",
        bounds.width()
    );
    assert!(
        bounds.height() > px(15.0),
        "Text height should be > 15px, got {}px",
        bounds.height()
    );
    assert!(
        bounds.height() < px(30.0),
        "Text height should be < 30px, got {}px",
        bounds.height()
    );

    tracing::info!("✓ T026: ASCII text measurement verified");
}

// ============================================================================
// T027: Measure text with emoji/CJK characters returns correct width
// ============================================================================

#[test]
fn test_measure_unicode_text() {
    init_tracing();
    tracing::info!("Test T027: Measure text with emoji/CJK characters");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();
    let font_family = text_system.default_font_family();
    let font_size = 16.0;

    // Test 1: Emoji
    let emoji_text = "Hello 👋 World 🌍!";
    let emoji_bounds = text_system.measure_text(emoji_text, &font_family, font_size);
    tracing::info!(
        "Emoji text: '{}' → width={:.2}px, height={:.2}px",
        emoji_text,
        emoji_bounds.width(),
        emoji_bounds.height()
    );

    // Test 2: CJK (Chinese/Japanese/Korean)
    let cjk_text = "你好世界"; // "Hello World" in Chinese
    let cjk_bounds = text_system.measure_text(cjk_text, &font_family, font_size);
    tracing::info!(
        "CJK text: '{}' → width={:.2}px, height={:.2}px",
        cjk_text,
        cjk_bounds.width(),
        cjk_bounds.height()
    );

    // Test 3: Cyrillic
    let cyrillic_text = "Привет мир"; // "Hello World" in Russian
    let cyrillic_bounds = text_system.measure_text(cyrillic_text, &font_family, font_size);
    tracing::info!(
        "Cyrillic text: '{}' → width={:.2}px, height={:.2}px",
        cyrillic_text,
        cyrillic_bounds.width(),
        cyrillic_bounds.height()
    );

    // Test 4: Arabic (RTL text)
    let arabic_text = "مرحبا بالعالم"; // "Hello World" in Arabic
    let arabic_bounds = text_system.measure_text(arabic_text, &font_family, font_size);
    tracing::info!(
        "Arabic text: '{}' → width={:.2}px, height={:.2}px",
        arabic_text,
        arabic_bounds.width(),
        arabic_bounds.height()
    );

    // Verify all measurements return reasonable bounds
    for (name, bounds) in [
        ("emoji", emoji_bounds),
        ("CJK", cjk_bounds),
        ("Cyrillic", cyrillic_bounds),
        ("Arabic", arabic_bounds),
    ] {
        assert!(
            bounds.width() > px(0.0),
            "{} text width should be positive",
            name
        );
        assert!(
            bounds.height() > px(0.0),
            "{} text height should be positive",
            name
        );
        assert!(
            bounds.width() < px(1000.0),
            "{} text width should be reasonable",
            name
        );
        assert!(
            bounds.height() < px(100.0),
            "{} text height should be reasonable",
            name
        );
    }

    tracing::info!("✓ T027: Unicode text measurement verified (emoji, CJK, Cyrillic, Arabic)");
}

// ============================================================================
// T028: Font fallback when requested family doesn't exist
// ============================================================================

#[test]
fn test_font_fallback() {
    init_tracing();
    tracing::info!("Test T028: Font fallback for non-existent family");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    // Try to load a font that definitely doesn't exist
    let nonexistent_font = "NonExistentFontFamily12345";
    let result = text_system.load_system_font(nonexistent_font);

    tracing::info!("Attempted to load font: '{}'", nonexistent_font);
    tracing::info!("Result: {:?}", result);

    // MVP: Should return NotImplemented error
    assert!(
        result.is_err(),
        "Loading non-existent font should return error"
    );

    // Verify we can still measure text with non-existent font
    // (should fall back to default font approximation)
    let text = "Test text";
    let font_size = 16.0;
    let bounds = text_system.measure_text(text, nonexistent_font, font_size);

    tracing::info!(
        "Text measurement with non-existent font: width={:.2}px, height={:.2}px",
        bounds.width(),
        bounds.height()
    );

    // Should still return reasonable bounds (fallback behavior)
    assert!(
        bounds.width() > px(0.0),
        "Fallback measurement should return positive width"
    );
    assert!(
        bounds.height() > px(0.0),
        "Fallback measurement should return positive height"
    );

    tracing::info!("✓ T028: Font fallback behavior verified");
}

// ============================================================================
// Additional test: Enumerate system fonts
// ============================================================================

#[test]
fn test_enumerate_system_fonts() {
    init_tracing();
    tracing::info!("Additional test: Enumerate system fonts");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let fonts = text_system.enumerate_system_fonts();
    tracing::info!("Available fonts ({}): {:?}", fonts.len(), fonts);

    // MVP: Should return at least the default font
    assert!(!fonts.is_empty(), "Should return at least one font");
    assert!(
        fonts.contains(&text_system.default_font_family()),
        "Font list should include default font"
    );

    tracing::info!("✓ Font enumeration verified");
}

// ============================================================================
// Additional test: Glyph shaping
// ============================================================================

#[test]
fn test_glyph_shaping() {
    init_tracing();
    tracing::info!("Additional test: Glyph shaping");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let text = "Hello";
    let font_family = text_system.default_font_family();
    let font_size = 16.0;

    let glyphs = text_system.shape_glyphs(text, &font_family, font_size);
    tracing::info!("Shaped '{}' into {} glyphs", text, glyphs.len());

    // MVP: Returns empty vector (no glyph data yet)
    // Phase 2: Will return positioned glyphs
    tracing::info!("✓ Glyph shaping verified (MVP: returns empty)");
}

// ============================================================================
// Performance test: Text measurement latency
// ============================================================================

#[test]
fn test_text_measurement_performance() {
    init_tracing();
    tracing::info!("Performance test: Text measurement latency");

    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();
    let font_family = text_system.default_font_family();
    let font_size = 16.0;

    let test_strings = vec![
        "Short",
        "Medium length text string",
        "This is a much longer text string that contains significantly more characters to measure",
        "Unicode: 你好世界 🌍 مرحبا",
    ];

    for text in test_strings {
        let start = std::time::Instant::now();
        let _bounds = text_system.measure_text(text, &font_family, font_size);
        let elapsed = start.elapsed();

        tracing::info!(
            "Measured '{}...' ({} chars) in {:.3}ms",
            &text.chars().take(20).collect::<String>(),
            text.chars().count(),
            elapsed.as_secs_f64() * 1000.0
        );

        // Target: <1ms for strings <100 characters (NFR-003)
        if text.chars().count() < 100 {
            assert!(
                elapsed.as_millis() < 10,
                "Text measurement should be fast (<10ms for MVP stub)"
            );
        }
    }

    tracing::info!("✓ Text measurement performance verified");
}
