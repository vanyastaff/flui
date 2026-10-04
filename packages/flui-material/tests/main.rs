//! flui-material's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).
//!
//! `common` is declared once here and every suite imports it with
//! `use crate::common;` -- a per-suite `mod common;` would load
//! `tests/common/mod.rs` once per suite (`clippy::duplicate_mod`).

mod common;

#[path = "app_bar.rs"]
mod app_bar;

#[path = "card.rs"]
mod card;

#[path = "checkbox.rs"]
mod checkbox;

#[path = "chip.rs"]
mod chip;

#[path = "data_table.rs"]
mod data_table;

#[path = "dialog.rs"]
mod dialog;

#[path = "divider.rs"]
mod divider;

#[path = "drawer.rs"]
mod drawer;

#[path = "elevated_button.rs"]
mod elevated_button;

#[path = "flexible_space_bar.rs"]
mod flexible_space_bar;

#[path = "floating_action_button.rs"]
mod floating_action_button;

#[path = "icon_button.rs"]
mod icon_button;

#[path = "ink_well.rs"]
mod ink_well;

#[path = "input_decorator.rs"]
mod input_decorator;

#[path = "list_tile.rs"]
mod list_tile;

#[path = "material.rs"]
mod material;

#[path = "material_app.rs"]
mod material_app;

#[path = "navigation_bar.rs"]
mod navigation_bar;

#[path = "radio.rs"]
mod radio;

#[path = "rebuild_exactness.rs"]
mod rebuild_exactness;

#[path = "scaffold.rs"]
mod scaffold;

#[path = "show_dialog.rs"]
mod show_dialog;

#[path = "sliver_app_bar.rs"]
mod sliver_app_bar;

#[path = "snack_bar.rs"]
mod snack_bar;

#[path = "switch.rs"]
mod switch;

#[path = "tab_bar_view.rs"]
mod tab_bar_view;

#[path = "tabs.rs"]
mod tabs;

#[path = "text_field.rs"]
mod text_field;

#[path = "text_form_field.rs"]
mod text_form_field;
#[path = "theme.rs"]
mod theme;

#[path = "theme_fields.rs"]
mod theme_fields;

/// Buttons, ink and tappable surfaces, one row per behaviour: dispatch, semantics, and theme-slot precedence on the mounted tree.
#[test]
fn action_component_contracts() {
    common::run_cases(&[
        (
            "elevated_button::elevated button with text child announces one labelled button node",
            elevated_button::elevated_button_with_text_child_announces_one_labelled_button_node,
        ),
        (
            "elevated_button::widget level style wins over the elevated button theme",
            elevated_button::widget_level_style_wins_over_the_elevated_button_theme,
        ),
        (
            "icon_button::icon button theme slot reaches the icons icon theme",
            icon_button::icon_button_theme_slot_reaches_the_icons_icon_theme,
        ),
        (
            "ink_well::disabled ink well does not fire a tap callback",
            ink_well::disabled_ink_well_does_not_fire_a_tap_callback,
        ),
        (
            "ink_well::pointer and keyboard activation write the owning signal",
            ink_well::pointer_and_keyboard_activation_write_the_owning_signal,
        ),
        (
            "ink_well::enter on a focused elevated button writes a signal and rebuilds its reader",
            ink_well::enter_on_a_focused_elevated_button_writes_a_signal_and_rebuilds_its_reader,
        ),
        (
            "floating_action_button::fab theme slot reaches the mounted materials color and elevation",
            floating_action_button::fab_theme_slot_reaches_the_mounted_materials_color_and_elevation,
        ),
        (
            "floating_action_button::mounted geometry in a scaffold slot is exactly 56 by 56 at the end float position",
            floating_action_button::mounted_geometry_in_a_scaffold_slot_is_exactly_56_by_56_at_the_end_float_position,
        ),
        (
            "chip::disabled chip and its delete icon are both inert through dispatch",
            chip::disabled_chip_and_its_delete_icon_are_both_inert_through_dispatch,
        ),
        (
            "chip::tapping the delete icon fires on deleted only not the chip tap",
            chip::tapping_the_delete_icon_fires_on_deleted_only_not_the_chip_tap,
        ),
        (
            "list_tile::merge semantics over a tile and radio announces as one radio button",
            list_tile::merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button,
        ),
        (
            "list_tile::whole tile tap fires from a point inside the content padding",
            list_tile::whole_tile_tap_fires_from_a_point_inside_the_content_padding,
        ),
    ]);
}

