//! Const-table oracle-diff test for [`CupertinoColors`] — every V1-scoped
//! constant's full 8-variant ARGB table, asserted against
//! `cupertino/colors.dart` at tag `3.44.0`.
//!
//! Each assertion is written against the raw `(r, g, b, a)` channels rather
//! than re-deriving `Color::rgba(...)` calls that would just restate
//! `src/colors.rs`'s own construction — a copy-paste-the-source test proves
//! nothing. The oracle's `dark` `systemBlue` value is the flagged trap: the
//! actual tag-3.44.0 value is `(10, 132, 255)`, one digit away from the
//! superficially-plausible `(9, 132, 255)`.

use flui_cupertino::CupertinoColors;
use flui_types::Color;

fn channels(color: Color) -> (u8, u8, u8, u8) {
    (color.r, color.g, color.b, color.a)
}

#[test]
fn label_matches_the_oracle() {
    let label = CupertinoColors::LABEL;
    assert_eq!(channels(label.color), (0, 0, 0, 255));
    assert_eq!(channels(label.dark_color), (255, 255, 255, 255));
    assert_eq!(channels(label.elevated_color), (0, 0, 0, 255));
    assert_eq!(channels(label.dark_elevated_color), (255, 255, 255, 255));
}

#[test]
fn secondary_label_matches_the_oracle_including_the_high_contrast_alpha_bump() {
    let secondary = CupertinoColors::SECONDARY_LABEL;
    assert_eq!(channels(secondary.color), (60, 60, 67, 153));
    assert_eq!(channels(secondary.dark_color), (235, 235, 245, 153));
    assert_eq!(channels(secondary.high_contrast_color), (60, 60, 67, 173));
    assert_eq!(
        channels(secondary.dark_high_contrast_color),
        (235, 235, 245, 173)
    );
}

#[test]
fn system_background_matches_the_oracle() {
    let bg = CupertinoColors::SYSTEM_BACKGROUND;
    assert_eq!(channels(bg.color), (255, 255, 255, 255));
    assert_eq!(channels(bg.dark_color), (0, 0, 0, 255));
    assert_eq!(channels(bg.dark_elevated_color), (28, 28, 30, 255));
}

#[test]
fn secondary_system_background_matches_the_oracle() {
    let bg = CupertinoColors::SECONDARY_SYSTEM_BACKGROUND;
    assert_eq!(channels(bg.color), (242, 242, 247, 255));
    assert_eq!(channels(bg.dark_color), (28, 28, 30, 255));
}

#[test]
fn separator_matches_the_oracle() {
    let separator = CupertinoColors::SEPARATOR;
    assert_eq!(channels(separator.color), (60, 60, 67, 73));
    assert_eq!(channels(separator.dark_color), (84, 84, 88, 153));
    // `separator`'s `darkElevatedColor` is the one variant among this
    // crate's table whose RGB (not just alpha) diverges from its own
    // `darkColor` — `(210, 210, 210)` vs `(84, 84, 88)` — a spot a
    // "just mirror light/dark" shortcut implementation would miss.
    assert_eq!(
        channels(separator.dark_elevated_color),
        (210, 210, 210, 153)
    );
}

#[test]
fn opaque_separator_matches_the_oracle() {
    let separator = CupertinoColors::OPAQUE_SEPARATOR;
    assert_eq!(channels(separator.color), (198, 198, 200, 255));
    assert_eq!(channels(separator.dark_color), (56, 56, 58, 255));
}

#[test]
fn system_fill_family_matches_the_oracle() {
    assert_eq!(
        channels(CupertinoColors::SYSTEM_FILL.color),
        (120, 120, 128, 51)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_FILL.dark_color),
        (120, 120, 128, 91)
    );
    assert_eq!(
        channels(CupertinoColors::SECONDARY_SYSTEM_FILL.color),
        (120, 120, 128, 40)
    );
    assert_eq!(
        channels(CupertinoColors::TERTIARY_SYSTEM_FILL.color),
        (118, 118, 128, 30)
    );
    // `CupertinoButton`'s plain-style default `disabledColor`.
    assert_eq!(
        channels(CupertinoColors::QUATERNARY_SYSTEM_FILL.color),
        (116, 116, 128, 20)
    );
    assert_eq!(
        channels(CupertinoColors::QUATERNARY_SYSTEM_FILL.dark_color),
        (118, 118, 128, 45)
    );
}

/// The flagged trap: `systemBlue`'s dark variant.
#[test]
fn system_blue_dark_variant_is_10_132_255_not_9_132_255() {
    let system_blue = CupertinoColors::SYSTEM_BLUE;
    assert_eq!(channels(system_blue.color), (0, 122, 255, 255));
    assert_eq!(channels(system_blue.dark_color), (10, 132, 255, 255));
    assert_eq!(channels(system_blue.high_contrast_color), (0, 64, 221, 255));
    assert_eq!(
        channels(system_blue.dark_high_contrast_color),
        (64, 156, 255, 255)
    );
    // `activeBlue` is a plain alias — a distinct-storage divergence would
    // fail this trivially.
    assert_eq!(CupertinoColors::ACTIVE_BLUE, CupertinoColors::SYSTEM_BLUE);
}

#[test]
fn system_grey_family_matches_the_oracle() {
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY.color),
        (142, 142, 147, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY2.color),
        (174, 174, 178, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY2.dark_color),
        (99, 99, 102, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY3.color),
        (199, 199, 204, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY3.dark_color),
        (72, 72, 74, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY4.color),
        (209, 209, 214, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY4.dark_color),
        (58, 58, 60, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY5.color),
        (229, 229, 234, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY5.dark_color),
        (44, 44, 46, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY6.color),
        (242, 242, 247, 255)
    );
    assert_eq!(
        channels(CupertinoColors::SYSTEM_GREY6.dark_color),
        (28, 28, 30, 255)
    );
}

#[test]
fn destructive_red_is_an_alias_of_system_red_with_the_oracle_values() {
    assert_eq!(
        CupertinoColors::DESTRUCTIVE_RED,
        CupertinoColors::SYSTEM_RED
    );
    let red = CupertinoColors::SYSTEM_RED;
    assert_eq!(channels(red.color), (255, 59, 48, 255));
    assert_eq!(channels(red.dark_color), (255, 69, 58, 255));
    assert_eq!(channels(red.high_contrast_color), (215, 0, 21, 255));
}

#[test]
fn inactive_gray_matches_the_oracle() {
    let inactive = CupertinoColors::INACTIVE_GRAY;
    assert_eq!(channels(inactive.color), (0x99, 0x99, 0x99, 255));
    assert_eq!(channels(inactive.dark_color), (0x75, 0x75, 0x75, 255));
}

#[test]
fn white_black_transparent_match_the_oracle() {
    assert_eq!(channels(CupertinoColors::WHITE), (255, 255, 255, 255));
    assert_eq!(channels(CupertinoColors::BLACK), (0, 0, 0, 255));
    assert_eq!(channels(CupertinoColors::TRANSPARENT), (0, 0, 0, 0));
}
