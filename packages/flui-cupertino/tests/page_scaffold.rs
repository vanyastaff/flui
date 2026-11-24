//! Integration tests for [`CupertinoPageScaffold`] — the navigation bar's
//! obstruction answer deciding whether content is padded below the bar or
//! runs under it, and that a navigation bar mounts as an overlay.

use crate::common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LaidOut, lay_out, tight};
use flui_cupertino::{CupertinoNavigationBar, CupertinoPageScaffold};
use flui_sdk::foundation::RenderId;
use flui_sdk::painting::styling::Color;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;
use flui_sdk::widgets::prelude::EdgeInsets;

/// The unique `RenderConstrainedBox` sized exactly `width x height` — used
/// to locate the content marker unambiguously alongside the navigation
/// bar's own (differently-sized) `SizedBox`.
pub(crate) fn find_by_size(laid: &LaidOut, width: f64, height: f64) -> RenderId {
    laid.find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            (size.width - width).abs() < 0.01 && (size.height - height).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("no RenderConstrainedBox sized {width}x{height}"))
}

/// A content leaf that reports the `MediaQuery` top padding it builds under.
#[derive(Clone, StatelessView)]
struct TopPaddingReader(Rc<Cell<Option<f64>>>);

impl StatelessView for TopPaddingReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.0
            .set(MediaQuery::padding_of(ctx).map(|padding| padding.top));
        SizedBox::new(60.0, 30.0)
    }
}

/// The scaffold asks the bar whether it fully obstructs the content
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/page_scaffold.dart:164`).
/// Under the default, translucent bar the content starts at the top, behind
/// the bar, and reads the obstructed height (44 + the 20-point top inset) as
/// its `MediaQuery` top padding; under an opaque bar the content is pushed
/// down by that height and its top padding is consumed.
pub fn content_runs_under_a_translucent_bar_and_below_an_opaque_one() {
    let opaque = Color::from_argb(0xFFF9_F9F9);
    for (name, bar, expected_top, expected_padding_top) in [
        ("translucent", CupertinoNavigationBar::new(), 0.0, 64.0),
        (
            "opaque",
            CupertinoNavigationBar::new().background_color(opaque),
            64.0,
            0.0,
        ),
    ] {
        let media = MediaQueryData {
            view_padding: EdgeInsets::new(20.0, 0.0, 0.0, 0.0),
            ..MediaQueryData::default()
        };
        let padding_top = Rc::new(Cell::new(None));
        let laid = lay_out(
            MediaQuery::new(
                media,
                CupertinoPageScaffold::new(TopPaddingReader(Rc::clone(&padding_top)))
                    .navigation_bar(bar),
            ),
            tight(400.0, 600.0),
        );

        let content = find_by_size(&laid, 60.0, 30.0);
        let offset = laid.absolute_offset(content);
        assert!(
            (offset.dy - expected_top).abs() < 0.01,
            "{name}: the content starts at y={expected_top}: {offset:?}"
        );
        let read = padding_top
            .get()
            .unwrap_or_else(|| panic!("{name}: the content built under a MediaQuery"));
        assert!(
            (read - expected_padding_top).abs() < 0.01,
            "{name}: the content reads a top padding of {expected_padding_top}, read {read}"
        );
    }
}
