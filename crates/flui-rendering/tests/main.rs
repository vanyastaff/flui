//! Single-binary consolidation of the flui-rendering integration tests.
//!
//! Each root test file is compiled as a module of one `rendering_it` test
//! binary instead of 36 separate binaries, cutting link time and disk usage.
//! Files stay in place so data paths (e.g. `tests/snapshots/`) keep working.

#[path = "animation_pipeline.rs"]
mod animation_pipeline;
#[path = "attach_detach_lifecycle.rs"]
mod attach_detach_lifecycle;
#[path = "compositing_bits_walk.rs"]
mod compositing_bits_walk;
#[path = "cross_protocol_layout.rs"]
mod cross_protocol_layout;
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
#[path = "layout_offset_commit.rs"]
mod layout_offset_commit;
#[path = "layout_raw_bridge.rs"]
mod layout_raw_bridge;
#[path = "paint_dirty_flag_discipline.rs"]
mod paint_dirty_flag_discipline;
#[path = "paint_fragment_snapshot.rs"]
mod paint_fragment_snapshot;
#[path = "pipeline_scenarios.rs"]
mod pipeline_scenarios;
#[path = "render_viewport.rs"]
mod render_viewport;
#[path = "repaint_handle.rs"]
mod repaint_handle;
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
#[path = "u21b_cyclic_intrinsic_query.rs"]
mod u21b_cyclic_intrinsic_query;
#[path = "u3c_lazy_sliver_contract.rs"]
mod u3c_lazy_sliver_contract;
#[path = "zz_scratch_m2_repro.rs"]
mod zz_scratch_m2_repro;
