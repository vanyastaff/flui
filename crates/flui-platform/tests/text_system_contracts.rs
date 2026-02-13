//! Cross-platform text system contract tests
//!
//! Tests: T043 - Verify text system consistency across all platforms
//!
//! These tests verify that all platform implementations of PlatformTextSystem
//! behave consistently and return reasonable values.

use flui_platform::current_platform;
use flui_platform::traits::{Font, FontRun, FontStyle, FontWeight};

#[test]
fn test_all_platforms_return_font_names() {
    // Test that every platform returns a non-empty font list
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();

    assert!(
        !font_names.is_empty(),
        "Platform {} returned no font names",
        platform.name()
    );

    // Platform-specific expectations
    #[cfg(windows)]
    assert!(
        font_names.iter().any(|n| n == "Segoe UI"),
        "Windows should have Segoe UI"
    );
}

#[test]
fn test_all_platforms_resolve_font() {
    // Test that every platform can resolve a standard font
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    assert!(!font_names.is_empty());

    // Use the first available font
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };

    let id = text_system.font_id(&font);
    assert!(
        id.is_ok(),
        "Platform {} failed to resolve font '{}': {:?}",
        platform.name(),
        font_names[0],
        id.err()
    );
}

#[test]
fn test_all_platforms_layout_line_reasonably() {
    // Test that all platforms return reasonable text layout
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let test_cases = vec![("Hello", 16.0), ("World!", 20.0), ("Test 123", 14.0)];

    for (text, font_size) in test_cases {
        let layout = text_system.layout_line(
            text,
            font_size,
            &[FontRun {
                font_id: id,
                len: text.len(),
            }],
        );

        // Width should be reasonable (roughly 0.3-1.0em per character)
        let char_count = text.chars().count() as f32;
        let min_width = char_count * font_size * 0.3;
        let max_width = char_count * font_size * 1.0;

        assert!(
            layout.width >= min_width,
            "Platform {} measured '{}' at {}px too narrow: {}px (expected >={}px)",
            platform.name(),
            text,
            font_size,
            layout.width,
            min_width
        );

        assert!(
            layout.width <= max_width,
            "Platform {} measured '{}' at {}px too wide: {}px (expected <={}px)",
            platform.name(),
            text,
            font_size,
            layout.width,
            max_width
        );
    }
}

#[test]
fn test_all_platforms_handle_empty_text() {
    // Test that all platforms handle empty text gracefully
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let layout = text_system.layout_line("", 16.0, &[]);

    assert_eq!(
        layout.width,
        0.0,
        "Platform {} measured empty text with width {}px (expected 0)",
        platform.name(),
        layout.width
    );
}

#[test]
fn test_all_platforms_scale_with_font_size() {
    // Test that measurements scale proportionally with font size
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let text = "Test";
    let small_size = 12.0;
    let large_size = 24.0;
    let scale_factor = large_size / small_size; // 2.0x

    let small_layout = text_system.layout_line(
        text,
        small_size,
        &[FontRun {
            font_id: id,
            len: text.len(),
        }],
    );

    let large_layout = text_system.layout_line(
        text,
        large_size,
        &[FontRun {
            font_id: id,
            len: text.len(),
        }],
    );

    // Large should be approximately 2x small (within 20% tolerance)
    let width_ratio = large_layout.width / small_layout.width;

    assert!(
        (width_ratio - scale_factor).abs() < scale_factor * 0.2,
        "Platform {} width scaling incorrect: {}x (expected ~{}x)",
        platform.name(),
        width_ratio,
        scale_factor
    );
}

#[test]
fn test_all_platforms_handle_unicode() {
    // Test that all platforms handle Unicode text
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let unicode_tests = vec![("Привет", "Cyrillic"), ("你好", "CJK")];

    for (text, description) in unicode_tests {
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
            "Platform {} failed to layout {} text '{}': zero width",
            platform.name(),
            description,
            text
        );
    }
}

#[test]
fn test_all_platforms_font_metrics_reasonable() {
    // Test that all platforms return reasonable font metrics
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();
    let metrics = text_system.font_metrics(id);

    assert!(metrics.units_per_em > 0, "units_per_em should be > 0");
    assert!(metrics.ascent > 0.0, "ascent should be > 0");
    assert!(metrics.descent > 0.0, "descent should be > 0");
}

#[test]
fn test_all_platforms_glyph_for_char() {
    // Test that all platforms can look up a basic glyph
    let platform = current_platform().expect("Failed to get platform");
    let text_system = platform.text_system();

    let font_names = text_system.all_font_names();
    let font = Font {
        family: font_names[0].clone(),
        weight: FontWeight::Normal,
        style: FontStyle::Normal,
    };
    let id = text_system.font_id(&font).unwrap();

    let glyph = text_system.glyph_for_char(id, 'A');
    assert!(
        glyph.is_some(),
        "Platform {} should have glyph for 'A'",
        platform.name()
    );
}
