//! `Tab`/`TabBar`/`DefaultTabController` widget-level mount/interaction
//! coverage, as end-to-end mount proof: a real pointer down+up reaches the
//! controller through [`flui_material::InkWell`]'s dispatch, the indicator is
//! painted where the controller's position says (animated, elastic, and
//! mirrored under RTL), a zero-tab bar mounts the documented 48px empty box,
//! and a `DefaultTabController` ancestor is reachable by a descendant
//! `TabBar`.

use crate::common;

use std::time::Duration;

use common::{LaidOut, lay_out, lay_out_animated, tight};
use flui_material::{
    DefaultTabController, Tab, TabBar, TabController, TabIndicatorAnimation, Theme, ThemeData,
};
use flui_sdk::animation::{ArcCurve, MotionOverride, MotionSpec, Vsync, curves};
use flui_sdk::painting::{DrawOp, TextDirection};
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::widgets::Directionality;
use flui_sdk::widgets::animated::VsyncScope;

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

/// Every painted indicator rect in the last frame, as `(left, top, right,
/// bottom)` in bar coordinates: the 2 px rects in the theme's indicator
/// color.
fn painted_indicators(laid: &LaidOut) -> Vec<(f64, f64, f64, f64)> {
    let primary = ThemeData::light().color_scheme.primary;
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { rect, paint }
                if paint.color == primary && (rect.height() - 2.0).abs() < 1e-9 =>
            {
                let (left, top) = command.transform.transform_point(rect.left(), rect.top());
                let (right, bottom) = command
                    .transform
                    .transform_point(rect.right(), rect.bottom());
                Some((left, top, right, bottom))
            }
            _ => None,
        })
        .collect()
}

/// The one painted indicator's `(left, right)` edges in bar coordinates.
pub(crate) fn painted_indicator(laid: &LaidOut) -> (f64, f64) {
    let rects = painted_indicators(laid);
    assert_eq!(
        rects.len(),
        1,
        "exactly one indicator is painted: {rects:?}"
    );
    (rects[0].0, rects[0].2)
}

/// A real pointer down+up over the second (of two, equal-width) tabs reaches
/// the controller through [`flui_material::InkWell`]'s dispatch.
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
        "a tap in the second tab's cell must select index 1"
    );
}

/// The indicator paints over the bottom band of every tab but takes no
/// pointers: a tap on that band still reaches the tab under it.
pub fn a_tap_on_the_indicator_band_reaches_the_tab() {
    let controller = TabController::new(2, 0);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            TabBar::secondary(two_tabs()).controller(controller.clone()),
        ),
        tight(200.0, 48.0),
    );

    // The second tab's indicator band spans y in [46, 48).
    laid.dispatch_pointer_down(150.0, 47.0);
    laid.dispatch_pointer_up(150.0, 47.0);

    assert_eq!(controller.index(), 1, "the band tap selects the tab");
}

/// A `DefaultTabController` whose `length` shrinks while its last tab is
/// selected re-creates the controller with a clamped index — end to end,
/// through a real root swap: mounting does not panic, and a subsequent tap
/// still dispatches correctly through the re-created controller.
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

    // The clamp itself must already be painted before any post-shrink tap:
    // a later tap on index 1 would paint index 1 whether or not the clamp
    // ran.
    assert_eq!(
        painted_indicator(&laid),
        (150.0, 300.0),
        "the re-created controller must already select index 1 right after the shrink"
    );

    // A tap on the (new) first tab must still dispatch through the
    // re-created controller; the indicator then animates there.
    laid.dispatch_pointer_down(75.0, 24.0);
    laid.dispatch_pointer_up(75.0, 24.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(TabController::DEFAULT_DURATION);

    assert_eq!(
        painted_indicator(&laid),
        (0.0, 150.0),
        "the post-shrink tap must move the single indicator to index 0"
    );
}

/// Loose height permits the bar's own allocation to be observed, rather than
/// forcing the expected size from the parent.
pub fn small_tab_height_overrides_determine_the_mounted_bar_height() {
    assert_mounted_tab_height(
        vec![
            Tab::new().text("One").height(20.0),
            Tab::new().text("Two").height(28.0),
        ],
        30.0,
    );
}

pub fn mixed_and_empty_tab_bars_keep_their_content_height_rules() {
    assert_mounted_tab_height(
        vec![Tab::new().text("One").height(20.0), Tab::new().text("Two")],
        48.0,
    );
    assert_mounted_tab_height(
        vec![
            Tab::new().text("One").height(60.0),
            Tab::new().text("Two").height(28.0),
        ],
        62.0,
    );
    assert_mounted_tab_height(Vec::new(), 48.0);
}

fn assert_mounted_tab_height(tabs: Vec<Tab>, expected: f64) {
    use flui_sdk::widgets::PreferredSizeView;
    let tab_count = tabs.len();
    let controller = TabController::new(tab_count, 0);
    let bar = TabBar::secondary(tabs).controller(controller);
    assert_eq!(bar.preferred_size().height, expected);
    let laid = lay_out(
        themed(ThemeData::light(), bar),
        bar_constraints(200.0, 120.0),
    );
    assert_eq!(laid.size(laid.root()).height, expected);
    let indicators = painted_indicators(&laid);
    if tab_count == 0 {
        assert!(indicators.is_empty(), "an empty bar paints no indicator");
        return;
    }
    assert_eq!(indicators.len(), 1);
    let (left, top, right, bottom) = indicators[0];
    assert_eq!(
        (left, right),
        (0.0, 100.0),
        "the first of two tabs is selected"
    );
    assert_eq!(
        (top, bottom),
        (expected - 2.0, expected),
        "the indicator sits on the bar's bottom edge"
    );
}

