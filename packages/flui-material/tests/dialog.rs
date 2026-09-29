//! `Dialog`/`AlertDialog` widget-level integration coverage — mounts each
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/material.rs` use).

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{AlertDialog, Theme, ThemeData};
use flui_sdk::painting::Color;
use flui_sdk::view::ViewExt;
use flui_sdk::widgets::{ColoredBox, GestureDetector, SizedBox, Text};

// ============================================================================
// Dialog — _DialogDefaultsM3
// ============================================================================

// ============================================================================
// AlertDialog — title / content / actions composition
// ============================================================================

/// A tap on an action reaches its own `on_tap` handler — the action row is
/// hit-testable, not just laid out.
pub fn a_tap_on_an_action_fires_its_handler() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);

    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            AlertDialog::new().title(Text::new("Delete?")).actions(vec![
                GestureDetector::new()
                    .on_tap(move |_cx| {
                        counted.fetch_add(1, Ordering::SeqCst);
                    })
                    .child(
                        ColoredBox::new(Color::rgb(200, 10, 10)).child(SizedBox::new(40.0, 40.0)),
                    )
                    .boxed(),
            ]),
        ),
        tight(1000.0, 1000.0),
    );

    let action = laid
        .try_find_by_render_type("RenderDecoratedBox")
        .expect("the action's ColoredBox must mount");
    let origin = laid.absolute_offset(action);
    let size = laid.size(action);
    let center_x = origin.dx + size.width / 2.0;
    let center_y = origin.dy + size.height / 2.0;

    laid.dispatch_pointer_down(center_x, center_y);
    laid.dispatch_pointer_up(center_x, center_y);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up dispatched at the action's own on-screen position must fire its handler"
    );
}
