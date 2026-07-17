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
//! **Semantics**: `RenderSemanticsAnnotations`'s own `Diagnosticable` surface
//! (`crates/flui-objects/src/proxy/semantics.rs`) exposes only
//! `container`/`explicit_child_nodes`/`exclude_semantics`/
//! `block_user_actions`/`has_semantics` — not the finer-grained
//! `role`/`selected`/`enabled`/`button` flags a `Semantics` builder sets, so
//! this file proves structural presence (one annotated node per destination
//! plus the outer tab-bar container, each carrying real semantics content),
//! not the individual flag values.
//!
//! **Constraint shape matters here**: every test mounts under a *tight
//! width, loose height* root (see [`bar_constraints`]), matching what
//! [`crate::common`]'s harness gives any real consumer (e.g. `Scaffold`'s
//! `bottom_navigation_bar` slot measures with `full_width_loose_height` —
//! see `scaffold.rs`). A fully tight root defeats `NavigationBar`'s own
//! `SizedBox::height(80)` clamp (a tight incoming height constraint wins
//! over the local override), which would silently make every geometry
//! assertion below test the wrong height band.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size};
use flui_material::{NavigationBar, NavigationDestination, Theme, ThemeData};
use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_widgets::icon::IconData;
use flui_widgets::{Icon, MediaQuery, MediaQueryData};

