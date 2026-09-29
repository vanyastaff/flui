//! Example-based `Color` tests: construction, hex parsing anchors and
//! errors, opacity, named constants. Properties over arbitrary colors live in
//! `color_property_tests.rs` and `color_blend_tests.rs`.

use flui_painting::styling::{Color, ParseColorError};
use rstest::rstest;

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
