# Tasks: Refactor flui-tree to Pure Tree Abstractions

## Phase 1: Prepare Target Locations

- [ ] 1.1 Create `flui_rendering/src/tree/mod.rs` module
- [ ] 1.2 Ensure `flui_core/src/element/` can receive element traits
- [ ] 1.3 Ensure `flui-view/src/tree/` exists
- [ ] 1.4 Create `flui_devtools` crate if needed (or use existing debug module)

## Phase 2: Move Render-Specific Code to flui_rendering

- [ ] 2.1 Move `flui-tree/src/traits/render.rs` → `flui_rendering/src/tree/access.rs`
- [ ] 2.2 Move `flui-tree/src/traits/dirty.rs` → `flui_rendering/src/tree/dirty.rs`
- [ ] 2.3 Move `flui-tree/src/iter/render.rs` → `flui_rendering/src/tree/iter.rs`
- [ ] 2.4 Move `flui-tree/src/iter/render_collector.rs` → `flui_rendering/src/tree/collector.rs`
- [ ] 2.5 Create `flui_rendering/src/tree/mod.rs` with re-exports
- [ ] 2.6 Update imports in flui_rendering
- [ ] 2.7 `cargo build -p flui_rendering`

## Phase 3: Move Element-Specific Code to flui_core

- [ ] 3.1 Move `traits/lifecycle.rs` → `flui_core/src/element/lifecycle.rs`
- [ ] 3.2 Move `traits/reconciliation.rs` → `flui_core/src/element/reconciliation.rs`
- [ ] 3.3 Move `traits/inherited.rs` → `flui_core/src/element/inherited.rs`
- [ ] 3.4 Move `traits/diff.rs` → `flui_core/src/element/diff.rs`
- [ ] 3.5 Update `flui_core/src/element/mod.rs` exports
- [ ] 3.6 `cargo build -p flui_core`

## Phase 4: Move View-Specific Code to flui-view

- [ ] 4.1 Move `traits/view.rs` → `flui-view/src/tree/snapshot.rs`
- [ ] 4.2 Update flui-view imports
- [ ] 4.3 `cargo build -p flui-view`

## Phase 5: Move Pipeline & Debug Code

- [ ] 5.1 Move `traits/pipeline.rs` → `flui-pipeline/src/traits.rs`
- [ ] 5.2 Move `traits/context.rs` → `flui_core/src/context.rs`
- [ ] 5.3 Move `traits/validation.rs` → debug/devtools location
- [ ] 5.4 Move `visitor/statistics.rs` → debug/devtools location
- [ ] 5.5 Delete `traits/combined.rs`
- [ ] 5.6 Update affected crate imports

## Phase 6: Simplify flui-tree to Pure Abstractions

- [ ] 6.1 Remove all moved files
- [ ] 6.2 Simplify `traits/read.rs` (remove domain-specific code)
- [ ] 6.3 Simplify `traits/nav.rs` (remove domain-specific code)
- [ ] 6.4 Simplify `traits/write.rs` (remove domain-specific code)
- [ ] 6.5 Simplify `visitor/mod.rs` (keep generic visitors only)
- [ ] 6.6 Keep `arity/` as-is (generic child count validation)
- [ ] 6.7 Keep generic iterators (`ancestors`, `descendants`, `siblings`, `dfs`, `bfs`)
- [ ] 6.8 Update `lib.rs` - clean exports
- [ ] 6.9 Update `README.md`
- [ ] 6.10 `cargo build -p flui-tree`

## Phase 7: Update All Dependent Crates

- [ ] 7.1 Update `flui_widgets` imports
- [ ] 7.2 Update `flui_app` imports
- [ ] 7.3 Update any examples
- [ ] 7.4 Fix any remaining import errors

## Phase 8: Final Verification

- [ ] 8.1 `cargo build --workspace`
- [ ] 8.2 `cargo test --workspace`
- [ ] 8.3 `cargo clippy --workspace -- -D warnings`
- [ ] 8.4 Verify line count (~3,000 lines in flui-tree)
- [ ] 8.5 Update CLAUDE.md documentation

## Notes

- Phases 2, 3, 4, 5 can be parallelized after Phase 1
- Each phase should end with a working build
- Keep git commits atomic per-phase for easy rollback
