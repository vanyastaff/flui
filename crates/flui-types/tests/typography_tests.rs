//! Comprehensive typography system tests
//!
//! This test suite validates the typography primitives including:
//! - FontWeight (enum variants W100-W900)
//! - FontStyle (Normal, Italic)
//! - TextAlign (Left, Right, Center, Justify, Start, End)
//! - TextDirection (Ltr, Rtl)
//! - TextOverflow (Clip, Fade, Ellipsis, Visible)
//! - TextDecoration (bitfield with has_* methods)
//! - TextStyle (complete styling configuration)

use flui_types::typography::{
    FontStyle, FontWeight, TextAlign, TextDecoration, TextDirection, TextOverflow, TextStyle,
};

// ============================================================================
// FontWeight Tests (5 tests)
// ============================================================================

#[test]
fn test_font_weight_variants() {
    // Test that all weight variants exist
    let weights = [
        FontWeight::W100,
        FontWeight::W200,
        FontWeight::W300,
        FontWeight::W400,
        FontWeight::W500,
        FontWeight::W600,
        FontWeight::W700,
        FontWeight::W800,
        FontWeight::W900,
    ];

    assert_eq!(weights.len(), 9);
}

#[test]
fn test_font_weight_values() {
    // Test that value() returns correct numbers
    assert_eq!(FontWeight::W100.value(), 100);
    assert_eq!(FontWeight::W200.value(), 200);
    assert_eq!(FontWeight::W300.value(), 300);
    assert_eq!(FontWeight::W400.value(), 400);
    assert_eq!(FontWeight::W500.value(), 500);
    assert_eq!(FontWeight::W600.value(), 600);
    assert_eq!(FontWeight::W700.value(), 700);
    assert_eq!(FontWeight::W800.value(), 800);
    assert_eq!(FontWeight::W900.value(), 900);
}

#[test]
fn test_font_weight_constants() {
    // Test named constants
    assert_eq!(FontWeight::NORMAL, FontWeight::W400);
    assert_eq!(FontWeight::BOLD, FontWeight::W700);
}

#[test]
fn test_font_weight_is_bold() {
    // Weights >= 600 should be considered bold
    assert!(!FontWeight::W100.is_bold());
    assert!(!FontWeight::W400.is_bold());
    assert!(!FontWeight::W500.is_bold());
    assert!(FontWeight::W600.is_bold());
    assert!(FontWeight::W700.is_bold());
    assert!(FontWeight::W900.is_bold());
}

#[test]
fn test_font_weight_from_css() {
    // Test CSS value conversion
    assert_eq!(FontWeight::from_css(100), FontWeight::W100);
    assert_eq!(FontWeight::from_css(200), FontWeight::W200);
    assert_eq!(FontWeight::from_css(400), FontWeight::W400);
    assert_eq!(FontWeight::from_css(700), FontWeight::W700);

    // Test rounding
    assert_eq!(FontWeight::from_css(350), FontWeight::W300);
    assert_eq!(FontWeight::from_css(550), FontWeight::W600);
}

// ============================================================================
// FontStyle Tests (3 tests)
// ============================================================================

#[test]
fn test_font_style_variants() {
    let normal = FontStyle::Normal;
    let italic = FontStyle::Italic;

    assert_ne!(normal, italic);
}

#[test]
fn test_font_style_default() {
    let default_style = FontStyle::default();
    assert_eq!(default_style, FontStyle::Normal);
}

#[test]
fn test_font_style_all_variants() {
    // Verify both variants are constructible
    let styles = [FontStyle::Normal, FontStyle::Italic];

    assert_eq!(styles.len(), 2);
}

// ============================================================================
// TextAlign Tests (4 tests)
// ============================================================================

#[test]
fn test_text_align_variants() {
    let left = TextAlign::Left;
    let right = TextAlign::Right;
    let center = TextAlign::Center;
    let justify = TextAlign::Justify;
    let start = TextAlign::Start;
    let end = TextAlign::End;

    // All variants should be distinct
    assert_ne!(left, right);
    assert_ne!(left, center);
    assert_ne!(center, justify);
    assert_ne!(start, end);
}

#[test]
fn test_text_align_default() {
    let default_align = TextAlign::default();
    assert_eq!(default_align, TextAlign::Left);
}

#[test]
fn test_text_align_directional() {
    // Start and End are direction-aware
    let start = TextAlign::Start;
    let end = TextAlign::End;
    let left = TextAlign::Left;
    let right = TextAlign::Right;

    // These should be different types even if they resolve the same
    assert_ne!(start, left);
    assert_ne!(end, right);
}

#[test]
fn test_text_align_all_variants() {
    let alignments = [
        TextAlign::Left,
        TextAlign::Right,
        TextAlign::Center,
        TextAlign::Justify,
        TextAlign::Start,
        TextAlign::End,
    ];

    assert_eq!(alignments.len(), 6);
}

// ============================================================================
// TextDirection Tests (3 tests)
// ============================================================================

#[test]
fn test_text_direction_variants() {
    let ltr = TextDirection::Ltr;
    let rtl = TextDirection::Rtl;

    assert_ne!(ltr, rtl);
}

