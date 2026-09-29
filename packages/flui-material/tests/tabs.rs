//! `Tab`/`TabBar`/`DefaultTabController` widget-level mount/interaction
//! coverage — complements `tabs.rs`'s/`tab_controller.rs`'s own unit tests
//! (M3 secondary default token-table probes, the pure `TabController`
//! state-machine, `bar_height`/`label_padding`/`indicator_rect` geometry)
//! with end-to-end mount proof: a real pointer down+up reaches
//! [`TabController::set_index`] through [`InkWell`]'s dispatch, the divider
//! and its theme override actually reach the mounted render tree (not just
//! `resolve_style` computed in isolation), a zero-tab bar mounts the
//! documented 48px empty box, and a `DefaultTabController` ancestor is
//! actually reachable by (and required by) a descendant `TabBar`.

use crate::common;

use common::{lay_out, tight};
use flui_material::{DefaultTabController, Tab, TabBar, TabController, Theme, ThemeData};
use flui_sdk::rendering::BoxConstraints;

fn two_tabs() -> Vec<Tab> {
    vec![Tab::new().text("One"), Tab::new().text("Two")]
}

/// Tight width, loose (`0..height`) height — a `TabBar`'s own requested
/// height must win over a merely-permissive parent, the same reasoning
/// `tests/navigation_bar.rs`'s `bar_constraints` and `tests/divider.rs`'s
/// module doc give for why a fully-tight root would test the wrong height.
fn bar_constraints(width: f64, max_height: f64) -> BoxConstraints {
    BoxConstraints::new(width, width, 0.0, max_height)
}

fn themed(theme: ThemeData, child: impl flui_sdk::view::prelude::IntoView) -> Theme {
    Theme::new(theme, child)
}

/// A real pointer down+up over the second (of two, equal-width) tabs reaches
/// [`TabController::set_index`] through [`flui_material::InkWell`]'s
/// dispatch — not just a directly-called closure, as the unit tests in
/// `tabs.rs` exercise.
pub fn tap_sets_the_controller_index_through_real_pointer_dispatch() {
    let controller = TabController::new(2, 0);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            TabBar::secondary(two_tabs()).controller(controller.clone()),
        ),
        tight(200.0, 48.0),
    );

    // Two equal-width tabs over a 200px bar: the second tab spans x in
    // [100, 200), 48px tall — (150, 24) is its midpoint.
    laid.dispatch_pointer_down(150.0, 24.0);
    laid.dispatch_pointer_up(150.0, 24.0);

    assert_eq!(
        controller.index(),
        1,
        "a tap in the second tab's cell must reach controller.set_index(1)"
    );
}

/// A `DefaultTabController` whose `length` shrinks while its last tab is
/// selected re-creates the controller with a clamped index (Flutter parity:
/// `_DefaultTabControllerState.didUpdateWidget`; see `tab_controller.rs`'s
/// own `recreate_for_length_change_clamps_an_out_of_range_index_to_the_last_tab`
/// for the pure-function proof) — end to end, through a real root swap:
/// mounting does not panic, and a subsequent tap still dispatches correctly
/// through the re-created controller.
pub fn default_tab_controller_survives_a_length_shrink_past_the_selected_index() {
    let three_tabs = vec![
        Tab::new().text("One"),
        Tab::new().text("Two"),
        Tab::new().text("Three"),
    ];
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            DefaultTabController::new(3, TabBar::secondary(three_tabs)),
        ),
        bar_constraints(300.0, 48.0),
    );

    // Select the last tab (index 2, x in [200, 300)) before the shrink.
    laid.dispatch_pointer_down(250.0, 24.0);
    laid.dispatch_pointer_up(250.0, 24.0);
    laid.pump();

    // Shrink from 3 tabs to 2 — the old index (2) is now out of range and
    // must clamp to 1, not panic or leave the bar unselectable.
    laid.pump_widget(themed(
        ThemeData::light(),
        DefaultTabController::new(2, TabBar::secondary(two_tabs())),
    ));

    // The clamp itself — index 1, not the tap about to happen — must
    // already be reflected BEFORE any post-shrink tap. Asserting this only
    // after tapping the very index the clamp should have produced would
    // mask a broken clamp (e.g. one that leaves the old, now out-of-range
    // index 2 in place): the subsequent tap on index 1 would still light up
    // exactly one band regardless of whether the clamp ran at all, since a
    // tap always selects whatever it lands on.
    let indicator_color = ThemeData::light().color_scheme.primary;
    let clamped_band_x = laid
        .find_all_by_render_type("RenderContainer")
        .into_iter()
        .filter(|&id| laid.size(id).height == 2.0 && laid.size(id).width == 150.0)
        .find(|&id| {
            laid.render_property(id, "color")
                .is_some_and(|color| color.contains(&format!("{indicator_color:?}")))
        })
        .map(|id| laid.absolute_offset(id).dx);
    assert_eq!(
        clamped_band_x,
        Some(150.0),
        "the re-created controller must already select index 1 (x=150) right after the shrink, \
         before any post-shrink tap"
    );

    // A tap on the (new) second tab must still dispatch through the
    // re-created controller and repaint a single opaque indicator band —
    // proof the swap left a live, correctly-wired TabController behind.
    // With 2 (not 3) tabs over the same 300px width, each tab is now
    // 150px wide — the second tab's midpoint is (225, 24), not (150, 24).
    laid.dispatch_pointer_down(225.0, 24.0);
    laid.dispatch_pointer_up(225.0, 24.0);
    laid.pump();

    let opaque_bands: Vec<_> = laid
        .find_all_by_render_type("RenderContainer")
        .into_iter()
        .filter(|&id| laid.size(id).height == 2.0 && laid.size(id).width == 150.0)
        .filter(|&id| {
            laid.render_property(id, "color")
                .is_some_and(|color| color.contains(&format!("{indicator_color:?}")))
        })
        .collect();

    assert_eq!(
        opaque_bands.len(),
        1,
        "exactly one indicator band must be opaque after the post-shrink tap"
    );
}
