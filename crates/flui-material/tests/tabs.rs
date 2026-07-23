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

mod common;

use common::{lay_out, tight};
use flui_material::{
    DefaultTabController, Tab, TabBar, TabBarThemeData, TabController, Theme, ThemeData,
    ThemeDataOverrides,
};
use flui_rendering::constraints::BoxConstraints;
use flui_types::Color;
use flui_types::geometry::px;

fn two_tabs() -> Vec<Tab> {
    vec![Tab::new().text("One"), Tab::new().text("Two")]
}

/// Tight width, loose (`0..height`) height — a `TabBar`'s own requested
/// height must win over a merely-permissive parent, the same reasoning
/// `tests/navigation_bar.rs`'s `bar_constraints` and `tests/divider.rs`'s
/// module doc give for why a fully-tight root would test the wrong height.
fn bar_constraints(width: f32, max_height: f32) -> BoxConstraints {
    BoxConstraints::new(px(width), px(width), px(0.0), px(max_height))
}

fn themed(theme: ThemeData, child: impl flui_view::prelude::IntoView) -> Theme {
    Theme::new(theme, child)
}

/// Unmounting a `TabBar` removes its listener from the (outliving)
/// `TabController` it was subscribed to — without `TabBarState::dispose`,
/// every unmount leaks an `Rc` closure that calls `rebuild.schedule(reason)` on a
/// `RebuildHandle` whose element no longer exists, and repeated mount/unmount
/// cycles against a long-lived controller (an explicit `TabBar::controller`
/// shared with a sibling, or a `DefaultTabController` that outlives one
/// particular child) accumulate dead listeners forever.
///
/// Same "count seam" pattern as
/// `crates/flui-material/tests/text_field.rs`'s
/// `unmounting_removes_the_focus_listener_from_the_process_wide_manager`:
/// the `TabBar` is a `Column` CHILD here, not the mounted root, so removing
/// it from the children list goes through ordinary list reconciliation
/// (ending in `dispose`) rather than a same-type root-config swap
/// (`pump_widget`/`swap_root_view` on the ROOT documents itself as NOT a
/// full deactivate-and-remount in that same test's module doc).
///
/// Mutation red-check (run and reverted, see the review evidence): deleting
/// `TabBarState::dispose`'s body makes `after_removal` stay at `while_mounted`
/// (`1`) instead of returning to `before_mount` (`0`) — confirmed by running
/// this test against that mutation before restoring the real `dispose`.
#[test]
fn unmounting_a_tab_bar_removes_its_listener_from_the_controller() {
    use flui_view::{IntoView, ViewExt};
    use flui_widgets::Column;

    let controller = TabController::new(2, 0);

    let before_mount = controller.listener_count();
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            Column::new(vec![
                TabBar::secondary(two_tabs())
                    .controller(controller.clone())
                    .into_view()
                    .boxed(),
            ]),
        ),
        tight(200.0, 48.0),
    );
    let while_mounted = controller.listener_count();
    assert!(
        while_mounted > before_mount,
        "mounting a TabBar must register its own listener on the controller"
    );

    // Remove the TabBar from the Column's children — an ordinary child
    // removal, not a root-type swap.
    laid.pump_widget(themed(
        ThemeData::light(),
        Column::new(Vec::<flui_view::BoxedView>::new()),
    ));

    let after_removal = controller.listener_count();
    assert_eq!(
        after_removal, before_mount,
        "removing a TabBar from the tree must remove its listener from the controller, not \
         leak it"
    );
}

/// A real pointer down+up over the second (of two, equal-width) tabs reaches
/// [`TabController::set_index`] through [`flui_material::InkWell`]'s
/// dispatch — not just a directly-called closure, as the unit tests in
/// `tabs.rs` exercise.
#[test]
fn tap_sets_the_controller_index_through_real_pointer_dispatch() {
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

/// A tap on the already-selected tab is the no-op [`TabController`] itself
/// already guarantees (`index == self.index()`) — this proves that no-op
/// reaches all the way through real dispatch too, not just direct
/// `set_index` calls.
#[test]
fn tapping_the_already_selected_tab_leaves_the_index_unchanged() {
    let controller = TabController::new(2, 0);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            TabBar::secondary(two_tabs()).controller(controller.clone()),
        ),
        tight(200.0, 48.0),
    );

    laid.dispatch_pointer_down(50.0, 24.0);
    laid.dispatch_pointer_up(50.0, 24.0);

    assert_eq!(controller.index(), 0);
}

