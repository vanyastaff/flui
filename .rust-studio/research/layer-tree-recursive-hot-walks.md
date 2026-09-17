QUESTION: Do FLUI's production layer-tree operations and render traversal tolerate deep composited trees without consuming one Rust stack frame per layer?

ANSWER: No. Diagnostic inspection uses an explicit stack and has a 10,000-deep small-stack regression test, but production layer-tree cloning, dirty-bit folding, dirty-bit clearing, the windowed renderer layer walk, and the headless renderer layer walk are recursive. A scratch probe shows `clone_subtree`, `update_subtree_needs_add_to_scene`, and `clear_needs_add_to_scene_subtree` each abort with stack overflow on a 10,000-node chain on a 64 KiB thread stack.

VERSIONS: flui workspace at HEAD b482a20824ba80c961a5c5074e2df6a8880f8b1e as inspected on 2026-09-13.

SOURCES:
- `crates/flui-layer/src/tree/layer_tree.rs:576-600` implements `clone_subtree` via recursive `clone_subtree_into`.
- `crates/flui-layer/src/tree/layer_tree.rs:960-987` implements recursive `update_subtree_needs_add_to_scene` and `clear_needs_add_to_scene_subtree`.
- `crates/flui-layer/src/testing/inspect.rs:47-72` documents and implements explicit-stack diagnostic traversal because deep composited trees are ordinary.
- `crates/flui-layer/src/testing/inspect.rs:280-307` tests a 10,000-deep chain on a 64 KiB stack for the diagnostic walker.
- `crates/flui-engine/src/wgpu/renderer.rs:1897-1906` calls `render_layer_recursive`; `renderer.rs:2011-2114`, `2151-2160`, and `2183-2190` recurse into children in the production renderer.
- `crates/flui-engine/src/wgpu/headless.rs:130-131` calls recursive `walk_layer_tree`; `headless.rs:261-274` recurses into children for screenshot/offscreen rendering.
- Scratch probe: `/mnt/data/audits/rendering-g-20260913/layer-recursion-probe`.
- Commands:
  - `cargo run --quiet --manifest-path /mnt/data/audits/rendering-g-20260913/layer-recursion-probe/Cargo.toml -- clear 10000` -> exit 134, stack overflow.
  - `cargo run --quiet --manifest-path /mnt/data/audits/rendering-g-20260913/layer-recursion-probe/Cargo.toml -- update 10000` -> exit 134, stack overflow.
  - `cargo run --quiet --manifest-path /mnt/data/audits/rendering-g-20260913/layer-recursion-probe/Cargo.toml -- clone 10000` -> exit 134, stack overflow.

OPEN: I did not execute the windowed `Renderer::render_layer_recursive` crash path because it needs a GPU render target and private method access, but the same source-recursive shape is present on the production render path and on the public headless render path.

UNRESOLVED: none for the layer-tree APIs; render traversal execution should be added as an acceptance test when the traversal is made iterative.
