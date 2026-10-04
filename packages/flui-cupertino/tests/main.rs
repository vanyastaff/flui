//! flui-cupertino's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).
//!
//! `common` is declared once here and every suite imports it with
//! `use crate::common;` -- a per-suite `mod common;` would load
//! `tests/common/mod.rs` once per suite (`clippy::duplicate_mod`).

mod common;

#[path = "bottom_tab_bar.rs"]
mod bottom_tab_bar;

#[path = "button.rs"]
mod button;

#[path = "colors.rs"]
mod colors;

#[path = "cupertino_app.rs"]
mod cupertino_app;

#[path = "nav_bar.rs"]
mod nav_bar;

#[path = "page_scaffold.rs"]
mod page_scaffold;

#[path = "route.rs"]
mod route;

#[path = "tab_scaffold.rs"]
mod tab_scaffold;

#[path = "theme.rs"]
mod theme;

/// Component contracts, one row per widget: what mounts, how it announces, and its geometry.
#[test]
fn component_contracts() {
    common::run_cases(&[
        (
            "bottom_tab_bar::every item mounts its icon and label",
            bottom_tab_bar::every_item_mounts_its_icon_and_label,
        ),
        (
            "button::tap callback writes a signal and rebuilds its reader",
            button::tap_callback_writes_a_signal_and_rebuilds_its_reader,
        ),
        (
            "button::cupertino button with text child announces one labelled button node",
            button::cupertino_button_with_text_child_announces_one_labelled_button_node,
        ),
        (
            "button::long press only button announces enabled",
            button::long_press_only_button_announces_enabled,
        ),
        (
            "button::disabled button announces disabled",
            button::disabled_button_announces_disabled,
        ),
        (
            "nav_bar::leading middle and trailing all mount",
            nav_bar::leading_middle_and_trailing_all_mount,
        ),
        (
            "page_scaffold::content is padded below the nav bar plus the top inset",
            page_scaffold::content_is_padded_below_the_nav_bar_plus_the_top_inset,
        ),
    ]);
}

/// Theme and colour resolution through a mounted context, including the live brightness republish.
#[test]
fn theme_and_color_resolution() {
    common::run_cases(&[
        (
            "colors::static color resolves to itself through a real context",
            colors::static_color_resolves_to_itself_through_a_real_context,
        ),
        (
            "theme::explicit theme brightness overrides media query",
            theme::explicit_theme_brightness_overrides_media_query,
        ),
        (
            "cupertino_app::publishes the resolved theme to descendants",
            cupertino_app::publishes_the_resolved_theme_to_descendants,
        ),
        (
            "cupertino_app::a live brightness republish re resolves the theme",
            cupertino_app::a_live_brightness_republish_re_resolves_the_theme,
        ),
    ]);
}

/// Route transition and tab-scaffold state contracts.
#[test]
fn navigation_contracts() {
    common::run_cases(&[
        (
            "route::cupertino page route slides in from off the right edge over 500ms",
            route::cupertino_page_route_slides_in_from_off_the_right_edge_over_500ms,
        ),
        (
            "tab_scaffold::an inactive tabs state survives switching away and back",
            tab_scaffold::an_inactive_tabs_state_survives_switching_away_and_back,
        ),
        (
            "tab_scaffold::tapping a tab item switches the active tab",
            tab_scaffold::tapping_a_tab_item_switches_the_active_tab,
        ),
        (
            "tab_scaffold::out of range controller selection reports error and recovers",
            tab_scaffold::out_of_range_controller_selection_reports_error_and_recovers,
        ),
        (
            "tab_scaffold::standalone bar rejects an out of range selection",
            tab_scaffold::standalone_bar_rejects_an_out_of_range_selection,
        ),
    ]);
}
