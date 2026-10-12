//! Integration tests for [`CupertinoNavigationBar`] — the 44pt persistent
//! height contract, the hairline border's oracle-cited alpha, background
//! resolution against the theme, the top safe-area inset, and that
//! leading/middle/trailing all actually reach the mounted render tree.
//!
//! Every mount wraps the bar in a [`MediaQuery`] ancestor: `SafeArea`
//! (`nav_bar.rs`'s own self-padding, matching `flui-material`'s `AppBar`
//! and its identical contract) reads `MediaQuery::of` unconditionally and panics
//! with no ancestor — see `flui-material/tests/app_bar.rs` for the same
//! precedent.

use crate::common;

use common::{lay_out, tight};
use flui_cupertino::CupertinoNavigationBar;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox, Text};
use flui_testing::a11y::Role;

/// `leading`/`middle`/`trailing` all reach the mounted render tree, not just
/// the constructor's stored fields — proven by a delta against a bar with
/// none of the three set (whose own outer `SizedBox` already contributes one
/// `RenderConstrainedBox`, so an absolute count would be misleading).
pub fn leading_middle_and_trailing_all_mount() {
    let empty = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    let empty_constrained_box_count = empty.find_all_by_render_type("RenderConstrainedBox").len();

    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new()
                .leading(SizedBox::new(20.0, 20.0))
                .middle(Text::new("Settings"))
                .trailing(SizedBox::new(20.0, 20.0)),
        ),
        tight(400.0, 44.0),
    );

    assert!(
        laid.try_find_by_render_type("RenderParagraph").is_some(),
        "the middle Text must mount as a RenderParagraph"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        empty_constrained_box_count + 2,
        "both the leading and trailing SizedBox must mount, on top of the bar's own"
    );
}

/// The middle is the page title, so a screen reader's heading navigation
/// reaches it as a level-one heading named by its text.
pub fn the_middle_announces_as_a_level_one_heading() {
    let mut laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new().middle(Text::new("Settings")),
        ),
        tight(400.0, 44.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    let heading = tree
        .find(Role::Heading)
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert_eq!(heading.label(), Some("Settings"), "{}", tree.describe());
    assert_eq!(heading.heading_level(), Some(1), "{}", tree.describe());
}
