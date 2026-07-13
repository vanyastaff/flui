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
- **`trybuild` compile-fail tests** — `tests/ui/` corpus exercises derive macro error messages (e.g., `column_17_compile_error.rs`).
- **No `Box<dyn View>` as struct fields** in element child collections — enforced by port-check trigger #6.
- **No `downcast_ref::<V>()` in update-dispatch path** — enforced by FR-033. `dispatch_view_update` (TypeId-keyed `Box::downcast::<V>`) is the only path.
- **Benchmarks** — `key_storage_shape`, `static_path_algorithm`, `global_key_reparent_latency`.
- **`cargo-shear` false positive** — `tests/ui/*.rs` declared in `[package.metadata.cargo-shear] ignored-paths`.

## Related crates

- `flui-macros` — proc-macro crate that emits `impl View` derives. Consumer must have `flui-view` as direct dependency.
- `flui-rendering` — downstream: View creates RenderObjects via `RenderView::create_render_object()`
