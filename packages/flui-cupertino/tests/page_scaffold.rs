//! Integration tests for [`CupertinoPageScaffold`] — the background
//! default, the navigation-bar content-padding contract (bar height + top
//! inset), `resize_to_avoid_bottom_inset`, and that a navigation bar
//! actually mounts as an overlay.

use crate::common;

use common::{LaidOut, lay_out, tight};
use flui_cupertino::{CupertinoNavigationBar, CupertinoPageScaffold};
use flui_sdk::foundation::RenderId;
use flui_sdk::widgets::prelude::EdgeInsets;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

/// The unique `RenderConstrainedBox` sized exactly `width x height` — used
/// to locate the content marker unambiguously alongside the navigation
/// bar's own (differently-sized) `SizedBox`.
fn find_by_size(laid: &LaidOut, width: f64, height: f64) -> RenderId {
    laid.find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            (size.width - width).abs() < 0.01 && (size.height - height).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("no RenderConstrainedBox sized {width}x{height}"))
}

/// With a navigation bar present, content is pushed down by exactly
/// `preferred_size().height + MediaQuery.padding.top` —
/// `page_scaffold.dart`'s `topPadding` (oracle tag `3.44.0`).
///
/// Red-check: drop `+ media.padding.top` from `top_padding`'s computation in
/// `page_scaffold.rs` — this test's offset assertion fails (would read
/// `44.0` instead of `64.0`).
#[test]
fn content_is_padded_below_the_nav_bar_plus_the_top_inset() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(20.0, 0.0, 0.0, 0.0),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        MediaQuery::new(
            media,
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0))
                .navigation_bar(CupertinoNavigationBar::new()),
        ),
        tight(400.0, 600.0),
    );

    let content = find_by_size(&laid, 60.0, 30.0);
    let offset = laid.absolute_offset(content);
    assert!(
        (offset.dy - 64.0).abs() < 0.01,
        "44.0 nav bar height + 20.0 top inset must push content to y=64.0: {offset:?}"
    );
}
