QUESTION: Does the remaining rendering-G scene/cache/resource-reuse pass expose a new nonduplicate issue beyond the existing damage and layer-walk findings?

ANSWER: No new nonduplicate issue was established in this bounded pass. Full-surface raster damage is already covered by #1037. Retained repaint-boundary CPU reuse, path-cache keying/eviction, texture-cache maintenance, and pipeline blend-key tests have targeted green coverage; the remaining risk is broader device/profile/E2E coverage, not a new confirmed defect from this pass.

VERSIONS: flui current worktree on 2026-09-13; source read from `/mnt/data/dev/flui`.

SOURCES:
- crates/flui-layer/src/scene_snapshot.rs:13 documents that `DamageRegion::Full` is the only `SceneSnapshot` damage variant today.
- crates/flui-app/src/app/raster_lane.rs:311 constructs `SceneSnapshot::new(..., DamageRegion::Full, scene)`.
- crates/flui-engine/src/raster_owner.rs:1408 documents that `DamageRegion::Full` is the only variant and unconditionally calls `backend.mark_full_repaint()`.
- crates/flui-engine/src/wgpu/renderer.rs:1531 documents the renderer's damage consumer and explicitly says no production caller uses `mark_dirty` today.
- https://github.com/vanyastaff/flui/issues/1037 already tracks the missing stable-layer-identity partial-damage producer.
- crates/flui-rendering/tests/retained_boundary_layers.rs:1 documents retained boundary reuse and its full-repaint oracle.
- `cargo nextest run -p flui-rendering --test rendering_it -E 'test(retained_boundary_layers::a_retained_frame_matches_what_a_full_repaint_produces) | test(retained_boundary_layers::clean_repaint_boundary_is_not_repainted_when_sibling_is_dirty) | test(retained_boundary_layers::a_layer_update_is_written_back_into_the_retained_capture)' --no-fail-fast` ran 2 tests / 2 passed / 342 skipped. The middle filter did not match a test name.
- crates/flui-engine/src/wgpu/resources.rs:1 documents `GpuResources` as the single owner of buffer pool, texture cache, layer texture pool, external texture registry, and uniform pool.
- crates/flui-engine/src/wgpu/texture_cache.rs:767 documents end-frame maintenance ordering: atlas reset and budget eviction before use-counter reset.
- crates/flui-engine/src/wgpu/path_cache.rs:1 and crates/flui-engine/src/wgpu/batches/paths.rs:217 document path-cache keying by geometry, tessellation parameters, and quantized scale, with dashed strokes bypassing the cache.
- `cargo nextest run -p flui-engine --lib -E 'test(wgpu::path_cache::tests::)' --no-fail-fast` ran 8 tests / 8 passed / 350 skipped.
- `cargo nextest run -p flui-engine --lib -E 'test(wgpu::texture_cache::unit_tests::) | test(wgpu::pipeline::blend_logic::) | test(wgpu::pipelines::)' --no-fail-fast` ran 30 tests / 30 passed / 328 skipped.

OPEN: The selected tests are bounded. They do not prove full GPU resource lifecycle correctness, device-loss behavior for every cache, WebGL/WebGPU profile compatibility, or full-scene partial damage. #1037 remains the architectural owner for partial damage.

ANSWERED