/// Tight width, loose (`0..height`) height — see the module docs' note on
/// why a fully-tight root is the wrong shape to mount a `NavigationBar`
/// under.
fn bar_constraints(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::new(px(width), px(width), px(0.0), px(height))
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

/// The `Row`'s render id — disambiguated from the 3 per-destination
/// `Column`s (both compile to `RenderFlex`) by child count: only the Row
/// has exactly one child per destination.
fn row_id(laid: &common::LaidOut, destination_count: usize) -> flui_foundation::RenderId {
    laid.find_all_by_render_type("RenderFlex")
        .into_iter()
        .find(|&id| laid.children(id).len() == destination_count)
        .expect("a RenderFlex with one child per destination (the Row) must be mounted")
}

/// The Row's children, left to right — true destination order, independent
/// of the render arena's own id-allocation order (which need not match
/// left-to-right tree order; see the module docs).
fn destination_cells(
    laid: &common::LaidOut,
    destination_count: usize,
) -> Vec<flui_foundation::RenderId> {
    laid.children(row_id(laid, destination_count))
}

/// The single indicator `Material` (`RenderPhysicalShape`, the only render
/// object under a destination cell reporting a `"color"` diagnostics
/// property) nested under `cell`.
///
/// # Panics
///
/// Panics if `cell` contains zero or more than one such node — a
/// destination is expected to mount exactly one indicator fill.
fn indicator_in_cell(
    laid: &common::LaidOut,
    cell: flui_foundation::RenderId,
) -> flui_foundation::RenderId {
    fn collect(
        laid: &common::LaidOut,
        id: flui_foundation::RenderId,
        out: &mut Vec<flui_foundation::RenderId>,
    ) {
        if laid.render_property(id, "color").is_some() {
            out.push(id);
        }
        for child in laid.children(id) {
            collect(laid, child, out);
        }
    }
    let mut found = Vec::new();
    collect(laid, cell, &mut found);
    match found.as_slice() {
        [id] => *id,
        other => panic!(
            "expected exactly one color-bearing render node (the indicator) under this \
             destination cell, found {}",
            other.len()
        ),
    }
}

#[test]
fn destinations_lay_out_at_equal_width() {
    let laid = lay_out(
        themed(NavigationBar::new(three_destinations())),
        bar_constraints(300.0, 800.0),
    );

    let cells = destination_cells(&laid, 3);
    assert_eq!(cells.len(), 3, "one Row child per destination");
    for cell in cells {
        assert_eq!(
            laid.size(cell).width,
            px(100.0),
            "each destination must take an equal 1/3 share of the bar's width",
        );
        assert_eq!(
            laid.size(cell).height,
            px(80.0),
            "each destination cell must span the bar's full 80dp height",
        );
    }
}

#[test]
fn tap_fires_on_destination_selected_with_the_tapped_index() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            NavigationBar::new(three_destinations()).on_destination_selected(move |index| {
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

#[test]
fn tapping_a_disabled_destination_does_not_fire_the_callback() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let mut destinations = three_destinations();
    destinations[1] = NavigationDestination::new(icon(), "Profile").enabled(false);

    let laid = lay_out(
        themed(
            NavigationBar::new(destinations).on_destination_selected(move |index| {
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

#[test]
fn selected_index_change_moves_the_indicator_fill() {
    let colors = ThemeData::light().color_scheme;
    let color_at = |laid: &common::LaidOut, id: flui_foundation::RenderId| {
        laid.render_property(id, "color")
            .expect("RenderPhysicalShape reports a \"color\" diagnostics property")
    };
    let transparent = format!("{:?}", flui_types::styling::Color::TRANSPARENT);
    let filled = format!("{:?}", colors.secondary_container);

    let mut laid = lay_out(
        themed(NavigationBar::new(three_destinations()).selected_index(0)),
        bar_constraints(300.0, 800.0),
    );
    let cells = destination_cells(&laid, 3);
    let indicators: Vec<_> = cells
        .iter()
        .map(|&cell| indicator_in_cell(&laid, cell))
        .collect();

    assert_eq!(
        color_at(&laid, indicators[0]),
        filled,
        "the selected (index 0) destination's indicator must be filled",
    );
    assert_eq!(
        color_at(&laid, indicators[1]),
        transparent,
        "an unselected destination's indicator must be fully transparent",
    );
    assert_eq!(
        color_at(&laid, indicators[2]),
        transparent,
        "an unselected destination's indicator must be fully transparent",
    );

    laid.pump_widget(themed(
        NavigationBar::new(three_destinations()).selected_index(1),
    ));
    let cells_after = destination_cells(&laid, 3);
    let indicators_after: Vec<_> = cells_after
        .iter()
        .map(|&cell| indicator_in_cell(&laid, cell))
        .collect();

    assert_eq!(
        color_at(&laid, indicators_after[0]),
        transparent,
        "after selecting index 1, the now-unselected index 0 indicator must go transparent",
    );
    assert_eq!(
        color_at(&laid, indicators_after[1]),
        filled,
        "after selecting index 1, its indicator must become filled",
    );
    assert_eq!(
        color_at(&laid, indicators_after[2]),
        transparent,
        "index 2 stays unselected and its indicator must remain transparent",
    );
}

#[test]
fn theme_indicator_color_beats_the_m3_default() {
    let overridden = flui_types::styling::Color::rgb(9, 9, 9);
    let laid = lay_out(
        themed(
            NavigationBar::new(three_destinations())
                .selected_index(0)
                .indicator_color(overridden),
        ),
        bar_constraints(300.0, 800.0),
    );

    let cells = destination_cells(&laid, 3);
    let indicator = indicator_in_cell(&laid, cells[0]);
    assert_eq!(
        laid.render_property(indicator, "color"),
        Some(format!("{overridden:?}")),
        "a widget-level indicator_color override must reach the selected destination's \
         rendered fill",
    );
}

#[test]
fn mounting_creates_a_tab_bar_container_and_one_annotated_node_per_destination() {
    let laid = lay_out(
        themed(NavigationBar::new(three_destinations())),
        bar_constraints(300.0, 800.0),
    );

    let semantics_nodes = laid.find_all_by_render_type("RenderSemanticsAnnotations");
    // One container node for the bar itself (`SemanticsRole::TabBar`) plus
    // one per destination (`SemanticsRole::Tab`).
    assert_eq!(
        semantics_nodes.len(),
        1 + three_destinations().len(),
        "expected one tab-bar container node plus one tab node per destination",
    );

    // `RenderSemanticsAnnotations::debug_fill_properties` only *emits* a
    // flag property when it's true (`DiagnosticsBuilder::add_flag` omits a
    // false flag rather than writing `"false"`) — so `container`/
    // `has_semantics` being present at all (`Some`, any string) IS the
    // true-flag proof; a `None` means that particular flag is unset on that
    // node, not "not yet populated".
    let bar_container = semantics_nodes
        .iter()
        .find(|&&id| laid.size(id) == size(300.0, 80.0))
        .copied()
        .expect("the outer tab-bar Semantics node must span the bar's full 300×80 size");
    assert!(
        laid.render_property(bar_container, "container").is_some(),
        "the outer tab-bar Semantics node must set container: true",
    );

    let destination_nodes: Vec<_> = semantics_nodes
        .into_iter()
        .filter(|&id| id != bar_container)
        .collect();
    assert_eq!(destination_nodes.len(), 3, "one tab node per destination");
    for id in destination_nodes {
        assert!(
            laid.render_property(id, "has_semantics").is_some(),
            "each destination's Semantics node (role: tab, selected, enabled, button) must \
             carry real content, not an empty passthrough",
        );
    }
}

#[test]
fn bar_height_and_elevation_match_the_m3_defaults() {
    let laid = lay_out(
        themed(NavigationBar::new(three_destinations())),
        bar_constraints(300.0, 800.0),
    );

    // Every destination also mounts its own (always-reserved, see
    // `navigation_bar.rs`'s module docs) indicator `RenderPhysicalShape` at
    // a fixed 64×32 — the bar's own background surface is the only one
    // sized to the full bar, so size disambiguates it from the three
    // indicators.
    let material = laid
        .find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .find(|&id| laid.size(id) == size(300.0, 80.0))
        .expect("NavigationBar must compose a full-size top-level Material surface");
    assert_eq!(
        laid.render_property(material, "elevation")
            .and_then(|value| value.parse::<f32>().ok()),
        Some(3.0),
        "_NavigationBarDefaultsM3.elevation is 3.0",
    );
}