const BAR_WIDTH: f64 = 400.0;
const TAB_WIDTH: f64 = BAR_WIDTH / 4.0;

pub(crate) fn four_tabs() -> Vec<Tab> {
    ["One", "Two", "Three", "Four"]
        .into_iter()
        .map(|label| Tab::new().text(label))
        .collect()
}

/// A linear 300 ms motion, so a half-time sample sits clearly between tabs.
fn linear_controller() -> TabController {
    TabController::new(4, 0).with_motion(MotionSpec::Curve {
        duration: Duration::from_millis(300),
        curve: ArcCurve::new(curves::LINEAR),
    })
}

/// A 400 px bar of four tabs on `vsync`'s clock.
fn mount_bar(
    controller: &TabController,
    animation: TabIndicatorAnimation,
    direction: TextDirection,
    vsync: &Vsync,
) -> LaidOut {
    lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Directionality::new(
                direction,
                themed(
                    ThemeData::light(),
                    TabBar::secondary(four_tabs())
                        .controller(controller.clone())
                        .indicator_animation(animation),
                ),
            ),
        ),
        tight(BAR_WIDTH, 48.0),
        vsync.clone(),
    )
}

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-6
}

/// A tap with no `TabBarView` attached commits the index at once and
/// animates the position, and the linear indicator, through the tabs between,
/// settling on the tapped tab.
pub fn a_tap_without_a_view_animates_the_position_and_settles() {
    let vsync = Vsync::new();
    let controller = linear_controller();
    let mut laid = mount_bar(
        &controller,
        TabIndicatorAnimation::Linear,
        TextDirection::Ltr,
        &vsync,
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(painted_indicator(&laid), (0.0, TAB_WIDTH));

    // The third tab's midpoint.
    laid.dispatch_pointer_down(250.0, 24.0);
    laid.dispatch_pointer_up(250.0, 24.0);
    assert_eq!(controller.index(), 2, "the tap commits the index at once");

    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    let position = controller.position().value();
    assert!(
        position > 0.5 && position < 1.5,
        "half way through the run the position is between the tabs, got {position}"
    );
    let (left, right) = painted_indicator(&laid);
    assert!(
        close(left, position * TAB_WIDTH) && close(right - left, TAB_WIDTH),
        "the linear indicator is the tab-wide rect at the position: {left}..{right} at {position}"
    );

    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        controller.position().value(),
        2.0,
        "the run settles on the tab"
    );
    assert_eq!(painted_indicator(&laid), (2.0 * TAB_WIDTH, 3.0 * TAB_WIDTH));
}

/// Under `Elastic` the leading edge leads and the trailing edge follows: the
/// indicator is wider mid-way than on either tab.
pub fn the_elastic_indicator_stretches_mid_way_between_tabs() {
    let vsync = Vsync::new();
    let controller = linear_controller();
    let mut laid = mount_bar(
        &controller,
        TabIndicatorAnimation::Elastic,
        TextDirection::Ltr,
        &vsync,
    );
    laid.pump_for(Duration::ZERO);
    let (left, right) = painted_indicator(&laid);
    let at_start = right - left;

    controller.animate_to(1);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    let position = controller.position().value();
    assert!(
        position > 0.25 && position < 0.75,
        "mid-way, got {position}"
    );
    let (left, right) = painted_indicator(&laid);
    let mid_way = right - left;

    laid.pump_for(Duration::from_millis(200));
    let (left, right) = painted_indicator(&laid);
    let at_end = right - left;

    assert!(close(at_start, TAB_WIDTH) && close(at_end, TAB_WIDTH));
    assert!(
        mid_way > at_start + 10.0 && mid_way > at_end + 10.0,
        "the elastic indicator stretches mid-way: {at_start} -> {mid_way} -> {at_end}"
    );
}

/// Under a right-to-left `Directionality` the first tab is rightmost and the
/// indicator moves leftward with the position.
pub fn the_indicator_mirrors_in_a_right_to_left_row() {
    let vsync = Vsync::new();
    let controller = linear_controller();
    let mut laid = mount_bar(
        &controller,
        TabIndicatorAnimation::Linear,
        TextDirection::Rtl,
        &vsync,
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(painted_indicator(&laid), (BAR_WIDTH - TAB_WIDTH, BAR_WIDTH));

    controller.animate_to(1);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    let position = controller.position().value();
    let (left, right) = painted_indicator(&laid);
    assert!(
        position > 0.25
            && position < 0.75
            && close(right, BAR_WIDTH - position * TAB_WIDTH)
            && close(right - left, TAB_WIDTH),
        "mirrored mid-way: {left}..{right} at {position}"
    );

    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        painted_indicator(&laid),
        (BAR_WIDTH - 2.0 * TAB_WIDTH, BAR_WIDTH - TAB_WIDTH)
    );
}

/// Reduced motion settles the owned position on the first frame after a
/// tab change; with full motion the same first frame still shows the old tab.
pub fn reduced_motion_settles_the_owned_position_on_the_first_frame() {
    for (preference, expected) in [(MotionOverride::Full, 0.0), (MotionOverride::Reduce, 1.0)] {
        let vsync = Vsync::new();
        let controller = linear_controller();
        let mut laid = mount_bar(
            &controller,
            TabIndicatorAnimation::Linear,
            TextDirection::Ltr,
            &vsync,
        );
        laid.set_motion_override(preference);
        laid.pump_for(Duration::ZERO);

        controller.animate_to(1);
        laid.pump_for(Duration::ZERO);
        assert_eq!(
            controller.position().value(),
            expected,
            "first frame after the change under {preference:?}"
        );
        assert_eq!(
            painted_indicator(&laid).0,
            expected * TAB_WIDTH,
            "indicator under {preference:?}"
        );
    }
}
