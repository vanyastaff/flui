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

#[path = "gesture_alternatives.rs"]
mod gesture_alternatives;

#[path = "icon_button.rs"]
mod icon_button;

#[path = "ink_ripple.rs"]
mod ink_ripple;

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

#[path = "open_container.rs"]
mod open_container;

#[path = "page_transitions.rs"]
mod page_transitions;

#[path = "progress_indicator.rs"]
mod progress_indicator;

#[path = "progress_variants.rs"]
mod progress_variants;

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
            "ink_well::press highlight lasts its delay under reduced motion",
            ink_well::press_highlight_lasts_its_delay_under_reduced_motion,
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
            "chip::a filter chip announces its selection as pressed",
            chip::a_filter_chip_announces_its_selection_as_pressed,
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
            "list_tile::a selectable tile is an option whether or not selected",
            list_tile::a_selectable_tile_is_an_option_whether_or_not_selected,
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

/// `InkWell`'s state layer and ripple on the recorded display list: alpha
/// tokens, wave radius and fade timing, clipping, reduced motion and a
/// disabled configuration.
#[test]
fn ink_feedback_contracts() {
    common::run_cases(&[
        (
            "ink_ripple::a settled hover paints the hover state layer",
            ink_ripple::a_settled_hover_paints_the_hover_state_layer,
        ),
        (
            "ink_ripple::a held press grows a wave between its start and end radius",
            ink_ripple::a_held_press_grows_a_wave_between_its_start_and_end_radius,
        ),
        (
            "ink_ripple::a quick release finishes the fade in before fading out",
            ink_ripple::a_quick_release_finishes_the_fade_in_before_fading_out,
        ),
        (
            "ink_ripple::a cancelled press fades out at once",
            ink_ripple::a_cancelled_press_fades_out_at_once,
        ),
        (
            "ink_ripple::reduced motion paints the state layer without a wave",
            ink_ripple::reduced_motion_paints_the_state_layer_without_a_wave,
        ),
        (
            "ink_ripple::a wave never paints inside the adjacent well",
            ink_ripple::a_wave_never_paints_inside_the_adjacent_well,
        ),
        (
            "ink_ripple::a press without a wave shows the pressed state layer",
            ink_ripple::a_press_without_a_wave_shows_the_pressed_state_layer,
        ),
        (
            "ink_ripple::a contact dragged past the slop cancels its wave",
            ink_ripple::a_contact_dragged_past_the_slop_cancels_its_wave,
        ),
        (
            "ink_ripple::a huge well publishes only finite ink",
            ink_ripple::a_huge_well_publishes_only_finite_ink,
        ),
        (
            "ink_ripple::a long press winning the arena cancels the wave",
            ink_ripple::a_long_press_winning_the_arena_cancels_the_wave,
        ),
        (
            "ink_ripple::a second contact grows no wave beside a pending tap",
            ink_ripple::a_second_contact_grows_no_wave_beside_a_pending_tap,
        ),
        (
            "ink_ripple::releasing another button leaves the primary pressing",
            ink_ripple::releasing_another_button_leaves_the_primary_pressing,
        ),
        (
            "ink_ripple::a primary release under another button ends the press",
            ink_ripple::a_primary_release_under_another_button_ends_the_press,
        ),
        (
            "ink_ripple::a well without a frame clock shows the pressed layer",
            ink_ripple::a_well_without_a_frame_clock_shows_the_pressed_layer,
        ),
        (
            "ink_ripple::a settled well detaches its painter",
            ink_ripple::a_settled_well_detaches_its_painter,
        ),
        (
            "ink_ripple::a disabled ripple configuration paints no feedback",
            ink_ripple::a_disabled_ripple_configuration_paints_no_feedback,
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
            "switch::mounted switch exports its role and toggle state",
            switch::mounted_switch_exports_its_role_and_toggle_state,
        ),
        (
            "switch::tap fires on changed with the flipped value",
            switch::tap_fires_on_changed_with_the_flipped_value,
        ),
        (
            "switch::a toggle is strictly between its ends at half time and mirrored in rtl",
            switch::a_toggle_is_strictly_between_its_ends_at_half_time_and_mirrored_in_rtl,
        ),
        (
            "switch::reversing a toggle mid flight continues from the shown position",
            switch::reversing_a_toggle_mid_flight_continues_from_the_shown_position,
        ),
        (
            "switch::reduced motion shows a toggle on its first frame",
            switch::reduced_motion_shows_a_toggle_on_its_first_frame,
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
            "input_decorator::a standalone decorator keeps its label and hint readable",
            input_decorator::a_standalone_decorator_keeps_its_label_and_hint_readable,
        ),
        (
            "text_field::tapping the decorated area focuses the field and reaches the decorator",
            text_field::tapping_the_decorated_area_focuses_the_field_and_reaches_the_decorator,
        ),
        (
            "text_field::the input purpose reaches the text store",
            text_field::the_input_purpose_reaches_the_text_store,
        ),
        (
            "text_field::the field carries its label hint and error for assistive technology",
            text_field::the_field_carries_its_label_hint_and_error_for_assistive_technology,
        ),
        (
            "text_field::error line sits below the indicator outside the tap target",
            text_field::error_line_sits_below_the_indicator_outside_the_tap_target,
        ),
        (
            "text_field::replacing and unmounting the field withdraws its node subscription",
            text_field::replacing_and_unmounting_the_field_withdraws_its_node_subscription,
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
        ("snack_bar_keeps_its_display_duration_under_reduced_motion", snack_bar::snack_bar_keeps_its_display_duration_under_reduced_motion),
        ("unmounting_a_hovered_snack_bar_releases_its_timer_pause", snack_bar::unmounting_a_hovered_snack_bar_releases_its_timer_pause),
        ("snack_bar_display_timer_pauses_while_hovered", snack_bar::snack_bar_display_timer_pauses_while_hovered),
        ("drawer::drawer_settling_uses_the_captured_fling_profile", drawer::drawer_settling_uses_the_captured_fling_profile),
        ("drawer::open_drawer_settling_uses_the_captured_fling_profile", drawer::open_drawer_settling_uses_the_captured_fling_profile),
        ("drawer::a_drawer_release_keeps_finger_speed", drawer::a_drawer_release_keeps_finger_speed),
        ("drawer::grabbing_a_closing_drawer_preserves_its_panel", drawer::grabbing_a_closing_drawer_preserves_its_panel),
        ("drawer::drawer_slides_without_rebuilding_per_frame", drawer::drawer_slides_without_rebuilding_per_frame),
        ("drawer::incoming_end_drawer_scrim_covers_the_outgoing_start_panel", drawer::incoming_end_drawer_scrim_covers_the_outgoing_start_panel),
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
            "show_dialog::a shown dialog hides the page beneath from assistive technology",
            show_dialog::a_shown_dialog_hides_the_page_beneath_from_assistive_technology,
        ),
        (
            "snack_bar::a snack bar completion cannot write another presentations signal",
            snack_bar::a_snack_bar_completion_cannot_write_another_presentations_signal,
        ),
        (
            "snack_bar::a shown snack bar is announced as a polite status",
            snack_bar::a_shown_snack_bar_is_announced_as_a_polite_status,
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

/// App bars, scaffold, navigation bar, tabs and route transitions: geometry, scroll behaviour, selection dispatch and the motion of pushed routes.
#[test]
fn navigation_and_layout_contracts() {
    common::run_cases(&[
        (
            "open_container::open container morph by scenario",
            open_container::open_container_morph_by_scenario,
        ),
        (
            "page_transitions::page transitions by platform and variant",
            page_transitions::page_transitions_by_platform_and_variant,
        ),
        (
            "page_transitions::a material dialog fades in over 150 ms",
            page_transitions::a_material_dialog_fades_in_over_150_ms,
        ),
        (
            "page_transitions::page transitions theme defaults and equality",
            page_transitions::page_transitions_theme_defaults_and_equality,
        ),
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
            "app_bar::the title announces as a level one heading",
            app_bar::the_title_announces_as_a_level_one_heading,
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
            "navigation_bar::destinations announce as tabs with their selection",
            navigation_bar::destinations_announce_as_tabs_with_their_selection,
        ),
        (
            "navigation_bar::a selection change expands the new indicator and collapses the old",
            navigation_bar::a_selection_change_expands_the_new_indicator_and_collapses_the_old,
        ),
        (
            "navigation_bar::reduced motion shows a selection change on its first frame",
            navigation_bar::reduced_motion_shows_a_selection_change_on_its_first_frame,
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
            "tab_bar_view::a drag moves the position and indicator but not the index",
            tab_bar_view::a_drag_moves_the_position_and_indicator_but_not_the_index,
        ),
        (
            "tab_bar_view::a released drag commits the page it settles on",
            tab_bar_view::a_released_drag_commits_the_page_it_settles_on,
        ),
        (
            "tab_bar_view::a jump during a tab animation keeps the jumped index",
            tab_bar_view::a_jump_during_a_tab_animation_keeps_the_jumped_index,
        ),
        (
            "tab_bar_view::a second view on one controller is refused",
            tab_bar_view::a_second_view_on_one_controller_is_refused,
        ),
        (
            "tabs::a panicking position observer does not hide the index change",
            tabs::a_panicking_position_observer_does_not_hide_the_index_change,
        ),
        (
            "tabs::a nested change beside a panicking observer does not outlive it",
            tabs::a_nested_change_beside_a_panicking_observer_does_not_outlive_it,
        ),
        (
            "tab_bar_view::unmounting the view mid animation keeps the committed index",
            tab_bar_view::unmounting_the_view_mid_animation_keeps_the_committed_index,
        ),
        (
            "tab_bar_view::a page change in a collapsed viewport reaches position observers",
            tab_bar_view::a_page_change_in_a_collapsed_viewport_reaches_position_observers,
        ),
        (
            "tab_bar_view::reduced motion settles an attached view on the first frame",
            tab_bar_view::reduced_motion_settles_an_attached_view_on_the_first_frame,
        ),
        (
            "tab_bar_view::a failed view does not claim the controller",
            tab_bar_view::a_failed_view_does_not_claim_the_controller,
        ),
        (
            "tabs::a listener that removes itself and panics does not skip the rest",
            tabs::a_listener_that_removes_itself_and_panics_does_not_skip_the_rest,
        ),
        (
            "tabs::a refused motion still lands on the committed index",
            tabs::a_refused_motion_still_lands_on_the_committed_index,
        ),
        (
            "tabs::on tap runs beside a panicking position observer",
            tabs::on_tap_runs_beside_a_panicking_position_observer,
        ),
        (
            "tabs::the position follows the surviving bars clock",
            tabs::the_position_follows_the_surviving_bars_clock,
        ),
        (
            "tab_bar_view::unmounting the view mid drag keeps the index",
            tab_bar_view::unmounting_the_view_mid_drag_keeps_the_index,
        ),
        (
            "tab_bar_view::a refused view does not change the holders motion policy",
            tab_bar_view::a_refused_view_does_not_change_the_holders_motion_policy,
        ),
        (
            "tab_bar_view::an updated view that fails releases its claim",
            tab_bar_view::an_updated_view_that_fails_releases_its_claim,
        ),
        (
            "tab_bar_view::a wheel pulse settles onto the committed page",
            tab_bar_view::a_wheel_pulse_settles_onto_the_committed_page,
        ),
        (
            "tab_bar_view::a refused curve still moves an attached pager",
            tab_bar_view::a_refused_curve_still_moves_an_attached_pager,
        ),
        (
            "tabs::a controller left by its last bar settles without a clock",
            tabs::a_controller_left_by_its_last_bar_settles_without_a_clock,
        ),
        (
            "tabs::a tap on the indicator band reaches the tab",
            tabs::a_tap_on_the_indicator_band_reaches_the_tab,
        ),
        (
            "tabs::a tap without a view animates the position and settles",
            tabs::a_tap_without_a_view_animates_the_position_and_settles,
        ),
        (
            "tabs::the elastic indicator stretches mid way between tabs",
            tabs::the_elastic_indicator_stretches_mid_way_between_tabs,
        ),
        (
            "tabs::the indicator mirrors in a right to left row",
            tabs::the_indicator_mirrors_in_a_right_to_left_row,
        ),
        (
            "tabs::reduced motion settles the owned position on the first frame",
            tabs::reduced_motion_settles_the_owned_position_on_the_first_frame,
        ),
        (
            "scaffold::body is positioned below the app bar with no padding",
            scaffold::body_is_positioned_below_the_app_bar_with_no_padding,
        ),
        (
            "scaffold::floating action button floats above the keyboard",
            scaffold::floating_action_button_floats_above_the_keyboard,
        ),
        (
            "scaffold::plain body text takes the theme body style and components keep theirs",
            scaffold::plain_body_text_takes_the_theme_body_style_and_components_keep_theirs,
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
            "theme::retained theme roles paint distinct numeric sizing answers",
            theme::retained_theme_roles_paint_distinct_numeric_sizing_answers,
        ),
        (
            "material_app::contrast selects authored themes in a retained app",
            material_app::contrast_selects_authored_themes_in_a_retained_app,
        ),
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
            "material_app::theme mode switch animates through a blended theme",
            material_app::theme_mode_switch_animates_through_a_blended_theme,
        ),
        (
            "material_app::theme mode switch animates through a blended theme",
            material_app::theme_mode_switch_animates_through_a_blended_theme,
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

/// `LinearProgressIndicator`: indeterminate bars on virtual time and its
/// semantics.
#[test]
fn linear_progress_indeterminate_keyframes() {
    common::run_cases(&[
        (
            "progress_indicator::indeterminate bars follow the published timing",
            progress_indicator::indeterminate_bars_follow_the_published_timing,
        ),
        (
            "progress_indicator::announced as progress",
            progress_indicator::announced_as_progress,
        ),
        (
            "progress_indicator::reduced motion keeps visible loading without continuation",
            progress_indicator::reduced_motion_keeps_visible_loading_without_continuation,
        ),
        (
            "progress_indicator::initially inactive loading stays visible without a percentage",
            progress_indicator::initially_inactive_loading_stays_visible_without_a_percentage,
        ),
        (
            "progress_indicator::nested pause and policy changes retain visible loading",
            progress_indicator::nested_pause_and_policy_changes_retain_visible_loading,
        ),
    ]);
}

/// The progress indicator family: gap and stop, circular and wavy variants, the value
/// policy, reduced motion, and the Material refresh indicator.
#[test]
fn progress_indicator_variants() {
    common::run_cases(&[
        (
            "linear gap and stop",
            progress_variants::linear_gap_and_stop,
        ),
        (
            "circular determinate arcs",
            progress_variants::circular_determinate_arcs,
        ),
        (
            "determinate values stay finite",
            progress_variants::determinate_values_stay_finite,
        ),
        (
            "circular indeterminate is the activity arc",
            progress_variants::circular_indeterminate_is_the_activity_arc,
        ),
        (
            "waves rise, lie flat and travel",
            progress_variants::waves_rise_lie_flat_and_travel,
        ),
        (
            "wave amplitude eases in",
            progress_variants::wave_amplitude_eases_in,
        ),
        (
            "flat waves need no frames",
            progress_variants::flat_waves_need_no_frames,
        ),
        (
            "huge layouts stay bounded",
            progress_variants::huge_layouts_stay_bounded,
        ),
        (
            "material app refresh shows the circular indicator",
            progress_variants::material_app_refresh_shows_the_circular_indicator,
        ),
    ]);
}

/// `LinearProgressIndicator`: the switch to a determinate value.
#[test]
fn linear_progress_switches_to_determinate() {
    common::run_cases(&[
        (
            "progress_indicator::reduced motion determinate switches keep their meaning",
            progress_indicator::reduced_motion_determinate_switches_keep_their_meaning,
        ),
        (
            "progress_indicator::switching to a value stops the bars",
            progress_indicator::switching_to_a_value_stops_the_bars,
        ),
        (
            "progress_indicator::values outside the range are clamped",
            progress_indicator::values_outside_the_range_are_clamped,
        ),
        (
            "progress_indicator::right-to-left progress starts at the right",
            progress_indicator::right_to_left_progress_starts_at_the_right,
        ),
        (
            "progress_indicator::negative zero announces zero percent",
            progress_indicator::negative_zero_announces_zero_percent,
        ),
    ]);
}

/// A `TabBarView`'s alternatives to a swipe (ADR-0216): the input-path matrix
/// rows, whose single-pointer path is the `TabBar` beside it.
#[test]
fn gesture_alternatives() {
    common::run_cases(&[
        (
            "gesture_alternatives::tab_bar_view_with_its_tab_bar",
            gesture_alternatives::tab_bar_view_with_its_tab_bar,
        ),
        (
            "gesture_alternatives::tab_bar_view_without_a_tab_bar",
            gesture_alternatives::tab_bar_view_without_a_tab_bar,
        ),
        (
            "gesture_alternatives::the_end_tabs_have_no_step_beyond_them",
            gesture_alternatives::the_end_tabs_have_no_step_beyond_them,
        ),
        (
            "gesture_alternatives::unmounting_the_tab_bar_unregisters_it",
            gesture_alternatives::unmounting_the_tab_bar_unregisters_it,
        ),
        (
            "gesture_alternatives::replacing_the_controller_moves_the_pager_and_bar_to_the_new_one",
            gesture_alternatives::replacing_the_controller_moves_the_pager_and_bar_to_the_new_one,
        ),
    ]);
}