/// Checkbox, radio, switch and the data table's selection column: tap dispatch, tri-state semantics and disabled handling.
#[test]
fn selection_control_contracts() {
    common::run_cases(&[
        (
            "checkbox::indeterminate tristate exports mixed semantics",
            checkbox::indeterminate_tristate_exports_mixed_semantics,
        ),
        (
            "checkbox::tristate cycle survives a rebuild between each tap",
            checkbox::tristate_cycle_survives_a_rebuild_between_each_tap,
        ),
        (
            "radio::a mounted radio announces as a radio button",
            radio::a_mounted_radio_announces_as_a_radio_button,
        ),
        (
            "radio::tap on an unselected radio fires on changed with its own value",
            radio::tap_on_an_unselected_radio_fires_on_changed_with_its_own_value,
        ),
        (
            "switch::disabled switch swallows a tap then resyncs once a handler is added",
            switch::disabled_switch_swallows_a_tap_then_resyncs_once_a_handler_is_added,
        ),
        (
            "switch::tap fires on changed with the flipped value",
            switch::tap_fires_on_changed_with_the_flipped_value,
        ),
        (
            "data_table::heading checkbox tap selects all from the indeterminate state",
            data_table::heading_checkbox_tap_selects_all_from_the_indeterminate_state,
        ),
        (
            "data_table::widget override beats theme beats default on a mounted tree",
            data_table::widget_override_beats_theme_beats_default_on_a_mounted_tree,
        ),
        (
            "data_table::themed checkbox margin matches the same widget margin",
            data_table::themed_checkbox_margin_matches_the_same_widget_margin,
        ),
        (
            "data_table::checkbox margin override beats theme and retains default spacing",
            data_table::checkbox_margin_override_beats_the_theme_without_changing_default_spacing,
        ),
    ]);
}

/// Text input stack: decorator, field focus and form validation reach the mounted tree.
#[test]
fn text_input_contracts() {
    common::run_cases(&[
        (
            "input_decorator::error replaces helper at the mounted level",
            input_decorator::error_replaces_helper_at_the_mounted_level,
        ),
        (
            "text_field::tapping the decorated area focuses the field and reaches the decorator",
            text_field::tapping_the_decorated_area_focuses_the_field_and_reaches_the_decorator,
        ),
        (
            "text_form_field::validator error reaches the input decorator error line",
            text_form_field::validator_error_reaches_the_input_decorator_error_line,
        ),
    ]);
}

/// Dialogs, snack bars and the drawer: what mounts, what dismisses, and single-fire actions.
#[test]
fn overlay_contracts() {
    common::run_cases(&[
        (
            "snack_bar::a completion panic still advances the accepted snack bar queue",
            snack_bar::a_completion_panic_still_advances_the_accepted_snack_bar_queue,
        ),
        ("drawer::narrow_start_drawer_cancel_uses_its_actual_panel_extent", drawer::narrow_start_drawer_cancel_uses_its_actual_panel_extent),
        ("drawer::narrow_end_drawer_cancel_uses_its_actual_panel_extent", drawer::narrow_end_drawer_cancel_uses_its_actual_panel_extent),
        ("drawer::smaller_configured_drawer_keeps_its_declared_panel_extent", drawer::smaller_configured_drawer_keeps_its_declared_panel_extent),
        ("drawer::ordinary_drawer_keeps_its_configured_extent_in_a_wider_viewport", drawer::ordinary_drawer_keeps_its_configured_extent_in_a_wider_viewport),
        ("drawer::retained_drawer_recomputes_its_extent_after_a_collapsed_resize", drawer::retained_drawer_recomputes_its_extent_after_a_collapsed_resize),
        (
            "drawer::cancelled fast edge drag settles closed below halfway",
            drawer::cancelled_fast_edge_drag_settles_closed_below_halfway,
        ),
        (
            "drawer::cancelled fast panel drag settles open above halfway",
            drawer::cancelled_fast_panel_drag_settles_open_above_halfway,
        ),
        (
            "dialog::a tap on an action fires its handler",
            dialog::a_tap_on_an_action_fires_its_handler,
        ),
        (
            "show_dialog::dialog covers the page and a barrier tap dismisses it leaving page state intact",
            show_dialog::dialog_covers_the_page_and_a_barrier_tap_dismisses_it_leaving_page_state_intact,
        ),
        (
            "snack_bar::a snack bar completion cannot write another presentations signal",
            snack_bar::a_snack_bar_completion_cannot_write_another_presentations_signal,
        ),
        (
            "snack_bar::action press closes the snack bar and is single fire",
            snack_bar::action_press_closes_the_snack_bar_and_is_single_fire,
        ),
        (
            "action_callback_panic_disables_the_button_and_fresh_action_progresses",
            snack_bar::action_callback_panic_disables_the_button_and_fresh_action_progresses,
        ),
        (
            "drawer::a fast release below halfway flings the drawer open rather than snapping shut",
            drawer::a_fast_release_below_halfway_flings_the_drawer_open_rather_than_snapping_shut,
        ),
        (
            "drawer::scrim mounts when open and a tap closes the drawer",
            drawer::scrim_mounts_when_open_and_a_tap_closes_the_drawer,
        ),
    ]);
}