/// The M3 secondary bar's divider (1dp, `outlineVariant`) reaches the
/// mounted tree as a full-bar-width [`flui_widgets::DecoratedBox`]-backed
/// fill, and a `TabBarThemeData.divider_color` override actually changes
/// what's painted — not just what `resolve_style` computes in isolation
/// (see `tabs.rs`'s own `resolve_style_theme_override_beats_the_default`
/// for that unit-level half of this contract).
#[test]
fn divider_theme_override_reaches_the_mounted_tree() {
    let themed_divider = Color::rgb(10, 20, 30);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        tab_bar_theme: Some(TabBarThemeData {
            divider_color: Some(themed_divider),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        themed(
            theme,
            TabBar::secondary(two_tabs()).controller(TabController::new(2, 0)),
        ),
        tight(200.0, 48.0),
    );

    // Every per-tab indicator band is ALSO a `RenderDecoratedBox` (a
    // `Container::color(...)`, same as the divider) — 100px wide, 2dp
    // tall. The divider is the one spanning the FULL bar width at 1dp
    // tall, which disambiguates it without walking exact tree structure.
    let divider = laid
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            size.height.get() == 1.0 && size.width.get() == 200.0
        })
        .expect("a full-width 1dp divider must be mounted");

    let decoration = laid
        .render_property(divider, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");

    assert!(
        decoration.contains(&format!("{themed_divider:?}")),
        "a configured tab_bar_theme.divider_color must reach the mounted divider — got \
         {decoration:?}"
    );
}

