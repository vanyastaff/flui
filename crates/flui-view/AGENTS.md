# AGENTS.md — flui-view

View and Element tree: immutable Views → mutable Elements → RenderObjects. The declarative UI layer.

## What lives here

- **View traits** — `StatelessView`, `StatefulView`, `InheritedView`, `RenderView`, `ProxyView`, `ParentDataView`
- **Element tree** — mutable lifecycle nodes that manage Views: mount, build, update, unmount
- **ElementCore** — central element machinery (`element/core.rs`)
- **BuildContext** — context passed to `build()` methods
- **Widget identity** — `ObjectKey`, `GlobalKey` (realm-owned registry activated by an owner-thread scope)
- **Proc-macro derives** — `#[derive(StatelessView)]`, `#[derive(StatefulView)]` via `flui-macros` (re-exported in `prelude`)
- **Binding** — `WidgetsBinding` trait for build-phase coordination

## Key constraints

- **`test-utils` feature** — enables `MockBuildContext` + `ReconcileEventCollector` tracing Layer fixture. Downstream test crates opt in.
- **GlobalKey activation is scoped** — production lookups resolve only inside `UiRealm::enter`; the TLS stack supports nested entry and restores on unwind. Legacy integration fixtures using the manual test adapter remain `#[serial]` until that adapter is retired.
- **A GlobalKey is identified by the key, never by its hash** ([ADR-0050](../../docs/adr/ADR-0050-global-key-identity-and-frame-reservations.md)) — every table that answers a key question (`GlobalKeyRegistry`, `GlobalKeyScope`, `GlobalKeyReservations`) buckets on `ViewKey::key_hash` and decides with `ViewKey::key_eq`. Do not reintroduce a `HashMap<u64, _>` keyed on a key hash: the retake path reads those tables, so a collision is a state transplant, not a lookup miss.
- **A duplicate GlobalKey is reported at the frame boundary, as data** ([ADR-0050](../../docs/adr/ADR-0050-global-key-identity-and-frame-reservations.md)) — two per-frame ledgers feed it: every parent's keyed-child *declaration* (mount, graft, and in-place update alike), and every parent a graft *robbed* of a keyed child. A parent rebuilding clears both of its own entries, which is how it consents to a loss. `BuildOwner::finalize_tree` verifies them after the inactive sweep, repairs the losing parent's dangling child edge, and appends a typed `DuplicateGlobalKey` to `take_global_key_diagnostics`. Both ledgers clear each frame, so a cross-frame reparent is not a duplicate. Do not turn this into a panic — the input is caller-controlled — and do not skip the one-parent-two-children shape: the eager check that owns it in debug is compiled out in release.
- **Focus ownership is explicit** — every `BuildOwner` owns one concrete `Rc<FocusManager>`; presentation composition passes the same manager into `WidgetsBinding::with_focus_manager`. `BuildContext::focus_manager()` is acquired only from `init_state` / `did_change_dependencies` and is guarded by port-check trigger #22.
- **`trybuild` compile-fail tests** — `tests/ui/` corpus exercises derive macro error messages (e.g., `column_17_compile_error.rs`).
- **No `Box<dyn View>` as struct fields** in element child collections — enforced by port-check trigger #6.
- **No `downcast_ref::<V>()` in update-dispatch path** — enforced by FR-033. `dispatch_view_update` (TypeId-keyed `Box::downcast::<V>`) is the only path.
- **Benchmarks** — `key_storage_shape`, `static_path_algorithm`, `global_key_reparent_latency`.
- **`cargo-shear` false positive** — `tests/ui/*.rs` declared in `[package.metadata.cargo-shear] ignored-paths`.
- **A user lifecycle-hook panic is contained per element, not per frame** (issue #561) — a `catch_unwind` at each hook's seam substitutes an `ErrorView` (build-side) or simply lets the rest of teardown continue (removal-side) instead of unwinding out of `BuildOwner::build_scope` / `finalize_tree` and killing the whole frame. Bounded today: `build` (`build_or_recover`, `element/behavior_commons.rs`), `dispose` (`StatefulBehavior::on_unmount`, gated on `self.initialized` so a state whose `init_state` never completed is never disposed), and `deactivate` (`StatefulBehavior::on_deactivate` — the catch sits in the behavior, not around `ElementBase::deactivate` at the tree level, so `ElementCore::deactivate`'s lifecycle flip to `Inactive` still runs unconditionally right after; `deactivate_subtree` still releases inherited edges before the call, and `ElementTree::remove`'s soft-park path still queues the element inactive after it). `ElementBase::{activate, deactivate}` and `ElementBehavior::{on_activate, on_deactivate}` carry an `owner: &mut ElementOwner<'_>` handle for this, mirroring `mount`/`unmount`'s existing shape; `on_activate` stays a plain forward — a `GlobalKey` retake's own containment window already bounds an `activate` panic. A child's `create_element`+`mount`, a `GlobalKey` retake's `activate`+`update`, and an element's `update` are bounded by `ElementTree::{mount_or_substitute, update_or_substitute}` (`tree/element_tree.rs`) — the region is exactly those user-code windows, never the framework code between them (the debug-only duplicate-`GlobalKey` rejection and the retake preflight propagate, ADR-0050); today only the lazy-sliver host (`element/sparse_children.rs`) calls the primitives, the dense reconciler and the `init_state`/`did_change_dependencies` seam adopt them next. The rest of the removal path — `did_unmount_render_object` — is landing in the same series. NOT bounded, by design: the `ErrorView` factory itself (a panicking substitute has nothing left to substitute with) and a lazy sliver delegate's `find_index_by_key` (Flutter parity — the closest analog throws uncaught too). Every caught panic is recorded through `ElementOwner::push_recovered_panic` and drained via `BuildOwner::take_recovered_panics` (`WidgetsBinding::take_recovered_panics` for a realm) instead of only reaching `tracing`.

## Related crates

- `flui-macros` — proc-macro crate that emits `impl View` derives. Consumer must have `flui-view` as direct dependency.
- `flui-rendering` — downstream: View creates RenderObjects via `RenderView::create_render_object()`
