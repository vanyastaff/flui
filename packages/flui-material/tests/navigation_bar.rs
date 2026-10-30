//! `NavigationBar` widget-level mount/interaction coverage.
//!
//! Complements `navigation_bar.rs`'s own unit tests (M3 default token-table
//! probes for icon/label color, the widget → theme → default geometry
//! cascade) with end-to-end mount proof: the destinations lay out at equal
//! width, a real down+up reaches [`flui_material::NavigationBar::on_destination_selected`]
//! with the tapped index, and a `selected_index` rebuild animates the
//! indicator from the previous destination to the new one, read from the
//! recorded display list at virtual times.
//!
//! **Not covered here** (see `navigation_bar.rs`'s own unit tests instead,
//! since neither needs a render tree): the M3 default icon/label color
//! branch order and combined-state pins, and the widget → theme → default
//! geometry cascade (`resolve_bar_geometry`).
//!
//! **Semantics**: `destinations_announce_as_tabs_with_their_selection` reads
//! the published accessibility tree: a tab list whose destinations are tabs,
//! each with its selection.
//!
//! **Constraint shape matters here**: every test mounts under a *tight
//! width, loose height* root (see [`bar_constraints`]), matching what
//! [`crate::common`]'s harness gives any real consumer (e.g. `Scaffold`'s
//! `bottom_navigation_bar` slot measures with `full_width_loose_height` —
//! see `scaffold.rs`). A fully tight root defeats `NavigationBar`'s own
//! `SizedBox::height(80)` clamp (a tight incoming height constraint wins
//! over the local override), which would silently make every geometry
//! assertion below test the wrong height band.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out, lay_out_animated};
use flui_material::{NavigationBar, NavigationDestination, Theme, ThemeData};
use flui_painting::DrawOp;
use flui_sdk::animation::{MotionOverride, Vsync};
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::widgets::icon::IconData;
use flui_sdk::widgets::{Icon, MediaQuery, MediaQueryData, VsyncScope};
use flui_testing::a11y::Role;

/// Tight width, loose (`0..height`) height — see the module docs' note on
/// why a fully-tight root is the wrong shape to mount a `NavigationBar`
/// under.
fn bar_constraints(width: f64, height: f64) -> BoxConstraints {
    BoxConstraints::new(width, width, 0.0, height)
}

/// Every `NavigationBar` needs a [`Theme`] ancestor (`Theme::of` panics
/// without one) and a [`MediaQuery`] ancestor (its internal `SafeArea`
/// panics without one, same as `tests/scaffold.rs`'s app-bar coverage) —
/// mirrors `tests/switch.rs`'s own `themed` helper, extended with the
/// `MediaQuery` wrap this component additionally needs.
fn themed(bar: NavigationBar) -> Theme {
    Theme::new(
        ThemeData::light(),
        MediaQuery::new(MediaQueryData::default(), bar),
    )
}

fn icon() -> Icon {
    Icon::new(IconData::new(0xE88A))
}

fn three_destinations() -> Vec<NavigationDestination> {
    vec![
        NavigationDestination::new(icon(), "Home"),
        NavigationDestination::new(icon(), "Profile"),
        NavigationDestination::new(icon(), "Settings"),
    ]
}

pub fn tap_fires_on_destination_selected_with_the_tapped_index() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            NavigationBar::new(three_destinations()).on_destination_selected(move |_cx, index| {
                *recorder.borrow_mut() = Some(index);
            }),
        ),
        bar_constraints(300.0, 800.0),
    );

    // Each destination cell spans 100px, 80dp tall; the second
    // destination's midpoint is (150, 40).
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_up(150.0, 40.0);

    assert_eq!(
        *observed.borrow(),
        Some(1),
        "a tap in the second destination's cell must fire on_destination_selected(1)",
    );
}

pub fn tapping_a_disabled_destination_does_not_fire_the_callback() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let mut destinations = three_destinations();
    destinations[1] = NavigationDestination::new(icon(), "Profile").enabled(false);

    let laid = lay_out(
        themed(
            NavigationBar::new(destinations).on_destination_selected(move |_cx, index| {
                *recorder.borrow_mut() = Some(index);
            }),
        ),
        bar_constraints(300.0, 800.0),
    );

    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_up(150.0, 40.0);

    assert_eq!(
        *observed.borrow(),
        None,
        "a disabled destination must swallow the tap and never fire the callback",
    );
}