/// The selected tab's indicator band (2dp, `indicator_color`) is the OTHER
/// `RenderDecoratedBox` shape — 100px wide (one of two equal tabs), 2dp
/// tall — and a `TabBarThemeData.indicator_color` override reaches it too.
#[test]
fn indicator_theme_override_reaches_the_mounted_selected_tab_band() {
    let themed_indicator = Color::rgb(200, 30, 40);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        tab_bar_theme: Some(TabBarThemeData {
            indicator_color: Some(themed_indicator),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        themed(
            theme,
            TabBar::secondary(two_tabs()).controller(TabController::new(2, 0)),
        ),
        tight(200.0, 48.0),
    );

    // Both tabs' indicator bands are 100px wide, 2dp tall — the size alone
    // doesn't distinguish selected (opaque) from unselected (transparent);
    // search for the one whose decoration actually carries the themed
    // color instead of guessing which match is "first".
    let indicator_bands: Vec<_> = laid
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .filter(|&id| {
            let size = laid.size(id);
            size.height.get() == 2.0 && size.width.get() == 100.0
        })
        .collect();
    assert_eq!(
        indicator_bands.len(),
        2,
        "both tabs' indicator bands (one per tab) must be mounted"
    );

    let has_themed_band = indicator_bands.iter().any(|&id| {
        laid.render_property(id, "decoration")
            .is_some_and(|decoration| decoration.contains(&format!("{themed_indicator:?}")))
    });

    assert!(
        has_themed_band,
        "a configured tab_bar_theme.indicator_color must reach the selected tab's band"
    );
}

/// The indicator band sits at the BOTTOM of its 48px-tall cell (`dy ==
/// 46.0` — the 46px content area's bottom edge, spanning the reserved 2dp
/// gutter to the bar's own bottom edge), the divider sits at the very
/// bottom of the whole bar (`dy == 47.0` — its own 1dp occupies the last
/// pixel of that gutter), and the tab row (carrying the indicator bands) is
/// the STACK's later child, so it paints over the divider wherever a
/// band is opaque — Flutter parity: `_IndicatorPainter.paint`'s draw order
/// (divider's `canvas.drawLine` first, then `_painter!.paint` for the
/// indicator, `tabs.dart` oracle tag `3.44.0`) and `_TabBarState.build`'s
/// `TabBarIndicatorSize::Tab` geometry (the indicator rect's height spans
/// the full bar height, i.e. its top is `bar_height - indicator_weight`).
///
/// Neither `resolve_style_defaults_to_the_m3_secondary_token_table` (a pure
/// unit test) nor `divider_theme_override_reaches_the_mounted_tree`/
/// `indicator_theme_override_reaches_the_mounted_selected_tab_band` (mounted,
/// but width/height-only) constrain the VERTICAL position of either shape —
/// a `build_tab_cell` regression that puts the band at the top of the cell
/// (`Column::new(vec![band.boxed(), Expanded::new(styled).boxed()])`,
/// reversed from the correct order) still passes every one of them. This
/// test is the one that catches it.
///
/// Mutation red-check (run and reverted, see the review evidence): swapping
/// `build_tab_cell`'s `Column` children to `[band, Expanded::new(styled)]`
/// (band first/top instead of last/bottom) makes the `band_top` assertion
/// below fail (`0.0` instead of `46.0`) — confirmed by running this test
/// against that mutation before restoring the correct order.
#[test]
fn indicator_band_sits_at_the_bar_bottom_beneath_the_divider_and_paints_over_it() {
    let laid = lay_out(
        themed(
            ThemeData::light(),
            TabBar::secondary(two_tabs()).controller(TabController::new(2, 0)),
        ),
        tight(200.0, 48.0),
    );

    let divider = laid
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            size.height.get() == 1.0 && size.width.get() == 200.0
        })
        .expect("a full-width 1dp divider must be mounted");
    let band = laid
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            size.height.get() == 2.0 && size.width.get() == 100.0
        })
        .expect("a per-tab 2dp indicator band must be mounted");

    let divider_top = laid.absolute_offset(divider).dy.get();
    let band_top = laid.absolute_offset(band).dy.get();
    assert_eq!(
        band_top, 46.0,
        "the indicator band must sit at the BOTTOM of the 48px bar (46px content + 2dp band), \
         not the top"
    );
    assert_eq!(
        divider_top, 47.0,
        "the 1dp divider must sit at the very bottom of the 48px bar"
    );

    // Structural "paints over" proof: the harness documents render-tree
    // child order as paint AND hit-test order, with a LATER child painting
    // on top (`tests/common/mod.rs`'s `LaidOut::children` doc comment). The
    // tab row (which carries the indicator bands) must be the STACK's
    // second (later) child, with the divider strip first — so wherever a
    // band is opaque, it paints over the divider beneath it.
    let stack = laid
        .find_by_render_type("RenderStack")
        .expect("the divider and tab row must be layered in a Stack");
    let stack_children = laid.children(stack);
    assert_eq!(
        stack_children.len(),
        2,
        "the Stack must have exactly two layers: the divider strip and the tab row"
    );
    assert!(
        subtree_contains(&laid, stack_children[0], divider),
        "the Stack's FIRST (earlier-painted) child must be the divider layer"
    );
    assert!(
        subtree_contains(&laid, stack_children[1], band),
        "the Stack's SECOND (later-painted, on top) child must be the tab row carrying the \
         indicator bands"
    );
}

/// Whether `id` is `root` itself or appears anywhere in `root`'s render
/// subtree — a small DFS helper for structural paint-order assertions.
fn subtree_contains(
    laid: &common::LaidOut,
    root: flui_foundation::RenderId,
    id: flui_foundation::RenderId,
) -> bool {
    if root == id {
        return true;
    }
    laid.children(root)
        .into_iter()
        .any(|child| subtree_contains(laid, child, id))
}

/// A zero-tab `TabBar` mounts the documented `48px` empty box (`TAB_HEIGHT +
/// indicator_weight`), not a collapsed/zero-height `Row` — Flutter parity:
/// `_TabBarState.build`'s zero-tabs early return. A (length-0) controller is
/// still required, matching the oracle: controller resolution happens
/// unconditionally, before `build` ever checks the tab count (see
/// `TabBar::build`'s doc comment on its zero-tabs branch).
#[test]
fn zero_tab_bar_mounts_a_48px_box() {
    let laid = lay_out(
        themed(
            ThemeData::light(),
            TabBar::secondary(vec![]).controller(TabController::new(0, 0)),
        ),
        bar_constraints(300.0, 100.0),
    );

    let root_size = laid.size(laid.root());
    assert_eq!(
        root_size.height.get(),
        48.0,
        "a zero-tab TabBar must report the TAB_HEIGHT + indicator_weight (48px) box"
    );
}

