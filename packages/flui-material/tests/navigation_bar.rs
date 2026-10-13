//! `NavigationBar` widget-level mount/interaction coverage.
//!
//! Complements `navigation_bar.rs`'s own unit tests (M3 default token-table
//! probes for icon/label color, the widget → theme → default geometry
//! cascade) with end-to-end mount proof: the destinations lay out at equal
//! width, a real down+up reaches [`flui_material::NavigationBar::on_destination_selected`]
//! with the tapped index, and a `selected_index` rebuild moves which
//! destination's indicator paints filled.
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

use common::lay_out;
use flui_material::{NavigationBar, NavigationDestination, Theme, ThemeData};
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::widgets::icon::IconData;
use flui_sdk::widgets::{Icon, MediaQuery, MediaQueryData};
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