fn assert_invalid_configuration_rejected(make_bar: impl FnOnce() -> NavigationBar) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(make_bar));
    assert!(
        result.is_err(),
        "invalid navigation configuration must fail in every profile"
    );
}

pub fn an_empty_destination_list_is_rejected() {
    assert_invalid_configuration_rejected(|| NavigationBar::new(Vec::new()));
}

pub fn a_single_destination_is_rejected() {
    assert_invalid_configuration_rejected(|| {
        NavigationBar::new(vec![NavigationDestination::new(icon(), "Home")])
    });
}

pub fn an_index_at_the_destination_count_is_rejected() {
    assert_invalid_configuration_rejected(|| {
        NavigationBar::new(three_destinations()).selected_index(3)
    });
}

pub fn an_unrepresentable_destination_index_is_rejected() {
    assert_invalid_configuration_rejected(|| {
        NavigationBar::new(three_destinations()).selected_index(usize::MAX)
    });
}

/// The bar is a tab list and each destination one tab, whose selection a
/// screen reader announces: selected for the current one, explicitly
/// unselected for the others.
pub fn destinations_announce_as_tabs_with_their_selection() {
    let mut laid = lay_out(
        themed(NavigationBar::new(three_destinations()).selected_index(1)),
        bar_constraints(300.0, 800.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    tree.find(Role::TabList)
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    let tabs: Vec<Option<bool>> = tree
        .find_all(Role::Tab)
        .iter()
        .map(|tab| tab.raw().is_selected())
        .collect();
    assert_eq!(
        tabs,
        [Some(false), Some(true), Some(false)],
        "{}",
        tree.describe()
    );
}

/// The full indicator width, M3's 64.
const INDICATOR_WIDTH: f64 = 64.0;

fn animated_bar(vsync: &Vsync, selected_index: usize) -> impl flui_sdk::view::View {
    VsyncScope::new(
        vsync.clone(),
        themed(NavigationBar::new(three_destinations()).selected_index(selected_index)),
    )
}

/// Each painted indicator as `(centre x, width)`, left to right: the
/// stadiums filled with the theme's indicator colour.
fn painted_indicators(laid: &LaidOut) -> Vec<(f64, f64)> {
    let indicator = ThemeData::light().color_scheme.secondary_container;
    let mut indicators: Vec<(f64, f64)> = laid
        .draw_ops()
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::RRect { rrect, paint } if paint.color == indicator => {
                let centre = rrect.center();
                Some((
                    command.transform.transform_point(centre.x, centre.y).0,
                    rrect.width(),
                ))
            }
            _ => None,
        })
        .collect();
    indicators.sort_by(|a, b| a.0.total_cmp(&b.0));
    indicators
}

/// Mounts the bar on its first destination, selects the second and pumps
/// the frame that starts the change.
fn second_selected(policy: MotionOverride) -> LaidOut {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        animated_bar(&vsync, 0),
        bar_constraints(300.0, 800.0),
        vsync.clone(),
    );
    laid.set_motion_override(policy);
    laid.pump_for(Duration::ZERO);
    let before = painted_indicators(&laid);
    assert!(
        before.len() == 1 && (before[0].1 - INDICATOR_WIDTH).abs() < 1e-6,
        "only the first destination shows a full indicator, got {before:?}"
    );
    laid.pump_widget(animated_bar(&vsync, 1));
    laid.pump_for(Duration::ZERO);
    laid
}

