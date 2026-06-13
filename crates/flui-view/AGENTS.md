# AGENTS.md — flui-view

View and Element tree: immutable Views → mutable Elements → RenderObjects. The declarative UI layer.

## What lives here

- **View traits** — `StatelessView`, `StatefulView`, `InheritedView`, `RenderView`, `ProxyView`, `ParentDataView`
- **Element tree** — mutable lifecycle nodes that manage Views: mount, build, update, unmount
- **ElementCore** — central element machinery (`element/core.rs`)
- **BuildContext** — context passed to `build()` methods
- **Widget identity** — `ObjectKey`, `GlobalKey` (process-wide singleton registry)
- **Proc-macro derives** — `#[derive(StatelessView)]`, `#[derive(StatefulView)]` via `flui-macros` (re-exported in `prelude`)
- **Binding** — `WidgetsBinding` trait for build-phase coordination

## Key constraints

- **`test-utils` feature** — enables `MockBuildContext` + `ReconcileEventCollector` tracing Layer fixture. Downstream test crates opt in.
- **`serial_test` required** — GlobalKey registry is a process-wide singleton. Tests that touch it must use `#[serial]` or they race non-deterministically.
- **`trybuild` compile-fail tests** — `tests/ui/` corpus exercises derive macro error messages (e.g., `column_17_compile_error.rs`).
- **No `Box<dyn View>` as struct fields** in element child collections — enforced by port-check trigger #6.
- **No `downcast_ref::<V>()` in update-dispatch path** — enforced by FR-033. `dispatch_view_update` (TypeId-keyed `Box::downcast::<V>`) is the only path.
- **Benchmarks** — `s1_key_storage`, `s2_static_path`, `sc012_global_key_reparent`.
- **`cargo-shear` false positive** — `tests/ui/*.rs` declared in `[package.metadata.cargo-shear] ignored-paths`.

## Related crates

- `flui-macros` — proc-macro crate that emits `impl View` derives. Consumer must have `flui-view` as direct dependency.
- `flui-rendering` — downstream: View creates RenderObjects via `RenderView::create_render_object()`