/// A `TabBar` with neither an explicit `controller` nor a
/// [`DefaultTabController`] ancestor panics loudly (Flutter parity:
/// `_updateTabController`'s `FlutterError`) instead of silently rendering
/// with no selection.
///
/// The panic itself happens inside `build()`, which this crate's build-error
/// boundary (`flui_view::element::behavior_commons::build_or_recover`)
/// catches and substitutes a render-less `ErrorView` for — so `#[should_panic]`
/// around the mount call would not observe THAT panic message directly. With
/// `TabBar` mounted as the sole root here, the substitution leaves nothing
/// to render, so `lay_out` itself panics next ("render root") — the same
/// documented mechanism `flui-cupertino/tests/tab_scaffold.rs`'s
/// `out_of_range_controller_index_panics_instead_of_silently_hiding_every_tab`
/// exercises for an identical reason.
#[test]
#[should_panic(expected = "render root")]
fn a_tab_bar_with_no_controller_and_no_default_tab_controller_ancestor_panics() {
    let _ = lay_out(
        themed(ThemeData::light(), TabBar::secondary(two_tabs())),
        tight(200.0, 48.0),
    );
}

/// A [`DefaultTabController`] ancestor is genuinely reachable by a
/// descendant `TabBar` with no explicit `controller` — mounting does not
/// panic (which it would, per the test above, with no controller reachable
/// at all), and a real pointer dispatch through it still reaches the
/// ancestor-owned [`TabController`]: tapping the second tab colors ITS
/// indicator band with the resolved indicator color and leaves the first
/// tab's band transparent.
#[test]
fn default_tab_controller_is_reachable_by_a_descendant_tab_bar_and_drives_its_indicator() {
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            DefaultTabController::new(2, TabBar::secondary(two_tabs())),
        ),
        tight(200.0, 48.0),
    );

    laid.dispatch_pointer_down(150.0, 24.0);
    laid.dispatch_pointer_up(150.0, 24.0);
    laid.pump();

    let indicator_color = ThemeData::light().color_scheme.primary;
    let bands = laid.find_all_by_render_type("RenderDecoratedBox");
    let opaque_bands: Vec<_> = bands
        .into_iter()
        .filter(|&id| laid.size(id).height.get() == 2.0 && laid.size(id).width.get() == 100.0)
        .filter(|&id| {
            laid.render_property(id, "decoration")
                .is_some_and(|decoration| decoration.contains(&format!("{indicator_color:?}")))
        })
        .collect();

    assert_eq!(
        opaque_bands.len(),
        1,
        "exactly one tab's indicator band must be opaque (the selected one) after the tap"
    );
    let selected_x = laid.absolute_offset(opaque_bands[0]).dx.get();
    assert_eq!(
        selected_x, 100.0,
        "the SECOND tab's band (starting at x=100) must be the opaque one after tapping it"
    );
}

/// A `DefaultTabController` whose `length` shrinks while its last tab is
/// selected re-creates the controller with a clamped index (Flutter parity:
/// `_DefaultTabControllerState.didUpdateWidget`; see `tab_controller.rs`'s
/// own `recreate_for_length_change_clamps_an_out_of_range_index_to_the_last_tab`
/// for the pure-function proof) — end to end, through a real root swap:
/// mounting does not panic, and a subsequent tap still dispatches correctly
/// through the re-created controller.
#[test]
fn default_tab_controller_survives_a_length_shrink_past_the_selected_index() {
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
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .filter(|&id| laid.size(id).height.get() == 2.0 && laid.size(id).width.get() == 150.0)
        .find(|&id| {
            laid.render_property(id, "decoration")
                .is_some_and(|decoration| decoration.contains(&format!("{indicator_color:?}")))
        })
        .map(|id| laid.absolute_offset(id).dx.get());
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
        .find_all_by_render_type("RenderDecoratedBox")
        .into_iter()
        .filter(|&id| laid.size(id).height.get() == 2.0 && laid.size(id).width.get() == 150.0)
        .filter(|&id| {
            laid.render_property(id, "decoration")
                .is_some_and(|decoration| decoration.contains(&format!("{indicator_color:?}")))
        })
        .collect();

    assert_eq!(
        opaque_bands.len(),
        1,
        "exactly one indicator band must be opaque after the post-shrink tap"
    );
}
