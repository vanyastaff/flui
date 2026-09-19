# AGENTS.md — flui-layer

Compositor layer tree — the fourth tree in FLUI's 5-tree architecture (View → Element → Render → Layer → Semantics).

## What lives here

- `Layer` — the closed enum of compositor primitives (19 variants: leaves, clips, transforms, effects, links, annotation); `Layer::local_translation()` is the one place the set of translating variants is written down
- `LayerTree` / `LayerNode` — an append-only arena built pre-order through `insert_root` / `push_child`; the tree indexes every `LeaderLayer` by `LayerLink`. A node is written once, at insertion: there is no removal, re-parenting, node-level link setter, or `&mut` reach into a node, so a cycle or a doubly-parented node is unrepresentable and the leader index cannot go stale
- `LayerLink` + `resolve_follower_offset` — leader/follower chain math over `local_translation`
- `SceneBuilder` — push/pop construction for hand-authored scenes (`run_direct`, examples, GPU readback tests); the production frame is built by `flui-rendering`'s fragment composer straight through the tree
- `Scene` — a frozen `LayerTree` (no `&mut` path); `SceneSnapshot` / `DamageRegion` — the stamped per-frame package that crosses the raster boundary
- `testing::inspect` (feature `testing`) — read-only structural walkers, re-exported by `flui-rendering`'s harness

## Key constraints

- Adding a `Layer` variant breaks `flui-engine`'s exhaustive match on purpose — land the GPU lowering in the same change, and add the variant to `local_translation` if it translates its children
- Every translating variant's engine impl must push exactly `local_translation()`; `flui-engine`'s `every_variant_pushes_exactly_its_local_translation` pins it
- `Layer`, `LayerTree`, `Scene`, `SceneSnapshot` are `Send + Sync` plain data, pinned by `static_assertions` in `scene_snapshot.rs`; the by-value raster contract is enforced by the mailbox API, not the auto traits
- No `RwLock<Box<dyn Layer>>` (port-check trigger #1); no `async fn` on `composite`/`render` (trigger #3)
- `flui-hot-reload` moves `Scene` across a dlopen boundary and hashes `size_of::<Scene>` / `size_of::<LayerTree>` into its ABI token — a field change there is fine (both halves rebuild), a move to another crate is not (hot-reload does not depend on `flui-engine`)

## Not here on purpose

- `DamageTracker` (renderer runtime state) lives in `flui-engine/src/damage.rs`; only the seam message `DamageRegion` is defined here
- `PerformanceStats` (a clock-bearing frame-time window) lives in `flui-app`; `PerformanceOverlayLayer` only carries the numbers it is handed

## Architecture doc

See `crates/flui-layer/ARCHITECTURE.md` for the mapping decisions and the reference accounting.