#[test]
fn test_text_direction_default() {
    let default_direction = TextDirection::default();
    assert_eq!(default_direction, TextDirection::Ltr);
}

#[test]
fn test_text_direction_is_ltr() {
    assert!(TextDirection::Ltr.is_ltr());
    assert!(!TextDirection::Rtl.is_ltr());
}

// ============================================================================
// TextOverflow Tests (4 tests)
// ============================================================================

#[test]
fn test_text_overflow_variants() {
    // Test that all variants are constructible
    let _clip = TextOverflow::Clip;
    let _ellipsis = TextOverflow::Ellipsis;
    let _fade = TextOverflow::Fade;
    let _visible = TextOverflow::Visible;

    // Can't compare without PartialEq, but construction is valid
}

#[test]
fn test_text_overflow_default() {
    let default_overflow = TextOverflow::default();

    // Verify it's Clip by pattern matching
    match default_overflow {
        TextOverflow::Clip => (),
        _ => panic!("Default should be Clip"),
    }
}

#[test]
fn test_text_overflow_all_variants() {
    let variants = vec![
        TextOverflow::Clip,
        TextOverflow::Ellipsis,
        TextOverflow::Fade,
        TextOverflow::Visible,
    ];

    assert_eq!(variants.len(), 4);
}

#[test]
fn test_text_overflow_pattern_matching() {
    let clip = TextOverflow::Clip;
    let visible = TextOverflow::Visible;

    // Test pattern matching works
    match clip {
        TextOverflow::Clip => (),
        _ => panic!("Should be Clip"),
    }

    match visible {
        TextOverflow::Visible => (),
        _ => panic!("Should be Visible"),
    }
}

// ============================================================================
// TextDecoration Tests (5 tests)
// ============================================================================

#[test]
fn test_text_decoration_none() {
    let none = TextDecoration::NONE;

    assert!(!none.has_underline());
    assert!(!none.has_overline());
    assert!(!none.has_line_through());
    assert!(none.is_none());
}

#[test]
fn test_text_decoration_single() {
    let underline = TextDecoration::UNDERLINE;
    let overline = TextDecoration::OVERLINE;
    let line_through = TextDecoration::LINE_THROUGH;

    assert!(underline.has_underline());
    assert!(!underline.has_overline());

    assert!(overline.has_overline());
    assert!(!overline.has_underline());

    assert!(line_through.has_line_through());
    assert!(!line_through.has_underline());
}

#[test]
fn test_text_decoration_combined() {
    let combined =
        TextDecoration::combine(&[TextDecoration::UNDERLINE, TextDecoration::LINE_THROUGH]);

    assert!(combined.has_underline());
    assert!(combined.has_line_through());
    assert!(!combined.has_overline());
}

#[test]
fn test_text_decoration_combine_method() {
    let decorations = [TextDecoration::UNDERLINE, TextDecoration::OVERLINE];

    let combined = TextDecoration::combine(&decorations);
    assert!(combined.has_underline());
    assert!(combined.has_overline());
    assert!(!combined.has_line_through());
}

#[test]
fn test_text_decoration_all() {
    let all = TextDecoration::combine(&[
        TextDecoration::UNDERLINE,
        TextDecoration::OVERLINE,
        TextDecoration::LINE_THROUGH,
    ]);

    assert!(all.has_underline());
    assert!(all.has_overline());
    assert!(all.has_line_through());
}

// ============================================================================
// TextStyle Tests (6 tests)
// ============================================================================

#[test]
fn test_text_style_default() {
    let style = TextStyle::default();

    // Default should have None for optional fields
    assert!(style.font_size.is_none());
    assert!(style.font_weight.is_none());
    assert!(style.font_style.is_none());
    assert!(style.color.is_none());
}

#[test]
fn test_text_style_with_font_family() {
    let style = TextStyle {
        font_family: Some("Roboto".to_string()),
        font_family_fallback: vec!["sans-serif".to_string()],
        ..Default::default()
    };

    assert_eq!(style.font_family, Some("Roboto".to_string()));
    assert_eq!(style.font_family_fallback.len(), 1);
}

#[test]
fn test_text_style_with_size() {
    let style = TextStyle {
        font_size: Some(16.0),
        ..Default::default()
    };

    assert_eq!(style.font_size, Some(16.0));
}

#[test]
fn test_text_style_with_weight() {
    let bold_style = TextStyle {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    };

    assert_eq!(bold_style.font_weight, Some(FontWeight::BOLD));

    let light_style = TextStyle {
        font_weight: Some(FontWeight::W300),
        ..Default::default()
    };

    assert_eq!(light_style.font_weight, Some(FontWeight::W300));
}

#[test]
fn test_text_style_with_italic() {
    let italic_style = TextStyle {
        font_style: Some(FontStyle::Italic),
        ..Default::default()
    };

    assert_eq!(italic_style.font_style, Some(FontStyle::Italic));
}

#[test]
fn test_text_style_with_spacing() {
    let spaced_style = TextStyle {
        letter_spacing: Some(1.5),
        word_spacing: Some(2.0),
        ..Default::default()
    };

    assert_eq!(spaced_style.letter_spacing, Some(1.5));
    assert_eq!(spaced_style.word_spacing, Some(2.0));
}
