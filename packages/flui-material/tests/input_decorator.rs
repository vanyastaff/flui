//! `InputDecorator` widget-level integration coverage — mounts a real
//! `InputDecorator` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/card.rs`/`tests/ink_well.rs` use) and probes the
//! composed [`flui_sdk::widgets::DecoratedBox`] (`RenderDecoratedBox`),
//! [`flui_sdk::widgets::MouseRegion`] (`RenderMouseRegion`), and
//! [`flui_sdk::widgets::Text`] (`RenderParagraph`) render objects it produces.
//!
//! # Hover blend is not end-to-end drivable here
//!
//! `tests/ink_well.rs`'s own module doc already established this gap:
//! `MouseRegion::on_enter`/`on_exit` require `MouseTracker::update_with_event`,
//! which only a full `UiRuntime` frame pump runs — the raw
//! `HitTestResult::dispatch` this headless harness's `dispatch_pointer_move`
//! calls never reaches it. `InputDecoratorState` uses `on_enter`/`on_exit`
//! (the oracle's own `TextField._handleHover` wiring, `text_field.dart:1799`,
//! tag `3.44.0`) for the same real-behavior reason `InkWell` does — the hover
//! blend math itself is exhaustively pinned by `input_decorator.rs`'s own
//! unit tests (`hover_blend_*`); this file only proves the `MouseRegion` is
//! actually composed around the container, structurally.

// a panic IS the failure report in test code (docs/PANIC-POLICY.md)

use crate::common;

use common::{lay_out, tight};
use flui_material::{InputDecoration, InputDecorator, Theme, ThemeData};
use flui_sdk::widgets::SizedBox;

/// A small render-object child standing in for a real field's content (e.g.
/// a future `EditableText`) — `SizedBox` renders as `RenderConstrainedBox`,
/// distinct from the decorator's own `RenderDecoratedBox`/`RenderParagraph`
/// nodes, so it can't be confused with them in a render-type count.
fn child_stub() -> SizedBox {
    SizedBox::new(20.0, 20.0)
}

/// Error replaces helper: with both set, exactly one helper/error text row
/// renders, not two.
pub fn error_replaces_helper_at_the_mounted_level() {
    let theme = ThemeData::light();
    let decoration = InputDecoration {
        helper_text: Some("Helper".to_string()),
        error_text: Some("Error".to_string()),
        filled: true,
        ..Default::default()
    };
    // No label/hint set, so the only text row is the helper-or-error line.
    let laid = lay_out(
        Theme::new(theme, InputDecorator::new(decoration).child(child_stub())),
        tight(300.0, 150.0),
    );

    let text_nodes = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_nodes.len(),
        1,
        "error must replace helper, not render alongside it"
    );
}