/// App bars, scaffold, navigation bar and tabs: geometry, scroll behaviour and selection dispatch.
#[test]
fn navigation_and_layout_contracts() {
    common::run_cases(&[
        (
            "tabs::small tab height overrides determine the mounted bar height",
            tabs::small_tab_height_overrides_determine_the_mounted_bar_height,
        ),
        (
            "tabs::mixed and empty tab bars keep their content height rules",
            tabs::mixed_and_empty_tab_bars_keep_their_content_height_rules,
        ),
        (
            "app_bar::tapping the implied back button pops the route",
            app_bar::tapping_the_implied_back_button_pops_the_route,
        ),
        (
            "sliver_app_bar::a pinned bar holds its collapsed height at deep scroll",
            sliver_app_bar::a_pinned_bar_holds_its_collapsed_height_at_deep_scroll,
        ),
        (
            "flexible_space_bar::collapsed background fades out and parallaxes up",
            flexible_space_bar::collapsed_background_fades_out_and_parallaxes_up,
        ),
        (
            "navigation_bar::tap fires on destination selected with the tapped index",
            navigation_bar::tap_fires_on_destination_selected_with_the_tapped_index,
        ),
        (
            "navigation_bar::tapping a disabled destination does not fire the callback",
            navigation_bar::tapping_a_disabled_destination_does_not_fire_the_callback,
        ),
        (
            "navigation_bar::an empty destination list is rejected",
            navigation_bar::an_empty_destination_list_is_rejected,
        ),
        (
            "navigation_bar::a single destination is rejected",
            navigation_bar::a_single_destination_is_rejected,
        ),
        (
            "navigation_bar::an index at the destination count is rejected",
            navigation_bar::an_index_at_the_destination_count_is_rejected,
        ),
        (
            "navigation_bar::an unrepresentable destination index is rejected",
            navigation_bar::an_unrepresentable_destination_index_is_rejected,
        ),
        (
            "tabs::default tab controller survives a length shrink past the selected index",
            tabs::default_tab_controller_survives_a_length_shrink_past_the_selected_index,
        ),
        (
            "tabs::tap sets the controller index through real pointer dispatch",
            tabs::tap_sets_the_controller_index_through_real_pointer_dispatch,
        ),
        (
            "tab_bar_view::an inactive tabs state survives switching away and back",
            tab_bar_view::an_inactive_tabs_state_survives_switching_away_and_back,
        ),
        (
            "scaffold::body is positioned below the app bar with no padding",
            scaffold::body_is_positioned_below_the_app_bar_with_no_padding,
        ),
        (
            "scaffold::floating action button floats above the keyboard",
            scaffold::floating_action_button_floats_above_the_keyboard,
        ),
    ]);
}

/// Static surfaces (material shape, card, divider): shape hit-testing and theme-slot precedence.
#[test]
fn surface_contracts() {
    common::run_cases(&[
        (
            "material::stadium shape excludes a corner a sharp rectangle would include",
            material::stadium_shape_excludes_a_corner_a_sharp_rectangle_would_include,
        ),
        (
            "card::card theme slot reaches the mounted materials color and elevation",
            card::card_theme_slot_reaches_the_mounted_materials_color_and_elevation,
        ),
        (
            "divider::widget color override wins over the divider theme",
            divider::widget_color_override_wins_over_the_divider_theme,
        ),
    ]);
}

/// Theme access and resolution, per-presentation themes and exact rebuild sets.
#[test]
fn theme_and_app_contracts() {
    common::run_cases(&[
        (
            "theme::theme of panicking accessor returns ancestor theme data",
            theme::theme_of_panicking_accessor_returns_ancestor_theme_data,
        ),
        (
            "theme_fields::changing one theme slot rebuilds that slots readers and whole theme readers only",
            theme_fields::changing_one_theme_slot_rebuilds_that_slots_readers_and_whole_theme_readers_only,
        ),
        (
            "material_app::theme mode switch on a live app updates descendants",
            material_app::theme_mode_switch_on_a_live_app_updates_descendants,
        ),
        (
            "material_app::two presentations resolve different themes simultaneously",
            material_app::two_presentations_resolve_different_themes_simultaneously,
        ),
        (
            "rebuild_exactness::swapping theme data rebuilds exactly the dependents",
            rebuild_exactness::swapping_theme_data_rebuilds_exactly_the_dependents,
        ),
    ]);
}