/// On an intermediate frame of a selection change the newly selected
/// destination's indicator is partly expanded and the previous one partly
/// collapsed, each moving toward its end; both settle.
pub fn a_selection_change_expands_the_new_indicator_and_collapses_the_old() {
    let mut laid = second_selected(MotionOverride::Full);
    laid.pump_for(Duration::from_millis(150));
    let early = painted_indicators(&laid);
    laid.pump_for(Duration::from_millis(50));
    let later = painted_indicators(&laid);
    for frame in [&early, &later] {
        assert_eq!(
            frame.len(),
            2,
            "both indicators paint mid-change: {frame:?}"
        );
        for (_, width) in frame {
            assert!(
                *width > 1e-6 && *width < INDICATOR_WIDTH - 1e-6,
                "each indicator is part way: {frame:?}"
            );
        }
    }
    assert!(
        later[0].1 < early[0].1 && later[1].1 > early[1].1,
        "the previous (left) indicator collapses and the new (right) one expands: \
         {early:?} then {later:?}"
    );

    laid.pump_for(Duration::from_secs(1));
    let settled = painted_indicators(&laid);
    assert!(
        settled.len() == 1
            && (settled[0].1 - INDICATOR_WIDTH).abs() < 1e-6
            && (settled[0].0 - 150.0).abs() < 1e-6,
        "only the second destination's full indicator remains, got {settled:?}"
    );
}

/// Under reduced motion the first frame after a selection change shows the
/// new selection, where full motion has not moved yet.
pub fn reduced_motion_shows_a_selection_change_on_its_first_frame() {
    let full = painted_indicators(&second_selected(MotionOverride::Full));
    assert!(
        full.len() == 1 && (full[0].0 - 50.0).abs() < 1e-6,
        "full motion still shows the first destination on the first frame, got {full:?}"
    );

    let reduced = painted_indicators(&second_selected(MotionOverride::Reduce));
    assert!(
        reduced.len() == 1
            && (reduced[0].1 - INDICATOR_WIDTH).abs() < 1e-6
            && (reduced[0].0 - 150.0).abs() < 1e-6,
        "reduced motion shows the second destination's full indicator at once, got {reduced:?}"
    );
}

/// The scale of every composited transform that scales, rounded to hundredths.
fn scaled_layers(laid: &LaidOut) -> Vec<f64> {
    laid.transform_layer_matrices()
        .iter()
        .map(|matrix| {
            let (x0, y0) = matrix.transform_point(0.0, 0.0);
            let (x1, y1) = matrix.transform_point(1.0, 0.0);
            ((x1 - x0).hypot(y1 - y0) * 100.0).round() / 100.0
        })
        .filter(|scale| (scale - 1.0).abs() > 1e-9)
        .collect()
}

/// A newly selected destination's icon bounces once, and the destination it leaves does
/// not; under reduced motion nothing bounces.
pub fn a_newly_selected_icon_bounces() {
    let mut laid = second_selected(MotionOverride::Full);
    assert_eq!(
        scaled_layers(&laid),
        [] as [f64; 0],
        "the bounce starts at rest"
    );
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scaled_layers(&laid),
        [1.2],
        "one icon, the newly selected one, at the bounce's peak"
    );
    laid.pump_for(Duration::from_secs(1));
    assert_eq!(scaled_layers(&laid), [] as [f64; 0], "the bounce settles");

    let mut reduced = second_selected(MotionOverride::Reduce);
    reduced.pump_for(Duration::from_millis(150));
    assert_eq!(
        scaled_layers(&reduced),
        [] as [f64; 0],
        "no bounce under reduced motion"
    );
}

/// A destination whose selected icon is built differently from its plain one still bounces
/// on selection.
pub fn a_distinct_selected_icon_bounces_too() {
    let destinations = || {
        vec![
            NavigationDestination::new(icon(), "Home"),
            NavigationDestination::new(icon(), "Profile").selected_icon(
                flui_sdk::widgets::Padding::all(0.0).child(Icon::new(IconData::new(0xE7FD))),
            ),
        ]
    };
    let bar = |vsync: &Vsync, selected| {
        VsyncScope::new(
            vsync.clone(),
            themed(NavigationBar::new(destinations()).selected_index(selected)),
        )
    };
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(bar(&vsync, 0), bar_constraints(300.0, 800.0), vsync.clone());
    laid.pump_for(Duration::ZERO);
    laid.pump_widget(bar(&vsync, 1));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scaled_layers(&laid),
        [1.2],
        "the selected icon at the bounce's peak"
    );
}
