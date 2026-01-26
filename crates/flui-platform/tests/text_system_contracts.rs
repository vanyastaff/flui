//! Cross-platform text system contract tests
//!
//! Tests: T043 - Verify text system consistency across all platforms
//!
//! These tests verify that all platform implementations of PlatformTextSystem
//! behave consistently and return reasonable values.

use flui_platform::current_platform;
use flui_types::geometry::px;

#[test]
fn test_all_platforms_return_default_font() {
    // Test that every platform returns a non-empty default font family
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let default_font = text_system.default_font_family();

    assert!(
        !default_font.is_empty(),
        "Platform {} returned empty default font family",
        platform.name()
    );

    // Platform-specific expectations
    #[cfg(windows)]
    assert_eq!(
        default_font, "Segoe UI",
        "Windows should use Segoe UI as default"
    );

    #[cfg(target_os = "macos")]
    assert_eq!(
        default_font, "SF Pro Text",
        "macOS should use SF Pro Text as default"
    );

    #[cfg(target_os = "linux")]
    assert_eq!(
        default_font, "Ubuntu",
        "Linux should use Ubuntu as default"
    );
}

#[test]
fn test_all_platforms_enumerate_fonts() {
    // Test that every platform can enumerate at least one font
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let fonts = text_system.enumerate_system_fonts();

    assert!(
        !fonts.is_empty(),
        "Platform {} returned no system fonts",
        platform.name()
    );

    // Default font should be in the list
    let default_font = text_system.default_font_family();
    assert!(
        fonts.contains(&default_font),
        "Default font '{}' should be in system fonts list",
        default_font
    );
}

#[test]
fn test_all_platforms_measure_text_reasonably() {
    // Test that all platforms return reasonable text measurements
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let test_cases = vec![
        ("Hello", 16.0),
        ("World!", 20.0),
        ("Test 123", 14.0),
    ];

    for (text, font_size) in test_cases {
        let bounds = text_system.measure_text(
            text,
            &text_system.default_font_family(),
            font_size,
        );

        // Width should be reasonable (roughly 0.5-0.7em per character)
        let char_count = text.chars().count() as f32;
        let min_width = char_count * font_size * 0.3; // Very conservative
        let max_width = char_count * font_size * 1.0; // Very generous

        assert!(
            bounds.width() >= px(min_width),
            "Platform {} measured '{}' at {}px too narrow: {}px (expected ≥{}px)",
            platform.name(),
            text,
            font_size,
            bounds.width(),
            min_width
        );

        assert!(
            bounds.width() <= px(max_width),
            "Platform {} measured '{}' at {}px too wide: {}px (expected ≤{}px)",
            platform.name(),
            text,
            font_size,
            bounds.width(),
            max_width
        );

        // Height should be roughly font_size * 1.0-1.5 (includes line height)
        let min_height = font_size * 0.8;
        let max_height = font_size * 2.0;

        assert!(
            bounds.height() >= px(min_height),
            "Platform {} measured '{}' at {}px too short: {}px (expected ≥{}px)",
            platform.name(),
            text,
            font_size,
            bounds.height(),
            min_height
        );

        assert!(
            bounds.height() <= px(max_height),
            "Platform {} measured '{}' at {}px too tall: {}px (expected ≤{}px)",
            platform.name(),
            text,
            font_size,
            bounds.height(),
            max_height
        );
    }
}

#[test]
fn test_all_platforms_handle_empty_text() {
    // Test that all platforms handle empty text gracefully
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let bounds = text_system.measure_text(
        "",
        &text_system.default_font_family(),
        16.0,
    );

    // Empty text should have zero or near-zero width
    assert!(
        bounds.width() <= px(5.0),
        "Platform {} measured empty text with width {}px (expected ≤5px)",
        platform.name(),
        bounds.width()
    );
}

#[test]
fn test_all_platforms_scale_with_font_size() {
    // Test that measurements scale proportionally with font size
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let text = "Test";
    let small_size = 12.0;
    let large_size = 24.0;
    let scale_factor = large_size / small_size; // 2.0x

    let small_bounds = text_system.measure_text(
        text,
        &text_system.default_font_family(),
        small_size,
    );

    let large_bounds = text_system.measure_text(
        text,
        &text_system.default_font_family(),
        large_size,
    );

    // Large should be approximately 2x small (within 10% tolerance)
    let width_ratio = large_bounds.width().0 / small_bounds.width().0;
    let height_ratio = large_bounds.height().0 / small_bounds.height().0;

    assert!(
        (width_ratio - scale_factor).abs() < scale_factor * 0.1,
        "Platform {} width scaling incorrect: {}x (expected ~{}x)",
        platform.name(),
        width_ratio,
        scale_factor
    );

    assert!(
        (height_ratio - scale_factor).abs() < scale_factor * 0.1,
        "Platform {} height scaling incorrect: {}x (expected ~{}x)",
        platform.name(),
        height_ratio,
        scale_factor
    );
}

#[test]
fn test_all_platforms_handle_unicode() {
    // Test that all platforms handle Unicode text
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let unicode_tests = vec![
        ("Hello 👋", "emoji"),
        ("你好", "CJK"),
        ("Привет", "Cyrillic"),
        ("مرحبا", "Arabic RTL"),
    ];

    for (text, description) in unicode_tests {
        let bounds = text_system.measure_text(
            text,
            &text_system.default_font_family(),
            16.0,
        );

        // Should return some non-zero width (stub approximation is fine)
        assert!(
            bounds.width() > px(0.0),
            "Platform {} failed to measure {} text '{}': returned zero width",
            platform.name(),
            description,
            text
        );

        assert!(
            bounds.height() > px(0.0),
            "Platform {} failed to measure {} text '{}': returned zero height",
            platform.name(),
            description,
            text
        );
    }
}

#[test]
fn test_all_platforms_glyph_shaping_returns_vec() {
    // Test that all platforms return a Vec for glyph shaping (even if empty in MVP)
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let glyphs = text_system.shape_glyphs(
        "Test",
        &text_system.default_font_family(),
        16.0,
    );

    // MVP stub returns empty vec - just verify it's a valid Vec
    // (Future implementations will return actual glyphs)
    assert!(
        glyphs.is_empty(),
        "MVP stub should return empty glyph vec, got {} glyphs",
        glyphs.len()
    );
}

#[test]
fn test_all_platforms_font_loading_not_implemented() {
    // Test that all platforms return NotImplemented for font loading in MVP
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    use flui_platform::TextSystemError;

    let result = text_system.load_system_font("Arial");

    assert!(
        result.is_err(),
        "MVP stub should return error for font loading"
    );

    if let Err(err) = result {
        assert_eq!(
            err,
            TextSystemError::NotImplemented,
            "Should return NotImplemented error"
        );
    }
}
