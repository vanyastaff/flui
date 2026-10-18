# ADR-0242: `Inspect` replaces `Diagnosticable`; one tracing convention and one span stream

- **Status:** Proposed
- **Date:** 2026-10-18
- **Amends:** [ADR-0040](ADR-0040-tree-observation-seam.md) (consumers of the observer seam join
  span time by element id) and [ADR-0202](ADR-0202-animation-observation-through-tracing.md)
  (animation spans move to the `flui.animation` target and are matched by `(target, name)`)
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md) (`InspectNode` on the wire),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the derive's SDK path),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (the geometry values `Value` carries)
- **Spec:** [flui-driver](../plans/specs/flui-driver/requirements.md), "Introspection and tracing
  across crates"; evidence in the
  [introspection and tracing survey](../research/2026-10-18-flui-introspection-and-tracing.md)

## Context

`Diagnosticable` (`flui-foundation/src/debug.rs`) follows Flutter's design:
- every property is a `Display` string with a separate kind tag;
- tests parse values back (`get_property_f64`);
- `DiagnosticsBuilder` allocates a `String` per name and value on every walk;
- the trait requires `Debug`;
- the derive accepts named structs only and has no `flui-sdk` path.

Only render objects, layers and one GPU profile implement it, so views and elements describe
nothing. Tracing is just as uneven:
- four target spellings;
- the profiler recognizes phase spans by name only;
- gesture recognizers create spans and never enter them;
- `element_rebuild` logs `element = ?id` at INFO;
- `FrameTimingLayer` and `Timeline` are two recorders.

Errors carry bare ids. A driver, an inspector and typed test assertions cannot be built on string
properties.

## Decision

1. **`Inspect`** lives in `flui-foundation::inspect`:

   ```rust
   pub trait Inspect { fn kind(&self) -> &'static str; fn inspect(&self, out: &mut Fields<'_>); }
   ```

   - `Fields` writes into a single `&mut dyn Sink`, so a walk allocates only if its sink does.
   - Values are a closed, `Copy`, borrowed `Value<'a>` set: `Bool`, `Int`, `Uint`, `Float`, `Str`,
     `Variant`, `Size`, `Offset`, `Rect`, `Color`, `EdgeInsets`, `Matrix`, `Duration`, `Link`,
     `Debug`, `None`.
   - Each field carries `Meta { level, unit, default }`, with a typed default compared by value.
   - There is no `Debug` supertrait. `Value::Debug` is counted in snapshots so a ratchet can shrink
     it.
2. **Identity and structure belong to the tree.** `InspectTree` (implemented by the element,
   render and layer trees) yields children, `InspectId`s over the generational ids, typed `Link`s
   across trees, and tree-known facts (lifecycle, geometry, dirty flags, creation site). An object
   never describes its children.
3. **Who implements it.** Render objects and layers require `Inspect`. Views get a provided
   `View::inspect` method that defaults to nothing and is filled by the derive. Stable Rust cannot
   detect at mount whether a type implements a trait, so the hook is a method, and a user view
   with no description still compiles.
4. **`#[derive(Inspect)]`** covers named, tuple and unit structs and enums, and refuses an
   unmapped field type at compile time with a hint. It takes `#[inspect(skip, name, default, level,
   unit, with, debug)]`, takes its kind from the type's identifier (with explicit type arguments for
   generic types), and resolves its paths through `flui_sdk` first.
5. **Outputs.**
   - An owned snapshot is `flui-protocol`'s `InspectNode`, behind `serde` and `schemars`.
   - Typed accessors and `find_by_link` serve tests.
   - A structural diff uses unit-derived tolerances.
   - One role-styled tree renderer, with colour only on a TTY without `NO_COLOR`.
   - A compact, pinned `Display` for geometry and colour; `Debug` is not an output format.
6. **Removal.** `Diagnosticable`, `DiagnosticsNode`, `DiagnosticsBuilder`, `DiagnosticsProperty`,
   their derive and `short_diagnostic_type_name` are deleted once the workspace is migrated.
   `DebugPaintConfig` moves to its own module.
