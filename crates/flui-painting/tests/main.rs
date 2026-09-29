//! Single-binary consolidation of flui-painting's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `painting_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! paths keep working unchanged.
//!
//! Each contract family is one table test: the modules expose plain row
//! functions and the tests below run them, naming every failing row.
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-painting's integration tests do — the crate's only process-global
//! is the lazily initialized `FONT_SYSTEM` `OnceLock` (benign once-init,
//! never replaced or reset by tests).

#[path = "support/cases.rs"]
mod cases;
#[path = "color_blend.rs"]
mod color_blend;
#[path = "color_property.rs"]
mod color_property;
#[path = "compile_fail.rs"]
mod compile_fail;
#[path = "damage_extent.rs"]
mod damage_extent;
#[path = "decoration_unit.rs"]
mod decoration_unit;
#[path = "recording.rs"]
mod recording;
#[path = "rich_text_example.rs"]
mod rich_text_example;
#[path = "text_layout_pipeline.rs"]
mod text_layout_pipeline;
#[path = "text_layout_unit.rs"]
mod text_layout_unit;
#[path = "text_overflow_unit.rs"]
mod text_overflow_unit;
#[path = "text_painter_unit.rs"]
mod text_painter_unit;

use cases::run_cases;

#[test]
fn color_contract() {
    run_cases(
        "color",
        &[
            (
                "hex_roundtrips_and_porter_duff_modes_mirror",
                color_property::hex_roundtrips_and_porter_duff_modes_mirror,
            ),
            (
                "blend_over_matches_flutter_alpha_blend",
                color_blend::blend_over_matches_flutter_alpha_blend,
            ),
            (
                "lerp_multi_stop_brackets_and_clamps",
                color_property::lerp_multi_stop_brackets_and_clamps,
            ),
        ],
    );
}

#[test]
fn recording_contract() {
    run_cases(
        "recording",
        &[
            (
                "save_restore_tracks_the_save_count",
                recording::save_restore_tracks_the_save_count,
            ),
            #[cfg(debug_assertions)]
            (
                "finish_panics_in_debug_on_unrestored_save",
                recording::finish_panics_in_debug_on_unrestored_save,
            ),
            (
                "isolated_append_preserves_bounds_and_scopes_the_run",
                recording::isolated_append_preserves_bounds_and_scopes_the_run,
            ),
            (
                "interning_shares_arc_for_identical_paints",
                recording::interning_shares_arc_for_identical_paints,
            ),
            (
                "a_finished_display_list_is_sendable_to_another_thread",
                recording::a_finished_display_list_is_sendable_to_another_thread,
            ),
        ],
    );
}

#[test]
fn damage_extent_contract() {
    run_cases(
        "damage_extent",
        &[
            (
                "a_color_fill_makes_the_extent_unbounded",
                damage_extent::a_color_fill_makes_the_extent_unbounded,
            ),
            (
                "paragraph_extent_covers_every_rasterized_glyph",
                damage_extent::paragraph_extent_covers_every_rasterized_glyph,
            ),
            (
                "stroke_and_shadow_extents_cover_their_outsets",
                damage_extent::stroke_and_shadow_extents_cover_their_outsets,
            ),
            (
                "shadow_extent_spreads_by_the_largest_scale_on_both_axes",
                damage_extent::shadow_extent_spreads_by_the_largest_scale_on_both_axes,
            ),
            (
                "fill_style_lines_and_points_reach_their_stroke_width",
                damage_extent::fill_style_lines_and_points_reach_their_stroke_width,
            ),
            (
                "atlas_extent_covers_the_sprite_destination",
                damage_extent::atlas_extent_covers_the_sprite_destination,
            ),
            (
                "transparent_source_and_transparent_black_predicates",
                damage_extent::transparent_source_and_transparent_black_predicates,
            ),
        ],
    );
}

#[test]
fn decoration_contract() {
    run_cases(
        "decoration",
        &[
            (
                "flutter_paint_order_shadow_background_border",
                decoration_unit::flutter_paint_order_shadow_background_border,
            ),
            (
                "hit_test_respects_rounded_corners",
                decoration_unit::hit_test_respects_rounded_corners,
            ),
            (
                "circle_uniform_border_is_a_stroked_circle_not_a_drrect",
                decoration_unit::circle_uniform_border_is_a_stroked_circle_not_a_drrect,
            ),
        ],
    );
}

#[test]
fn text_contract() {
    run_cases(
        "text",
        &[
            (
                "caret_position",
                text_layout_unit::test_text_layout_caret_position,
            ),
            (
                "two_space_run_word_boundary",
                text_layout_unit::get_word_boundary_two_space_run_boundary_matrix,
            ),
            (
                "styled_text_pipeline",
                text_layout_pipeline::full_pipeline_with_styled_text,
            ),
            (
                "wide_ellipsis_floors_min_intrinsic_width",
                text_painter_unit::wide_ellipsis_floors_min_intrinsic_width,
            ),
            (
                "truncated_paragraph_paints_what_it_measured",
                text_overflow_unit::a_truncated_paragraph_paints_exactly_the_lines_it_measured,
            ),
            (
                "root_recolor_keeps_the_shaped_buffer",
                text_overflow_unit::root_recolor_keeps_the_shaped_buffer_and_span_recolor_reshapes_once,
            ),
            (
                "bidirectional_text_lays_out",
                rich_text_example::example_bidirectional_text,
            ),
        ],
    );
}
