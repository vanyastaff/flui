//! Example-based `Color` tests: construction, hex parsing anchors and
//! errors, opacity, named constants. Properties over arbitrary colors live in
//! `color_property_tests.rs` and `color_blend_tests.rs`.

use flui_types::styling::{Color, ParseColorError};
use rstest::rstest;

#[test]
fn rgb_is_opaque_and_rgba_keeps_every_channel() {
    assert_eq!(
        Color::rgb(1, 2, 3),
        Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255
        }
    );
    assert_eq!(
        Color::rgba(1, 2, 3, 4),
        Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4
        }
    );
}

#[rstest]
#[case::rrggbb("#FF0000", Color::rgb(255, 0, 0))]
#[case::aarrggbb("#80FF0000", Color::rgba(255, 0, 0, 128))]
#[case::white("#FFFFFF", Color::WHITE)]
#[case::black("#000000", Color::BLACK)]
fn from_hex_anchors(#[case] hex: &str, #[case] expected: Color) {
    assert_eq!(Color::from_hex(hex), Ok(expected));
    // Each anchor is also the canonical form `to_hex` writes: `#`, upper
    // case, and six digits when opaque.
    assert_eq!(expected.to_hex(), hex);
}

#[rstest]
#[case::three_digits("#FFF", ParseColorError::InvalidLength)]
#[case::empty("", ParseColorError::InvalidLength)]
#[case::ten_digits("#FFFFFFFFFF", ParseColorError::InvalidLength)]
#[case::non_hex_digits("#GGGGGG", ParseColorError::InvalidHex)]
#[case::non_hex_argb("#GG000000", ParseColorError::InvalidHex)]
fn from_hex_errors(#[case] hex: &str, #[case] expected: ParseColorError) {
    assert_eq!(Color::from_hex(hex), Err(expected));
}

#[test]
fn parse_color_error_messages() {
    assert_eq!(
        ParseColorError::InvalidHex.to_string(),
        "Invalid hex color format"
    );
    assert_eq!(
        ParseColorError::InvalidLength.to_string(),
        "Invalid hex color length (expected 6 or 8 characters)"
    );
}

#[test]
fn lighten_white_and_darken_black_are_fixed_points() {
    assert_eq!(Color::WHITE.lighten(0.5), Color::WHITE);
    assert_eq!(Color::BLACK.darken(0.5), Color::BLACK);
}

#[rstest]
#[case::opaque(1.0, 255)]
#[case::half(0.5, 127)]
#[case::zero(0.0, 0)]
#[case::clamped_high(2.0, 255)]
#[case::clamped_low(-1.0, 0)]
fn with_opacity_sets_only_alpha(#[case] opacity: f32, #[case] alpha: u8) {
    let c = Color::rgba(100, 150, 200, 7);
    assert_eq!(c.with_opacity(opacity), Color::rgba(100, 150, 200, alpha));
}

#[test]
fn named_constants() {
    for (named, expected) in [
        (Color::TRANSPARENT, Color::rgba(0, 0, 0, 0)),
        (Color::BLACK, Color::rgb(0, 0, 0)),
        (Color::WHITE, Color::rgb(255, 255, 255)),
        (Color::RED, Color::rgb(255, 0, 0)),
        (Color::GREEN, Color::rgb(0, 255, 0)),
        (Color::BLUE, Color::rgb(0, 0, 255)),
        (Color::YELLOW, Color::rgb(255, 255, 0)),
        (Color::CYAN, Color::rgb(0, 255, 255)),
        (Color::MAGENTA, Color::rgb(255, 0, 255)),
        (Color::GRAY, Color::rgb(128, 128, 128)),
        (Color::LIGHT_GRAY, Color::rgb(192, 192, 192)),
        (Color::DARK_GRAY, Color::rgb(64, 64, 64)),
    ] {
        assert_eq!(named, expected);
    }
    assert_eq!(Color::default(), Color::TRANSPARENT);
}
