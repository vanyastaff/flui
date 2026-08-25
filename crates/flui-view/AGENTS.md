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

## Related crates

- `flui-macros` — proc-macro crate that emits `impl View` derives. Consumer must have `flui-view` as direct dependency.
- `flui-rendering` — downstream: View creates RenderObjects via `RenderView::create_render_object()`
