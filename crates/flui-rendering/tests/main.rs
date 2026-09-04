//! Single-binary consolidation of the flui-rendering integration tests.
//!
//! Each root test file is compiled as a module of one `rendering_it` test
//! binary instead of one binary per file, cutting link time and disk usage.
//! Files stay in place so relative data paths keep working. Scaffolding
//! shared across the modules lives in [`common`].

mod common;

#[path = "animation_pipeline.rs"]
mod animation_pipeline;
#[path = "attach_detach_lifecycle.rs"]
mod attach_detach_lifecycle;
#[path = "compositing_bits_walk.rs"]
mod compositing_bits_walk;
#[path = "cross_protocol_layout.rs"]
mod cross_protocol_layout;
mod cyclic_intrinsic_query;
#[path = "decorated_box_pipeline.rs"]
mod decorated_box_pipeline;
#[path = "deep_tree_stack.rs"]
mod deep_tree_stack;
#[path = "dirty_queue_dedup.rs"]
mod dirty_queue_dedup;
#[path = "dispose_eviction.rs"]
mod dispose_eviction;
#[path = "dpr_pipeline.rs"]
mod dpr_pipeline;
#[path = "flex_layout_fixes.rs"]
mod flex_layout_fixes;
#[path = "harness_animation.rs"]
mod harness_animation;
#[path = "harness_self_test.rs"]
mod harness_self_test;
#[path = "hit_test_pipeline.rs"]
mod hit_test_pipeline;
#[path = "intrinsics_cache.rs"]
mod intrinsics_cache;
#[path = "layout_cycle_guard.rs"]
mod layout_cycle_guard;
#[path = "layout_dirty_root.rs"]
mod layout_dirty_root;
#[path = "layout_marks_laid_out_boundaries.rs"]
mod layout_marks_laid_out_boundaries;
#[path = "layout_offset_commit.rs"]
mod layout_offset_commit;

#[path = "layout_poison.rs"]
mod layout_poison;
#[path = "layout_raw_bridge.rs"]
mod layout_raw_bridge;
#[path = "placed_generation_gate.rs"]
mod placed_generation_gate;

#[path = "retained_boundary_layers.rs"]
mod retained_boundary_layers;

#[path = "paint_dirty_flag_discipline.rs"]
mod paint_dirty_flag_discipline;
#[path = "paint_fragment_snapshot.rs"]
mod paint_fragment_snapshot;
#[path = "pipeline_scenarios.rs"]
mod pipeline_scenarios;
#[path = "render_invalidation_handle.rs"]
mod render_invalidation_handle;
#[path = "render_viewport.rs"]
mod render_viewport;
#[path = "root_resize_repaint.rs"]
mod root_resize_repaint;
#[path = "run_layout_wiring.rs"]
mod run_layout_wiring;
#[path = "semantics_assembly.rs"]
mod semantics_assembly;
#[path = "sliver_direction_matrix.rs"]
mod sliver_direction_matrix;
#[path = "sliver_fill_remaining.rs"]
mod sliver_fill_remaining;
#[path = "sliver_fill_viewport.rs"]
mod sliver_fill_viewport;
#[path = "sliver_fixed_extent_list.rs"]
mod sliver_fixed_extent_list;
#[path = "sliver_geometry_validation.rs"]
mod sliver_geometry_validation;
#[path = "sliver_grid.rs"]
mod sliver_grid;
#[path = "sliver_hit_direction_matrix.rs"]
mod sliver_hit_direction_matrix;
#[path = "sliver_to_box_adapter.rs"]
mod sliver_to_box_adapter;
#[path = "structural_invalidation.rs"]
mod structural_invalidation;
#[path = "transform_to.rs"]
mod transform_to;