7. **Tracing convention.**
   - Contract targets are `flui.<subsystem>`.
   - Spans are intervals and events are points.
   - Frame-path fields are primitives only, with ids as `u64` and kinds as `&'static str`.
   - Phase spans (build, layout, compositing, paint, semantics, commit, submit) are DEBUG. Per-node
     spans are TRACE, at relayout-boundary and rebuild roots only.
   - Costly fields are gated behind `tracing::enabled!`, and nothing allocates while disabled.
   - Framework crates set no `max_level_*` feature.
   - Old targets keep a one-release alias, which lands only after an `EnvFilter` test confirms
     prefix matching.
8. **One span stream.** A capture layer partitioned by runtime and presentation id feeds the
   profiler, the timeline (now a view over the capture), the driver and a Chrome/Perfetto export
   with `X` events. Phases are matched by `(target, name)` from one table.
9. **Creation sites.** `CreationSite` moves from `flui-animation` to `flui-foundation`. It is
   debug-only and zero-sized in release, and is filled by `#[track_caller]` constructors and builders
   (the widget derive generates them). Elements copy it at mount.
10. **Tree context on errors.** `TreeError<E> { source: E, context: TreeContext }`, with
    `TreeContext { path, site }` capped. The owning boundary (pipeline owner, build owner) attaches
    it, so hot-path errors stay id-only and stay `thiserror` enums. Frame containment adds phase and
    node to internal-invariant reports.

## Consequences

- Tests stop parsing rendered dumps. `tests/agent_workflow.rs` and the render-object harness read
  typed fields.
- The driver's `inspect.*`, `layout.facts`, layout snapshots and trace diffs share one model.
- Migration is mechanical but wide: about 81 files define a fill function and 21 use the derive.
  It lands beside the old trait, migrates `flui-objects` by derive, moves consumers, then deletes
  the old machinery.
- Two published tracing targets change spelling, with an alias for one release.

## Alternatives considered

The survey records the reasons in full:

- **Evolve `Diagnosticable` in place.** Rejected by the owner: it keeps the string values, the
  Flutter-shaped naming and the `Debug` supertrait.
- **`bevy_reflect` or `facet` as the model.** Rejected: they need `'static` and registration, and
  a process-global registry is against ADR-0027; `facet` is pre-1.0 and would become an SDK
  contract.
- **`valuable` for structured tracing fields.** Rejected while tracing keeps it behind
  `--cfg tracing_unstable`, because a library cannot require its users' `RUSTFLAGS`.
- **`serde::Serialize` as the description.** Rejected: it has no level, unit, link or identity. It
  is kept as the snapshot's output format.
- **Compose-style source strings, or Flutter's compiler transform.** Rejected: there is no compiler
  plugin, and `#[track_caller]` is the Rust-native reach.
- **A second profiling API (`puffin`).** Rejected: there is one instrumentation API, and users may
  add layers.
- **Context on every error variant, or `miette` source spans.** Rejected: hot-path errors stay
  small, and FLUI errors point into a tree, not into text.

## Verification

- `flui-foundation`:
  - a value-mapping table;
  - a derive table, with trybuild fixtures for the unmapped-field error;
  - zero allocations for a counting sink (an allocation-counting test allocator);
  - renderer and compact-`Display` goldens with round-trip tests.
- `flui-rendering` and `flui-objects`: the harness reads typed fields, and a diff of two layouts
  reports the moved node with its delta.
- Tracing:
  - an `EnvFilter` test for the new targets and aliases;
  - a phase-table test showing an app span named `paint` is not counted;
  - a disabled-level test showing the frame path allocates nothing;
  - recognizer spans entered or turned into events;
  - `element_rebuild` at TRACE with numeric ids.
- Errors: a layout cycle reports its ancestor path and creation site, and hot-path error
  variants gain no context field.
