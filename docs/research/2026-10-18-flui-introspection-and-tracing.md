# Introspection and tracing for FLUI: replacing `Diagnosticable`, improving observability

- **Date:** 2026-10-18
- **For:** `docs/plans/specs/flui-driver/` (requirements.md, design.md), the owner's decision to replace
  `Diagnosticable` with a typed introspection model, and the request to use `tracing` spans well across crates.
- **Method:** repository facts are cited as `file:line` (repo-relative, read in this worktree). External
  claims cite a primary URL (docs.rs, official docs, or source read through `gh api`). Anything not read
  directly is marked **[unverified]** and collected in the last section.
- **Scope note:** this is a checklist-driven survey plus a proposal. Nothing here is decided until an ADR
  says so.

## Summary

1. **`Diagnosticable` is a string bag.** Every value is rendered through `Display` into a `String`, typed
   only by a separate `DiagnosticsPropertyKind` tag, and read back by parsing (`get_property_f64`). Only
   render objects, `Layer` and `GpuFrameProfile` implement it. **Views and elements implement nothing**, so
   the widget tree has no property source today. About 150 files mention it, 21 use the derive, and the
   derive refuses enums and tuple structs.
2. **`tracing` is used well in pockets, not as a system.** There is a real frame span with build, layout,
   paint and compositing children, and a typed-field convention (`flui_foundation::diagnostics`). But
   targets use four spellings, the devtools layer recognises phase spans by name alone, element identity
   is recorded as a `Debug` string, ten gesture "spans" are created and never entered, and nothing in the
   element/render path carries a creation site.
3. **Prior art converges on one shape:** a *typed, borrowed, visitor-style* description at the node
   (Compose `InspectorInfo`, `valuable`, Flutter's property classes), identity assigned by the *walker*,
   source locations captured as `&'static Location` or compiler-emitted data, and *spans* as the single
   timing source that several sinks consume (tokio-console, tracing-tracy, tracing-chrome).
4. **Proposal:** `Inspect`, a borrowed visitor trait with a closed `Value` enum, per-field metadata, a
   derive that also covers enums, and an owned `InspectTree` snapshot with typed accessors, a human
   renderer, a structural diff and serde output. Tracing gets one target scheme, `(target, name)` span
   identity, `u64`/`&'static str` fields only, per-node spans only at TRACE, and one capture layer that
   feeds the driver, the profiler and Chrome/Perfetto export. Creation sites become a debug-only
   `CreationSite` captured by `#[track_caller]` constructors. Errors carry typed ids, with the element
   path attached at the boundary that knows the tree.
5. **Rejected:** full runtime reflection (`bevy_reflect`, `facet`) as the core model; `valuable` as the
   transport (still behind `tracing_unstable`); Compose-style string source info; a second profiling API
   (`puffin`); static max-level features in framework crates; `miette`-style source-span diagnostics for
   tree errors.

---

# Part 1: Current state in the repo

## 1.1 `Diagnosticable`, `DiagnosticsNode`, `DiagnosticsBuilder`

**Shape.** `crates/flui-foundation/src/debug.rs` (1368 lines):

| Item | Where | Notes |
|---|---|---|
| `DiagnosticLevel` | `debug.rs:22` | Verbosity filter for `to_string_deep_at_level`. |
| `DiagnosticsPropertyKind` | `debug.rs:252` | A tag alongside the string value. |
| `DiagnosticsProperty` | `debug.rs:312` | `new(name, value: impl Display)` stores `String` (`debug.rs:350`); `with_default(impl Into<String>)` (`debug.rs:455`). The default is a *string*, compared as a string. |
| `DiagnosticsNode` | `debug.rs:603` | Name, `Vec<DiagnosticsProperty>`, `Vec<Self>`. `get_property -> Option<&str>` (`:679`), `get_property_f64` (`:729`) parses the string back. |
| `to_string_deep*` | `debug.rs:910`, `:916` | The human renderer. |
| `Diagnosticable: fmt::Debug` | `debug.rs:956` | `to_diagnostics_node()` default builds the node from the stripped `type_name`; `debug_fill_properties(&mut DiagnosticsBuilder)`. |
| `DiagnosticsBuilder` | `debug.rs:1045` | `add`, `add_double`, `add_int`, `add_size`, `add_color`, `add_enum`, ... : 10+ typed-looking helpers that all collapse to strings. |
| `short_diagnostic_type_name` | `debug.rs:~985` | A hand-written type-name scanner, needed because the node name is derived from `std::any::type_name`. |

The `Debug` supertrait means every implementor must also derive or write `Debug`, whether or not anyone
wants its debug output.

**Derive.** `crates/flui-macros/src/derive_diagnosticable.rs` (176 lines) generates only
`debug_fill_properties`, one `add` per named field, with `#[diagnostic(skip)]`. It rejects tuple structs,
unit structs and enums with a compile error (`derive_diagnosticable.rs:~55-85`). Runtime paths resolve
"through the SDK, then the owning crate, then the `flui` facade" (`derive_diagnosticable.rs:24-26`), but
`crates/flui-sdk/ARCHITECTURE.md:150` records that `Diagnosticable`'s derive has no SDK path yet. The
facade re-exports both trait and derive (`src/lib.rs:171`, `src/lib.rs:177`).

**Who implements it** (counted with `rg` over `*.rs`):

- 151 files mention `Diagnosticable`; 81 files define `fn debug_fill_properties` (92 functions); 21
  `#[derive(..Diagnosticable)]` uses; about 323 `.add*` calls, of which about 27 wrap a `format!` argument.
- By crate: `flui-objects` (the render-object catalog, ~100 files), `flui-rendering` (trait bound and
  tests), `flui-layer` (`impl Diagnosticable for Layer`, `crates/flui-layer/src/layer/mod.rs:272`),
  `flui-engine` (`GpuFrameProfile`, `crates/flui-engine/src/profiler.rs:91`), `flui-painting`
  (`display_list/mod.rs`), `flui-macros`, `flui-testing`, and test files.
- `RenderObject<P>: Diagnosticable + Downcast + 'static`
  (`crates/flui-rendering/src/traits/render_object.rs:185`): implementing the trait is mandatory for every
  render object, which is why the catalog is covered.
- **`flui-view` and `flui-widgets` implement nothing.** The only `flui-view` hit is a test type
  (`crates/flui-view/src/element/sparse_children/tests.rs:64`). `flui-driver`'s design says
  `inspect.widgets` reports "`Diagnosticable` properties when the view provides them"
  (`docs/plans/specs/flui-driver/design.md:281`); today no view can.

**Who consumes the output.**

| Consumer | Where | How it reads |
|---|---|---|
| Render dump | `PipelineOwner::debug_diagnostics_tree`, `crates/flui-rendering/src/pipeline/owner/diagnostics.rs` | Builds nodes from `to_diagnostics_node()` and then layers **`format!("{:?}", size)`, `format!("{:?}", node.offset())`, `format!("{geometry:?}")`** on as string properties. Geometry is therefore stringified twice, as `Debug`, in a format the types do not promise. |
| Layer inspection | `crates/flui-layer/src/layer/mod.rs:272-300`, `crates/flui-layer/src/testing/inspect.rs` | Same pattern: `properties.add("bounds", format!("{bounds:?}"))`. |
| Harness | `crates/flui-rendering/src/testing/inspect.rs:187-212`; `render_diagnostics` at `:~140` | `get_property`, `get_property_f64`, `find_descendant_unique(type_name)`. |
| Catalog tests | `crates/flui-objects/tests/render_object_harness.rs` | ~12 `find_descendant("RenderX")` calls (e.g. `:1142`, `:2244`, `:7610`); lookups are by *display name of the type*. |
| Widget tests | `crates/flui-testing/src/widgets.rs:1468,1496`; `tests/vertical_slice_demo.rs:157`; `tests/cupertino_demo.rs:165`; `tests/material_demo.rs:582` | `get_property("text") == Some(text)`: finding a node by its rendered text property. |
| Fixture | `tests/fixtures/notes_flow.rs:188-194` | `get_property("offstage")`, `get_property("skip_count")`. |
| Agent workflow | `tests/agent_workflow.rs:139` (`to_string_deep`) and `has_property_line` (`:~147`) | Whole-line match on `"key: value"` in the deep dump: it parses the *human renderer's output*. |
| Snapshots | `tests/snapshots/*.snap` | Hold layer-structure text (`Offset dx=0.00 dy=0.00 / Picture bounds=...`), apparently from `testing::inspect::structure`, not `DiagnosticsNode`. **[unverified: the producer was not opened]** |

So the string-parse contract is small in call sites (about 10 `get_property*` uses outside `debug.rs`,
plus ~12 `find_descendant` uses) but wide in *formatting* exposure: the dump text is a de-facto API for
`agent_workflow.rs`, and the numeric values in it are `Debug` renderings.

**Mismatches with the repo's own rules.** "Make rules types, not reviews" (AGENTS.md): a size is typed in
the render tree, becomes a `String`, and is parsed as `f64` in tests. "Identity is not a label": test
lookups name `RenderColoredBox` by type-name string, and the node carries no `RenderId`, so two
instances are distinguishable only by `find_descendant_unique` failing.

## 1.2 `tracing` today

**Volume** (non-test, non-bench, non-example `*.rs`, `rg` counts, includes `tools/`): `trace!` 171,
`debug!` 321, `info!` 129, `warn!` 348, `error!` 240. Heaviest emitters are the platform backends and
`flui-engine/src/renderer.rs`. Workspace dependency is `tracing = "0.1"` (`Cargo.toml:317`); no
`max_level_*` features are set anywhere (`rg max_level` over manifests finds none).

**Span inventory** (every `*_span!` in framework sources):

| Span | Where | Level | Target | Fields |
|---|---|---|---|---|
| `frame` | `crates/flui-scheduler/src/scheduler/execution.rs:359` | DEBUG | module path (`flui_scheduler::scheduler::execution`) | `id` declared `Empty`, recorded later with `field::debug(id)` (`:368`): **a Debug string**. |
| `build` | `crates/flui-view/src/owner/build_owner.rs:1498` | DEBUG | module path | `dirty_elements`, `absorbed_mid_drain = Empty` |
| `build` (mid-layout) | `crates/flui-view/src/owner/layout_builder.rs:206` | DEBUG | module path | `during_layout = true`, `absorbed_mid_drain = Empty` |
| `layout` | `crates/flui-rendering/src/pipeline/owner/layout.rs:50` | DEBUG | module path | `dirty_nodes` |
| `compositing` | `.../owner/compositing.rs:63` | DEBUG | module path | `dirty_nodes`; skipped when no work (fast path above it) |
| `paint` | `.../owner/paint.rs:89` | DEBUG | module path | `dirty_nodes`, `layer_updates` |
| `element_rebuild` | `build_owner.rs:1761` | **INFO** | module path | `element = ?id` (Debug string), `reasons = %reasons` (Display string) |
| `animation_tick`, `animation_sampling`, `animation_listeners` | `crates/flui-animation/src/observability.rs:38` | DEBUG | `flui_animation` | none |
| `animation_controller` | `observability.rs:126` (feature `diagnostics`) | DEBUG | `flui_animation::diagnostics` | subscriber-issued identity (ADR-0202) |
| `tap.add_pointer`, `tap.handle_event`, `eager.*`, `long_press.*`, `multidrag.*`, `tap_and_drag.*` | e.g. `crates/flui-interaction/src/recognizers/tap.rs:401,458`, `eager.rs:60,71` | INFO | module path | `pointer = ?pointer`, `event = %GestureEvent::..` |

Observations from reading the code:

1. **The per-frame hierarchy exists** (frame > build/layout/compositing/paint, plus animation phases),
   but there are no spans for semantics assembly, layer-tree commit, raster submit or present. `flui.frame`
   *events* carry presentation telemetry (`crates/flui-runtime/src/ui_runtime/frame.rs:266,461,1138`)
   outside the span tree.
2. **Four target spellings coexist:** `flui.frame`/`flui.pace` (`frame.rs:266,902`; `sampling.rs:181`),
   `flui::signals` (`commands.rs:734`), `flui::reconcile` (`crates/flui-view/src/tree/reconcile_event.rs:26`,
   a declared stability boundary under FR-035), `flui_animation[::diagnostics]`, and the module-path default
   for everything else, including the phase spans devtools depends on.
3. **Devtools matches phases by name only.** `packages/flui-devtools/src/frame_timing_layer.rs:96-101`
   guards the *frame* span by target and name (an application span called `frame` cannot spoof it), but
   `phase_for` (`:113-122`) and `is_frame_or_phase` (`:131-142`) accept `build`, `layout`, `paint`,
   `compositing` by name with no target check. An application's `tracing::debug_span!("paint")` inside a
   frame would be taken for a phase. ADR-0202 hardened exactly this for animation phases; the four core
   phases were not given the same treatment.
4. **Element identity is a string.** `element = ?id` renders `ElementId(index=.., generation=..)`
   (`crates/flui-foundation/src/id.rs:1315`) into the subscriber's formatter. There is no `u64` form on
   the span, no view type, and no render id. A consumer cannot join an `element_rebuild` span to a
   `DiagnosticsNode` or an `ElementRef` without parsing text.
5. **Ten spans are never entered.** `let _span = tracing::info_span!(...);` with no `.entered()` or
   `.enter()` (`tap.rs:401,458`, `eager.rs:60,71`, and the same pattern in `long_press.rs`, `multidrag.rs`,
   `tap_and_drag.rs`). A subscriber sees `new_span` and `close` only; there is no enter/exit, so there is
   no duration, and events emitted afterwards do not nest under them. They behave as events with an
   unusual shape. If they are meant as events, they should be events; if they are meant as intervals, they
   are broken. **[reading of the code; the intent is not documented at those sites]**
6. **Cost gating is rare.** `tracing::enabled!` appears once in production code
   (`frame.rs:1122`, guarding `frame_telemetry`) and once each in animation diagnostics and two platform
   files. The per-frame phase spans are cheap because they are per-phase; `element_rebuild` is per element
   per rebuild at INFO.
7. **A shared-names module exists but has one entry.** `crates/flui-foundation/src/diagnostics.rs` defines
   the rule ("instrument with an identifier that names the real owner", constants only when an emitter
   exists) and `PRESENTATION_ID` (`:48`). Nothing else is named.

**Consumers.**

- `flui-log` (`crates/flui-log/src/lib.rs:1-60`) is only a composition-root subscriber assembler (platform
  sink, filter, ownership policy). Framework crates depend on `tracing` alone.
- `flui-devtools` `FrameTimingLayer` (`packages/flui-devtools/src/frame_timing_layer.rs`) turns spans into
  `FrameProfile`s through a `Profiler`; it is a `tracing-subscriber` layer with a `FilterFn` that admits
  only frame/phase spans. `Timeline` (`packages/flui-devtools/src/timeline.rs`) is a separate event
  recorder with `export_chrome_trace()` (`:529`), emitting `B`/`E` events with `pid: 1` (`:535-556`).
  Neither `frame_timing_layer.rs` nor `profiler.rs` references `Timeline` (checked with `rg`): **two
  recorders, not fed from one span stream.**
- `ADR-0202` (`docs/adr/ADR-0202-animation-observation-through-tracing.md`): animation observation is
  tracing-only behind the `diagnostics` feature, spans at target `flui_animation`, identity from the
  span's subscriber-issued id plus a run generation, "owned scalar state, never borrowed user values or
  user formatting", per-callout panic containment. It is the most careful tracing contract in the repo.
- `ADR-0040` (`docs/adr/ADR-0040-tree-observation-seam.md`): tree observation is a **trait**
  (`TreeObserver` with `ElementMounted`, `ElementMoved`, `ElementRebuilt`, `ElementUnmounted`, carrying
  generational `ElementId` and a `TypeId`), not tracing, because typed payloads degrade to strings, the
  subscriber lifecycle is process-global, there is no per-UI-runtime scoping, tracing is event-only and
  enabled-path dispatch is erased `Visit`. The `flui::reconcile` stream stays for trace tooling. This
  matters for the proposal: *tree mutation* is a trait seam; *time* is spans.

## 1.3 Debug/Display conventions and errors

- Ids: `ElementId` has `Debug` = `ElementId(index=i, generation=g)` and `Display` = `Element(i:g)`
  (`id.rs:1315-1331`), and `TreeId::debug_value() -> u64` (`id.rs:1333`). Render and element ids already
  pack into a `u64`, which is what a span field wants.
- Geometry: `Size<T: Unit + Display>` and `Rect<f64>` implement `Display`
  (`crates/flui-foundation/src/geometry/size.rs:766`, `rect.rs:850`), yet the render dump uses `{:?}`. There
  is no compact, documented "geometry line" format.
- Errors: `thiserror` in libraries per AGENTS.md and `docs/PANIC-POLICY.md`; ~107 files derive an `Error`.
  `RenderError` (`crates/flui-rendering/src/error.rs:71`) is `#[non_exhaustive]` and carries ids only:
  `NodeNotFound(RenderId)` (`:92`), `LayoutCycle(RenderId)` (`:348`),
  `child_index_out_of_bounds(parent, index, count)` (`:549`). **No variant carries an ancestor path, a type
  name or a creation site.** A failed layout therefore reports "layout cycle at render id N", and a person
  must map N to a widget.
- PANIC-POLICY: panics are bug reports, `expect("BUG: <invariant>")` for internal invariants, `Result` for
  caller-triggerable failures. One function reads the `BUG:` prefix back
  (`flui_foundation::panic::is_internal_invariant`).

## 1.4 Creation sites

`#[track_caller]` / `Location::caller` appear only in `flui-animation` and test helpers:

- `crates/flui-animation/src/origin.rs:18`: `CreationSite::caller()`, a `&'static Location` kept "only with
  debug assertions or the `diagnostics` feature; zero-sized otherwise" (`origin.rs:12-14`), used by
  `AnimationControllerBuilder::build` (`builder.rs:150,173`) and `animated_value.rs:239-264`.
- No view, element, render object or widget records where it was created
  (`rg track_caller` over `crates/flui-view` and `crates/flui-widgets` finds nothing). The driver design
  already assumes it: SARIF findings carry "source locations when a node's element records its creation
  site (debug builds)" (`design.md:865`).

---

# Part 2: Prior art

## 2.1 Rust ecosystem

| Item | Model | What works | Costs and pitfalls |
|---|---|---|---|
| `tracing` | Spans (intervals) and events (points) with typed fields; callsite `Metadata` registered once; subscribers decide interest. | Docs: spans "represent a period of time", events mark "points in time within a span"; a span or event no subscriber is interested in "is never constructed"; `?` records via `Debug`, `%` via `Display`; `field::Empty` declares a field to record later. Typed `Value` impls cover all primitives, `str`, `Option<T>`, `dyn Error`. ([docs.rs/tracing](https://docs.rs/tracing/latest/tracing/), [Value](https://docs.rs/tracing-core/latest/tracing_core/field/trait.Value.html)) | `Value` has no struct or sequence form on stable. `?`/`%` defer formatting to the subscriber but flatten the type to text. `#[instrument]` records arguments with `Debug` and needs `syn` (`attributes` feature, default on). Compile-time level features exist (`max_level_*`, `release_max_level_*`) and make "levels above the maximum" disabled at compile time ([level_filters](https://docs.rs/tracing/latest/tracing/level_filters/index.html)). |
| `valuable` in tracing | Object-safe structured value inspection (`Valuable`, `Visit`); tracing records `&dyn Valuable` fields. | Gives subscribers (e.g. the JSON formatter) real structure. | **Still unstable.** `tracing-core` (main, 0.1.36) lists `valuable` only under `[target.'cfg(tracing_unstable)'.dependencies]` ([Cargo.toml](https://github.com/tokio-rs/tracing/blob/main/tracing-core/Cargo.toml)); `tracing/src/lib.rs` documents that these features require `--cfg tracing_unstable` and "may be removed or changed in breaking releases" (lines ~847-873 of [lib.rs](https://github.com/tokio-rs/tracing/blob/main/tracing/src/lib.rs)); the `Value` page says the impls exist "only with both the `tracing_unstable` cfg and the `valuable` crate feature"; the JSON formatter calls it experimental ([fmt::format::Json](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html)). The `valuable` crate itself is 0.1.1, "object-safe value inspection ... across trait-object boundaries" ([Cargo.toml](https://github.com/tokio-rs/valuable/blob/master/valuable/Cargo.toml)). A required `RUSTFLAGS` cfg is not something a library can ask consumers to set. |
| `tracing-subscriber` layers | `Layer<S>` observes events/spans with a `Context`; compose on `Registry`; `Option<L>`, `Vec<L>`, boxed layers; per-layer `Filter`s (`LevelFilter`, `Targets`, `FilterFn`). | A new consumer is a new layer, not a change to emitters. Per-layer filters keep one layer's interest from silencing others ([layer docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/index.html)). | The docs warn that a *global* `enabled`/`register_callsite` returning "never" disables the item for the whole stack; use per-layer filters to target one layer. `FrameTimingLayer` already uses `FilterFn`. |
| `tracing-chrome` | Layer that writes Chrome trace JSON for `chrome://tracing`/Perfetto, via a background thread and a `FlushGuard` ([docs.rs](https://docs.rs/tracing-chrome/latest/tracing_chrome/)). | Zero-code Perfetto export of any span tree. | Writes to a file on guard drop; the page I read does not document field/args handling. **[unverified]** Not designed to be embedded in a session bundle. |
| `tracing-tracy` | Layer mapping spans to Tracy zones and events to messages ([docs.rs](https://docs.rs/tracing-tracy/latest/tracing_tracy/)). | Real profiler UI, low effort. | Only span enter/exit are recorded; events become messages and Tracy "can struggle with large numbers of messages"; out-of-order or cross-thread enter/exit unsupported; `enable` should be conditional because Tracy broadcasts discovery packets and exposes collected data on the local network. |
| `tracing-timing` | Subscriber with inter-event timing histograms ([README](https://github.com/jonhoo/tracing-timing)). | Percentile latency between events. | Event-gap oriented; FLUI wants per-phase durations and worst-frame attribution, which spans already give. |
| `puffin` | Own macro API (`profile_scope!`) writing to thread-local streams; `puffin_http` + `puffin_viewer`; scopes cost "around 50-200 ns" when on and ~1 ns off (README, M1) ([README](https://github.com/EmbarkStudios/puffin)). | Very cheap scopes; frame-oriented (`new_frame()`). | A second instrumentation API next to `tracing`, so every site is annotated twice or one is dropped. |
| `tokio-console` | Instrumentation is `tracing` spans/events from Tokio; `console-subscriber` is a `Layer` that aggregates and serves a gRPC protocol; the TUI is one client ([README](https://github.com/tokio-rs/console), [console-subscriber](https://docs.rs/console-subscriber/latest/console_subscriber/)). | The cleanest precedent for FLUI: *emit through tracing, aggregate in a purpose-built layer, expose over a separate wire protocol, let several front ends consume it.* | Needs targets `tokio`/`runtime` at TRACE and **no compile-time level filters**; Tokio's tracing is behind `tokio_unstable`, so mixing builds forces full rebuilds. Retention is a knob (`retention`), no documented memory cap. |
| `miette` | `Diagnostic: Error` with code, help, severity, url, labelled `SourceSpan`s into `SourceCode`, related errors; graphical, narratable and JSON report handlers ([docs.rs](https://docs.rs/miette/latest/miette/)). | Excellent for errors that point into *text*. | The core value is byte spans in source text. FLUI errors point into a *tree*; there is no source text to label. |
| `error-stack` | `Report<C>` stack of contexts and attachments; `change_context` at boundaries; backtrace and `SpanTrace` capture ([docs.rs](https://docs.rs/error-stack/latest/error_stack/)). | Context accumulates as an error crosses layers. | Docs list development overhead and that `attach`ed values are hidden unless hooks print them; behaviour depends on features and env vars. |
| `eyre`/`anyhow` | Dynamic error reports with context. | Fine for applications. | AGENTS.md reserves `anyhow` for apps; libraries use `thiserror`. Not re-surveyed. |
| `bevy_reflect` + `bevy-inspector-egui` | `Reflect` trait, derive, `TypeRegistry`, `DynamicStruct`, `GetPath`, serializers needing the registry ([docs.rs](https://docs.rs/bevy_reflect/latest/bevy_reflect/)); the inspector renders any registered reflected value ([README](https://github.com/jakobhellermann/bevy-inspector-egui)). | Editing arbitrary data at runtime is possible; the inspector is generic. | Docs list: types must be `'static + Send + Sync`; non-`'static` borrowed data cannot be reflected; generic types are not auto-registered; auto-registration "might slow down compilation"; "small runtime cost". The inspector requires `register_type::<T>()` or the type "won't appear". Read-write reflection is far more than an inspector needs. |
| `facet` | `SHAPE` const describing layout, fields, attributes; crates for JSON, pretty-print, diff ([docs.rs](https://docs.rs/facet/latest/facet/)). | Attractive: derive-light, diff and pretty-print exist. | Docs: crates "in various states of progress, buyer beware"; `facet-inspect` "less developed". Version 0.46 at time of reading. Adopting it would make a pre-1.0 third-party schema type the contract of an Evolving SDK surface. |
| serde-based introspection | `Serialize` walks a value into any `Serializer`. | Mature; `insta` redactions and JSON output come for free ([insta redactions](https://insta.rs/docs/redactions/)). | `Serialize` has no notion of *level*, *unit*, *default*, *link* or *identity*, and requires every inspected type to be serde-serializable (render objects hold closures, callbacks, `Arc<dyn ..>`). Good as the *output* format of an `Inspect` snapshot, not as the description trait. |
| `Debug` builders | `debug_struct`, `finish_non_exhaustive`, `{:#?}` pretty printing ([DebugStruct](https://doc.rust-lang.org/std/fmt/struct.DebugStruct.html)). | Ubiquitous, zero setup. | `field_with` is nightly-only; Debug output is explicitly not a stable format. Today's FLUI dump stringifies geometry through it. |
| `insta` | Snapshot assertions; with `serde`, structured YAML/JSON with selector redactions, `dynamic_redaction`, `sorted_redaction`. | Redactions let a structured tree snapshot ignore ids and floats' noise. | Redaction needs `Serialize`; a changed snapshot still needs a human claim (AGENTS.md). |

## 2.2 UI frameworks

| Framework | Introspection model | Source locations / identity | Pitfalls for FLUI |
|---|---|---|---|
| **Flutter DevTools** | `DiagnosticsNode` tree with typed property subclasses (`DoubleProperty`, `IntProperty`, `FlagProperty`, `EnumProperty`, `IterableProperty`, `ObjectFlagProperty`, `DiagnosticsProperty<T>`), each overriding `toJsonMap`, which adds `unit`, `defaultValue`, `ifTrue/ifFalse`, `tooltip`, `level`, `style`, `propertyType`, `isDiagnosticableValue`, `hasChildren`, `truncated` ([diagnostics.dart](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/foundation/diagnostics.dart), lines ~1639-1716, 2014-2675). The inspector service answers JSON requests with a configurable `DiagnosticsSerializationDelegate` (subtree depth, include properties) ([widget_inspector.dart](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/widget_inspector.dart), ~2307-2320). | `--track-widget-creation`, on by default in debug builds, records a creation location per widget; `creationLocation`, `locationId` and `createdByLocalProject` are added to the JSON (`widget_inspector.dart` ~4477-4485, flag text ~1909); local-project widgets are shown at a finer level than library ones (`:~4513`). | The model was designed *typed* (the Dart classes carry types), but the *transport* is strings plus a few typed fields, and the Rust port copied the string form, not the typing. The creation-location mechanism is a compiler (kernel) transform; the exact transform is **[unverified]**. |
| Flutter `debug*` family | `debugDumpApp`, `debugDumpRenderTree` (size, constraints, creator), `debugPrint*`, `debugFillProperties` feeds them; debug-mode only ([code-debugging](https://docs.flutter.dev/testing/code-debugging)). | `creator:` chain in render dumps. | Text dumps are human tools; Flutter's own tests parse them rarely. FLUI's `agent_workflow.rs` does. |
| **Jetpack Compose** | `Modifier.inspectable` / `InspectorInfo` / `InspectableValue`: a builder with `name`, `value` and `properties: ValueElementSequence` of `ValueElement(name, value: Any?)`; gated by a global flag `isDebugInspectorInfoEnabled`, otherwise the cheap `NoInspectorInfo` lambda is used ([InspectableValue.kt](https://github.com/androidx/androidx/blob/androidx-main/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/platform/InspectableValue.kt)). | The compiler plugin calls `Composer.sourceInformation(String)` and `sourceInformationMarkerStart(key: Int, sourceInformation: String)`, "a string value to that provides the compose tools enough information to calculate the source location" ([Composer.kt](https://github.com/androidx/androidx/blob/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/Composer.kt), ~396-423). Layout Inspector shows recomposition and skip counts for debuggable processes ([Layout Inspector](https://developer.android.com/studio/debug/layout-inspector)). | Two ideas are valuable: *inspector info is lazy and gated by a global switch*, and *recomposition and skip counts are first-class*. The source info is an opaque string produced by a compiler plugin FLUI does not have, and `value: Any?` is untyped. |
| **SwiftUI** | `Self._printChanges()` prints which property caused a body re-evaluation; Instruments has a SwiftUI template with a View Body lane. | None in app code. | Underscored, undocumented, reported false positives; I found only third-party descriptions, not Apple's page. **[unverified against Apple docs]** The lesson is the *question* it answers ("why did this rebuild"): FLUI already has `RebuildReason`/`RebuildReasons` (ADR-0040) and the `reasons` field on `element_rebuild`. |
| **React DevTools** | Component tree, props/state in `$r`; `componentDidCatch` receives a `componentStack`; `React.captureOwnerStack()` is available in development only ([Component](https://react.dev/reference/react/Component), [DevTools README](https://github.com/facebook/react/blob/main/packages/react-devtools/README.md)). | Component stack has "names and source locations"; owner stack distinguishes *who rendered it* from *where it sits*. | I could not confirm "why did this render" from the pages read. **[unverified]** Idea worth keeping: an error report should carry both the *tree path* (ancestors) and the *owner/creation* (where it was written). |
| **Xilem/Masonry** | `Widget::make_trace_span(&self, id: WidgetId) -> Span` returning `trace_span!("Widget", r#type = self.short_type_name(), id = id.trace())`; passes are wrapped in `info_span!("compose")`, `("update_anim")`, `("accessibility")`, `("mutate_widget", name = widget.short_type_name())`; `get_debug_text()` adds text next to the type ([widget.rs](https://github.com/linebender/xilem/blob/main/masonry_core/src/core/widget.rs) ~498-514, [passes](https://github.com/linebender/xilem/tree/main/masonry_core/src/passes)). | Id is `WidgetId` formatted via `.trace()`; no creation site. | Closest Rust analogue: per-widget spans *at TRACE*, pass spans at INFO, identity in the span. The span is created while recursing the tree, so cost scales with the tree whenever TRACE is on. |
| **egui** | `Context::inspection_ui`, `debug_on_hover` with callstacks, and `profiling::scope!` calls throughout ([context.rs](https://github.com/emilk/egui/blob/main/crates/egui/src/context.rs), [plugin.rs](https://github.com/emilk/egui/blob/main/crates/egui/src/plugin.rs)). | `show_callstacks` when hovering (debug option). | egui uses the `profiling` abstraction crate, which can route to `tracing`, `puffin` or others (changelog mentions a tracing backend fix). Immediate mode has no retained tree to inspect, so the model does not transfer. |
| **GPUI** | `InspectorElementId { path: Rc<InspectorElementPath>, instance_id }`; `InspectorElementPath { global_id, source_location: &'static Location<'static> }`, under `cfg(any(feature = "inspector", debug_assertions))`; "Source location where this element was constructed" ([inspector.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/inspector.rs)). Tests use `VisualTestContext::debug_bounds` with string selectors. | `&'static Location` via `#[track_caller]` builders, compiled out in release. | Closest to FLUI's Rust shape: debug-only `Location`, plus a disambiguating instance id. The `debug_bounds` string-key map is the kind of test hook FLUI should replace with typed ids. |
| **Slint** | Live preview with a property editor; `slint-viewer --auto-reload`, `--check` ([viewer docs](https://docs.slint.dev/latest/docs/slint/guide/tooling/slint-viewer/)). | Compiled markup, so source positions are intrinsic. | I did not find a current doc page for an "inspector" element tree. **[unverified]** Markup-driven; not a model for Rust values. |
| **Iced** | `iced_debug` crate: `Span` helpers for `update(message)`, `view(window)`, `layout(window)`, `draw(window)`, `time(name)`, a `Command::RewindTo{message}` for time travel, optional `enable` feature that wires `iced_beacon` (the `comet` tool) ([lib.rs](https://github.com/iced-rs/iced/blob/master/debug/src/lib.rs), [Cargo.toml](https://github.com/iced-rs/iced/blob/master/debug/Cargo.toml)). | Per-phase spans plus the Debug of the message. | Own span type rather than `tracing`; a feature-gated beacon. Evidence for the per-phase span set (update, view, layout, draw) FLUI should have. |

## 2.3 Trace formats and conventions

- **Chrome trace event format and Perfetto.** Perfetto imports JSON traces: `B`/`E` and `X` become slices,
  `I` instants, `C` counters, `M` metadata, `s`/`t`/`f` flows as arrows. Duration events "must nest";
  overlapping non-nested events are "out of spec and inherently ambiguous" and land on an overflow track.
  JSON is convenient for third parties; Perfetto's native TrackEvent protobuf is recommended for more power
  ([Perfetto: other formats](https://perfetto.dev/docs/getting-started/other-formats)). For FLUI this
  means: export `X` events for phases (self-contained), counters for dirty counts, flows for
  frame-to-present, and async `b`/`e` only if needed (Perfetto's page does not list them as supported).
  `Timeline::export_chrome_trace` already emits `B`/`E` with a constant `pid` (`timeline.rs:535-556`).
- **OpenTelemetry attribute naming.** Lowercase, dot-delimited namespaces, `snake_case` within a segment,
  no consecutive delimiters; application-specific names take an application prefix and must not reuse an
  existing namespace ([naming](https://opentelemetry.io/docs/specs/semconv/general/attribute-naming/)).
  That page does not cover span-name cardinality **[unverified here]**. Relevance is limited to a naming
  discipline for field names and a precedent for dotted namespaces; FLUI is not exporting to an OTel
  backend.

---

# Part 3: Proposal

The owner's two decisions are taken as given: `Diagnosticable` is replaced, and observability is improved
across crates. Where a prior-art idea does not fit, it is rejected below with a reason specific to FLUI.

## 3.1 The `Inspect` model

**Goals.** (1) Values keep their types from the object to the test. (2) Describing a node allocates
nothing unless a sink chooses to. (3) Identity, links and creation sites are separate from per-object
fields. (4) One description serves the human renderer, `--json`, the driver protocol, tests and diffs.
(5) The model is a closed set of value kinds, so it can be a wire contract.

**Where it lives.** The trait, `Value`, `Field`, `Level`, `Unit` and the derive target live in
`flui-foundation` (value layer, next to the geometry types they describe, ADR-0098). Owned snapshot
types with serde live in `flui-protocol` behind its existing `serde` feature
(`crates/flui-protocol/Cargo.toml:19`). `flui-sdk` re-exports the trait and the derive so official
packages use `flui_sdk::inspect::Inspect` and `#[derive(Inspect)]` (closing the gap recorded at
`crates/flui-sdk/ARCHITECTURE.md:150`; the derive resolves its runtime path the way the existing
resolver does).

### Trait and visitor

```rust
// flui-foundation::inspect

/// One node's own description. No Debug supertrait.
pub trait Inspect {
    /// Stable, human-meaningful kind name; defaults to the short type name.
    /// A `&'static str`, not a `String`: the derive supplies it from the type.
    fn kind(&self) -> &'static str;

    /// Describe own fields. Called by tree walkers; never recurses into children
    /// (structure belongs to the tree that owns it).
    fn inspect(&self, out: &mut Fields<'_>);
}

/// A borrowed, closed set of value kinds. Copy; never owns heap data.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value<'a> {
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(&'a str),
    /// A fieldless enum variant: `("Axis", "Horizontal")`. The derive emits both as statics.
    Variant { ty: &'static str, name: &'static str },
    Size(Size), Offset(Offset), Rect(Rect), Color(Color),
    EdgeInsets(EdgeInsets), Matrix(Matrix4),
    Duration(core::time::Duration),
    /// A typed reference to another node. See "Links".
    Link(Link),
    /// An escape hatch for types with only `Debug`: counted as untyped by the snapshot.
    Debug(&'a dyn core::fmt::Debug),
    None,
}

pub struct Field<'a> {
    pub name: &'static str,
    pub value: Value<'a>,
    pub meta: Meta,
}

#[derive(Clone, Copy, Default)]
pub struct Meta {
    pub level: Level,                 // Summary | Detail | Debug
    pub unit: Option<Unit>,           // LogicalPx, DevicePx, Seconds, Degrees, ...
    pub default: Option<Default>,     // see below
}

/// Receives fields. The one dyn boundary in the model.
pub trait Sink {
    fn field(&mut self, field: Field<'_>);
    fn seq(&mut self, name: &'static str, items: &mut dyn FnMut(&mut dyn Sink));
}

pub struct Fields<'a> { sink: &'a mut dyn Sink }
impl<'a> Fields<'a> {
    pub fn field<'v>(&mut self, name: &'static str, v: impl IntoValue<'v>) -> FieldMeta<'_, 'v>;
}
// FieldMeta: .level(Level) .unit(Unit) .default(impl IntoValue) ; emits on drop.
```

Notes on the shape:

- **Visitor, not builder.** A `Sink` receives borrowed `Field`s; the allocation decision belongs to the sink.
  A text sink writes straight to a `fmt::Write`; the snapshot sink copies; a counting or filtering sink
  (`find a node whose "text" field is "Save"`) allocates nothing. `DiagnosticsBuilder` always allocates a
  `String` per name and value (`debug.rs:350`). The single `&mut dyn Sink` is a deliberate object boundary:
  render objects are already `dyn` in the render tree, so static dispatch buys nothing here (AGENTS.md:
  "`dyn` where a heterogeneous collection or an object boundary needs it").
- **`default` is typed.** `Meta::default` holds a `Value`-compatible default (`Some(Value::Float(0.0))`),
  compared by value, not by string. The walker or the renderer hides a field equal to its default at
  `Level::Summary`. The derive can read `#[inspect(default)]` to use `Default::default()`.
- **`unit` and `level` are metadata, not part of the value.** Level replaces `DiagnosticLevel` and keeps
  its role (hide detail in summaries); unit lets the renderer and the diff pick tolerances.
- **`Value::Debug` is the single concession**, for types that cannot yet describe themselves. A snapshot
  records how many fields were `Debug`-only; a gate can ratchet that number down.
- **No `valuable`, no `Serialize` bound.** See verdicts.

### Identity, children and links

Identity is **not a field and not owned by the object**. The tree walker assigns it:

```rust
pub struct InspectId(/* kind prefix + generational slot, per design.md:288 */);

pub enum Link {            // typed cross-tree references
    Element(ElementId), Render(RenderId), Layer(LayerId), Semantics(SemanticsId),
}

pub trait InspectTree {                      // implemented by ElementTree, render tree, LayerTree
    type Node: Inspect;
    fn root(&self) -> Option<InspectId>;
    fn node(&self, id: InspectId) -> Option<NodeView<'_, Self::Node>>;   // fields + site + links
    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId>;
}
```

- Children come from the tree, so a tree can walk lazily, cap depth (`omitted_children`, as in
  `design.md:212`), and never builds an object graph just to describe it.
- `Link`s let a widget-tree node name its render object and semantics node as typed ids; tests and the
  driver stop joining on strings. This is the "identity is not a label" rule applied to inspection.
- A `NodeView` carries the *tree-known* facts that are not the object's own: id, parent, lifecycle state,
  element kind (`debug_kind`, today at `crates/flui-view/src/element/behavior.rs:257`), committed geometry
  and offset, dirty flags, and the creation site (3.3). The render dump stops building
  `format!("{:?}", size)` strings and emits `Value::Size(size)` once.

### Views and elements

Views gain `Inspect` the same way as render objects: `#[derive(Inspect)]` on the widget struct, or nothing.
`View` does **not** require `Inspect` (so existing user views still compile); the element stores a
`fn(&dyn Any, &mut Fields)` pointer captured at mount when the view type implements it. That keeps the
bound off the public `View` trait and avoids a blanket cost; the type-name and `TypeId` are recorded at
mount for views that do not (as `design.md:280` already plans). Render objects keep a *required*
`Inspect` (replacing the `Diagnosticable` bound at `render_object.rs:185`) because the harness and the
catalog depend on it.

### Derive

`#[derive(Inspect)]`:

- named structs, **tuple structs, unit structs and enums** (the current derive rejects all but named
  structs). Enum fields become `Value::Variant`; enums as whole nodes use the variant name as `kind` plus
  the variant's fields.
- attributes: `#[inspect(skip)]`, `#[inspect(name = "..")]`, `#[inspect(default)]` /
  `#[inspect(default = EXPR)]`, `#[inspect(level = detail|debug)]`, `#[inspect(unit = logical_px)]`,
  `#[inspect(with = path)]` for a custom projection to `Value`.
- field types implement `IntoValue` (provided for primitives, `&str`/`String`, `Option<T>`, geometry,
  `Color`, `Duration`, ids, `Vec<T: IntoValue>` as a sequence). A field type with no impl is a **compile
  error with a hint** (`skip`, `with`, or `#[inspect(debug)]` for the `Value::Debug` escape hatch). The
  current derive instead adds a per-field `Debug` where-bound and stringifies.
- the kind comes from `stringify!(Ident)`: no `type_name` scanning, so `short_diagnostic_type_name`
  (`debug.rs:~985`) is deleted. Generic types need care: the current code keeps type arguments
  (`RenderViewport<ScrollPosition>`) and its doc comment explains why that matters. The derive should
  emit a `kind_args` list for generic types, built from `type_name` of each parameter only in the generic
  case. How exactly is left open.
- paths: runtime paths resolve through `flui_sdk` first, then the owning crate, then the facade, as the
  existing resolver does.

### Serialization, human renderer, typed access, diff

- **Snapshot.** `InspectTree::snapshot(query) -> Snapshot` fills owned
  `SnapshotNode { id, kind, fields: Vec<(Cow<'static, str>, OwnedValue, Meta)>, links, site, children,
  omitted_children }`. `OwnedValue` mirrors `Value` without borrows. With `flui-protocol`'s `serde`
  feature this is the `InspectNode` in `design.md:212`; `schemars` (the existing `schemars` feature,
  `Cargo.toml:22`) emits the JSON Schema for MCP clients.
- **Human renderer.** One renderer over `Sink` writes the tree outline (3.5). It replaces
  `to_string_deep`. Geometry uses a documented compact `Display` (3.5), not `Debug`.
- **Typed test access.** On `SnapshotNode`: `node.get::<f64>("width")`, `node.size("size")`,
  `node.text("text")`, each returning `Result<T, FieldError>` with `Missing`, `WrongKind { expected,
  found }`. `find_by_link(Link::Render(id))` replaces `find_descendant("RenderX")` for identity;
  `find_by_kind("RenderX")` stays for catalog tests but returns *all* matches and the helper
  `unique()` errors on ambiguity, as `find_descendant_unique` does today. `get_property_f64` parsing is
  gone.
- **Structural diff.** `diff(&Snapshot, &Snapshot, DiffOptions) -> Vec<Change>` matches nodes by
  `InspectId` when both have one and by `(parent, index, kind)` otherwise; fields compare by `Value`
  equality, with floats compared under an explicit tolerance taken from `Meta::unit` (a pixel tolerance for
  `LogicalPx`, none for `Variant`). `Change` is `Added | Removed | Field { name, from, to } | Moved`.
  This is what `trace diff`, golden geometry and the CI report need, and it is why metadata must be typed.

### Migration

The cost is mechanical but large (81 files define the fill function, 21 derive, ~323 `add` calls). It
follows the AGENTS.md "ID offset"/derive pattern: land the new trait and derive beside the old, migrate
`flui-objects` by derive where possible (most `debug_fill_properties` bodies are field lists), migrate the
~25 consumer call sites, then delete `debug.rs`'s property machinery. The `Diagnosticable` derive and the
`short_diagnostic_type_name` scanner are removed, not deprecated (pre-1.0, AGENTS.md: "fix a bad shape
instead of working around it"). `DebugPaintConfig` (`debug.rs:1306`) is unrelated and moves to a
`debug_paint` module. Tests currently pinned to the dump's text (`agent_workflow.rs`) move to typed
accessors.

## 3.2 Tracing conventions

### Targets

One scheme for **contract-bearing** instrumentation, i.e. anything a layer, the driver or a test filters
on: `flui.<subsystem>`, dot-separated, lowercase, no `::`:

| Target | Contents | Today |
|---|---|---|
| `flui.frame` | `frame` span, phase spans (`build`, `layout`, `compositing`, `paint`, `semantics`, `commit`, `submit`), frame telemetry events | `flui.frame` events exist; spans use module paths |
| `flui.pace` | pacing and presentation timing | exists |
| `flui.tree` | reconcile events; element rebuild spans | `flui::reconcile` (FR-035 boundary) and `element_rebuild` |
| `flui.signals` | reactive reads/writes | `flui::signals` |
| `flui.animation` | the three animation phase spans, controller identity spans | `flui_animation`, `flui_animation::diagnostics` (ADR-0202) |
| `flui.input` | gesture arena and recognizer events | module paths |

Everything else keeps the module-path default and is ordinary logging. The two renames that break
published contracts (`flui::reconcile`, `flui_animation`) get the FR-035 treatment: emit under the new
target and keep the old for one release. I could not verify how `EnvFilter` directives prefix-match targets
containing `.` versus `::` **[unverified]**, so the rename should be confirmed with a test through the real
`EnvFilter` before it lands.

### Spans versus events

- **Span** = an interval someone wants a duration or a parent for: `frame`, each phase, a relayout-boundary
  layout, an element rebuild, an animation tick, a gesture arena resolution, an asset load.
- **Event** = a point: `tree_revision_committed`, a reconcile disposition, a pointer routing decision, a
  state transition. The ten recognizer "spans" (1.2 item 5) become events, or are entered.
- Spans never carry large payloads; events carry the typed facts.

### Fields

- **Only `u64`, `i64`, `bool`, `f64`, `&'static str`** (and `&str` that already exists) on spans and events
  in the frame path. No `?x` or `%x` on a hot path: they erase the type (ADR-0040 point 1) and allocate when
  an interested subscriber formats them.
- Names are `snake_case` constants in `flui_foundation::diagnostics` (the module's own rule: add a constant
  when an emitter exists): `FRAME_ID`, `PRESENTATION_ID` (exists), `ELEMENT_ID`, `RENDER_ID`, `VIEW_KIND`,
  `RENDER_KIND`, `DIRTY_NODES`, `REASONS` (a bitmask `u64` of `RebuildReasons`, which is a `Copy` bitset,
  ADR-0040). Ids are `TreeId::debug_value()` (`id.rs:1333`), a `u64` that maps back to the generational
  id, so the driver can join `element_rebuild{element}` to an `ElementRef`.
- Type names are the `kind()` `&'static str`, not `type_name::<T>()` strings.
- Keep OTel's discipline (lowercase, snake segments, no collisions) as a naming review rule; do not import
  OTel namespaces.

### Span hierarchy and levels

```
frame                                   DEBUG  flui.frame   frame_id, presentation_id
  build                                 DEBUG                dirty_elements, absorbed_mid_drain
    element_rebuild                     TRACE  flui.tree     element, view_kind, reasons
  layout                                DEBUG                dirty_nodes
    layout_root                         TRACE                render_id, render_kind   (per relayout boundary)
  compositing                           DEBUG                dirty_nodes
  paint                                 DEBUG                dirty_nodes, layer_updates
  semantics                             DEBUG                dirty_nodes             (new)
  commit / submit                       DEBUG                (new; engine side)
animation_tick / sampling / listeners   DEBUG  flui.animation (ADR-0202, unchanged semantics)
```

- Phase spans stay DEBUG (they exist and are cheap: per phase, not per node).
- `element_rebuild` moves from INFO to **TRACE**: it is per element per rebuild, at INFO it is emitted in
  a default-filtered production run.
- Per-node spans (Masonry's `make_trace_span` pattern) are limited to **relayout-boundary roots and
  rebuild roots**, not every `layout_child` call, so TRACE costs scale with dirty subtrees rather than tree
  size.

### Cost rules for the frame path

1. A span with fields on the frame path is created behind `if tracing::enabled!(target: .., Level::X)` when
   building a field value costs anything (e.g. `queue_len()`; `layout.rs:50` computes
   `layout_queue_len()` eagerly today, though it is cheap). Cheap `Copy` reads need no gate; the `tracing`
   docs say an uninterested callsite "is never constructed".
2. No allocation when disabled: no `format!`, no `String`, no `Vec` before the macro.
3. No per-node span below TRACE. No span in `perform_layout` of a leaf.
4. `field::Empty` + `record` for values known only at the end (already used for `absorbed_mid_drain`);
   declare every field up front, because recording an undeclared field is silently dropped (the comment at
   `layout_builder.rs:~200` records this existing trap).
5. Do **not** set `max_level_*`/`release_max_level_*` in framework crates or the facade: they would make
   release-build profiling and the driver's `perf` capability impossible, and tokio-console's docs warn
   against compile-time filters for the same reason. An application may set them.
6. Nothing in the frame path awaits or locks to emit; the dispatcher is the only shared state, and
   user-code reentrancy rules from AGENTS.md ("Treat user code as reentrant") apply to subscribers (ADR-0202
   already contains their panics).

### One span stream, several consumers

```
framework crates --(tracing spans/events, flui.* targets)--> Registry
                                                              |
                  CaptureLayer (flui-devtools, per-UI-runtime ring) <--- FilterFn on (target, name)
                    |-> FrameProfile / Profiler (HUD, tests)
                    |-> driver `perf` + `trace` (spans -> JSONL rows with ElementRef joins)
                    |-> Chrome/Perfetto exporter ('X' events, counters, flows)
                    |-> tracing-tracy / tracing-chrome (optional, user-added layers)
```

- `FrameTimingLayer` recognises spans by **`(target, name)` pair**, in one table of
  `(target, name) -> Phase`, so the application's `paint` span cannot spoof a phase.
- `Timeline` stops being a second recorder: its event source becomes the same captured spans, and
  `export_chrome_trace` emits `X` events (self-contained, nest-safe) instead of `B`/`E` pairs.
- Tree *mutation* stays on the `TreeObserver` trait (ADR-0040); *time* stays on spans. The driver joins the
  two by `ElementId`.
- Per-UI-runtime scoping, which ADR-0040 notes tracing lacks, is solved at the consumer: spans carry
  `presentation_id`/runtime id as a field, and the capture layer partitions by it.

## 3.3 Widget creation-site tracking

**What is possible in Rust.** `#[track_caller]` makes `Location::caller()` return the first unattributed
caller's location; the compiler passes an implicit extra argument ([Rust Reference](https://doc.rust-lang.org/reference/attributes/codegen.html)).
Limits that matter here: it works on *function calls*, not struct-literal construction; through a function
pointer or trait object the location collapses to the definition site; and the location is "a hint".

**Design.**

- Move `CreationSite` from `crates/flui-animation/src/origin.rs` to `flui-foundation`: a `Copy` wrapper over
  `&'static Location<'static>` kept under `cfg(any(debug_assertions, feature = "diagnostics"))` and
  zero-sized otherwise, exactly as the animation crate already does. GPUI uses the same cfg shape
  (`any(feature = "inspector", debug_assertions)`).
- `View::creation_site(&self) -> Option<CreationSite>` defaults to `None`. Widgets with a constructor or
  builder entry point mark it `#[track_caller]` and store the site; the builder derive emits both. The
  element copies the site at mount, so a rebuilt-but-identical view keeps the site of the code that wrote it.
- Inspector shows `file:line:col`. Findings carry it (`design.md:865`). Errors carry it (3.4).

**Cost.** Release: zero (type is ZST, no `Location` static, no extra ABI argument if the attribute is also
`cfg_attr(debug)`; confirming that rustc elides the argument when the attribute is conditionally absent is
trivial because the attribute is not present). Debug: one pointer-sized hidden argument per tracked call and
one `Location` static per call site; fine.

**Ergonomics gap.** Struct-literal widgets (`Padding { .. }`) get no site. Either constructors are the
documented entry point (`Padding::new(..)` or builders), or the widget derive generates a `#[track_caller]
fn new`. A markup macro would capture sites for free but contradicts the owner's authoring stance (no
React/HTML-like markup or DSL), so it is rejected as a means.

**How Flutter and Compose do it.** Flutter: a compiler transform under `--track-widget-creation`, default
on in debug, giving each widget a `creationLocation`; the inspector filters "local project" widgets
(`widget_inspector.dart` ~4477-4515). Compose: the compiler plugin emits `sourceInformation` strings per
group. Both depend on compiler support FLUI does not have; `#[track_caller]` is the Rust-native equivalent
with a narrower reach (calls only).

## 3.4 Errors with tree context

Tree errors are *caller-triggerable or invariant-breaking inside the tree*: `LayoutCycle(RenderId)`,
`NodeNotFound`, constraint violations, build panics inside a user `build`. Today they carry a bare id
(1.3). Proposal:

- A small `TreeContext { path: Vec<PathSegment>, site: Option<CreationSite> }` (owned, built only on the
  error path) where `PathSegment { id: InspectId, kind: &'static str }`, ancestor-first, capped (e.g. 12
  segments plus `omitted`). Built by the **boundary that owns the tree**: the pipeline owner on a
  `RenderError`, the `BuildOwner` on a build failure. The error type deep in `layout_child` keeps its
  cheap id-only variant.
- `RenderError` stays a `thiserror` enum (AGENTS.md); the context is attached by a wrapper,
  `TreeError<E> { source: E, context: TreeContext }` with `Display` printing
  `layout cycle at RenderPadding (r12) < RenderFlex (r7) < ... , created at src/app.rs:41:13`, and
  `std::error::Error::source()` returning `E`. A variant is not widened with a context field for each
  error: that would put a `Vec` in every error value, including those produced on hot paths.
- `PANIC-POLICY.md` fit: caller-triggerable failures stay `Result`. For `expect("BUG: ...")` invariants
  that fire inside a frame, the frame-containment boundary (the same place that already reads the `BUG:`
  prefix through `flui_foundation::panic::is_internal_invariant`) appends the *current phase and node* to
  the report, because at that point the pipeline knows which node it was processing. No new panic path.
- The same `TreeContext` goes into the `frame` span's recorded outcome and into the driver's `Finding`
  rows, so a failure in a trace and a failure in a terminal read the same.
- `miette`-style labelled source text is rejected (3.6), but its *shape* (code, help, related) is a good
  template for the `Finding` rows the driver already designs (rule id + message + refs).

## 3.5 Unified output formatting

- **One tree renderer** over the `Sink`/`InspectTree` pair, used by tests, `flui drive inspect`, the
  render dump and error reports. Layout:

  ```
  RenderFlex r7  axis=Horizontal  size=360x48  @0,0            lib/app.rs:41
    RenderPadding r12  padding=8  size=344x32  @8,8
      RenderParagraph r19  text="Save"  size=40x14
  ```

  Name, id and the creation site on the first line, fields after, defaults hidden at Summary level,
  `omitted_children` shown as `... N more`.
- **Compact geometry `Display`** (a foundation task, independent of `Inspect`): `Size` as `360x48`, `Offset`
  as `@8,8`, `Rect` as `8,8 344x32`, `EdgeInsets` as `8` or `8,4,8,4`, `Color` as `#RRGGBB[AA]`, with a
  documented, pinned format and round-trip tests. `Debug` is reserved for debugging, not output.
- **TTY colour** is the caller's decision: the renderer emits semantic roles (`Kind`, `Id`, `FieldName`,
  `Value`, `Dim`, `Site`) through a small `Style` trait; `flui-cli` maps roles to ANSI only when stdout is a
  terminal and `NO_COLOR` is unset (`miette` documents the same env-driven switch between graphical and
  plain handlers); tests use the plain mapper.
- **`--json`** is the serde snapshot, one document per command, same field names as the protocol; the
  human view is never parsed (the regression `agent_workflow.rs` demonstrates today).
- **Trace output**: the same renderer prints span trees from a captured trace; the Chrome/Perfetto file is
  an export of the same capture.

## 3.6 Verdict table

| Idea | Source | Verdict | Reason specific to FLUI |
|---|---|---|---|
| Typed property classes with unit, default, level, tooltip | Flutter `diagnostics.dart` | ACCEPT WITH CHANGE | Keep the metadata, drop the class hierarchy and the string value: `Value` enum + `Meta`; typed defaults compared by value. |
| String-valued properties parsed back in tests | current `Diagnosticable`, Flutter transport | REJECT | Violates "make rules types"; `get_property_f64` exists only to undo `Display`. |
| `Diagnosticable: Debug` supertrait | current repo | REJECT | Forces `Debug` on every render object for a different purpose; `Inspect` has no supertrait. |
| Borrowed visitor with a single `dyn Sink` | `valuable` `Visit`, Compose `InspectorInfo` | ACCEPT | Zero-alloc sinks (search, count, text); one object boundary, where render objects are already `dyn`. |
| Builder returning `Vec<Property>` | current `DiagnosticsBuilder` | REJECT | Allocates a `String` per name and value on every inspection, even for a filter that discards them. |
| Global switch making inspector info lazy | Compose `isDebugInspectorInfoEnabled`/`NoInspectorInfo` | ACCEPT WITH CHANGE | Inspection is already pull-based (`inspect` is only called by walkers), so no global flag; the per-view fn pointer is captured at mount instead. |
| Full runtime reflection (get/set by path, type registry) | `bevy_reflect`, `bevy-inspector-egui` | REJECT | Needs `'static + Send + Sync` and registration; cannot reflect borrowed or closure-holding render objects; read-write is out of scope for an inspector; a registry is process-global state against ADR-0027. |
| `facet` shapes as the description | `facet` | REJECT (for now) | Pre-1.0 ("buyer beware") third-party type would become an Evolving SDK contract. Revisit as a possible *backend* for the derive if it stabilises. |
| `valuable` as the transport for structured tracing fields | `valuable`, tracing-core | REJECT | Still behind `--cfg tracing_unstable` on `tracing` main; a library cannot require consumers' `RUSTFLAGS`. Spans carry primitive typed fields; structure lives in `Inspect`. |
| `serde::Serialize` as the description trait | serde, `insta` | REJECT as trait, ACCEPT as output | No level/unit/link/identity; render objects are not serializable. The owned snapshot derives `Serialize` for JSON, `insta` redactions and the protocol. |
| Derive covering enums and tuple structs; compile error for unsupported field type | current derive (limitation), `bevy_reflect` derive | ACCEPT | Removes the "named struct only" gap; the compile error is "rules as types". |
| Node identity assigned by the tree, typed `Link`s between trees | Flutter `locationId`, GPUI `InspectorElementId`, `design.md:288` | ACCEPT | "Identity is not a label": tests stop finding nodes by type-name string; driver joins by generational id. |
| Test hooks by string key | GPUI `debug_bounds("name")` | REJECT | Replaced by typed `find_by_link`/`find_by_kind().unique()`. |
| Structural diff with unit-derived tolerance | new; Android ATF-style typed results in market survey | ACCEPT | Needed for trace diff and geometry goldens; only possible because metadata is typed. |
| One target scheme `flui.<subsystem>` | existing `flui.frame`, OTel dotted namespaces | ACCEPT WITH CHANGE | Two live spellings already; rename `flui::reconcile` and `flui_animation` with a one-release alias per FR-035, after testing `EnvFilter` prefix behaviour. |
| Match phase spans by `(target, name)` | ADR-0202 hardening of frame/animation | ACCEPT | Closes the name-only match in `frame_timing_layer.rs:113-142`. |
| Frame, phase and `semantics`/`commit`/`submit` spans | Iced `iced_debug` (update/view/layout/draw), Masonry pass spans | ACCEPT | Fills the missing semantics/commit/submit phases; same span stream feeds profiler and driver. |
| Per-widget spans at TRACE | Masonry `make_trace_span` | ACCEPT WITH CHANGE | Only at relayout-boundary and rebuild roots; per-`layout_child` spans scale with tree size. |
| Element id on spans as `u64`, view kind as `&'static str` | Masonry `id.trace()`; ADR-0040 point 1 | ACCEPT | Replaces `element = ?id`; enables joins without parsing. |
| `element_rebuild` at INFO | current repo | REJECT (move to TRACE) | Per element per rebuild; default-filtered runs would pay for it. |
| Spans created but never entered | current gesture recognizers | REJECT (fix) | They produce no duration and nest nothing; make them events or enter them. |
| `tracing::enabled!` gating and allocation-free disabled path | `tracing` docs, `frame.rs:1122` | ACCEPT | Written as cost rules; reviewed by the "Recurring defects" mindset. |
| Compile-time `max_level_*`/`release_max_level_*` in framework crates | `tracing` features | REJECT | Would remove release profiling and the driver's `perf`; tokio-console documents the same hazard. Applications may opt in. |
| Dedicated profiling API next to tracing | `puffin` `profile_scope!`, egui `profiling` | REJECT | One instrumentation API; tracing spans already exist at every phase. Users can add `puffin`-style sinks as layers if wanted. |
| Emit through tracing, aggregate in a purpose-built layer, expose a separate protocol | `tokio-console` | ACCEPT | Matches ADR-0202 and the driver: `CaptureLayer` + `flui-protocol`; any front end (CLI, inspector, MCP) consumes the same stream. |
| Optional Tracy/Chrome layers | `tracing-tracy`, `tracing-chrome` | ACCEPT WITH CHANGE | Documented as user-added layers composed with `flui-log`'s builder. FLUI keeps its own exporter for session bundles: `tracing-chrome` writes on guard drop and its field handling is **[unverified]**. Tracy must be opt-in (network exposure). |
| Inter-event histograms | `tracing-timing` | REJECT | FLUI needs per-phase durations and worst-frame attribution; spans give both. |
| Chrome/Perfetto export with `X` events, counters, flows | Perfetto docs | ACCEPT WITH CHANGE | Replace `B`/`E` pairs with `X` (nest-safe); add counters for dirty counts; protobuf TrackEvent only if JSON becomes limiting. |
| Two recorders (`FrameTimingLayer` and `Timeline`) | current repo | REJECT | One capture of spans; `Timeline` becomes a view/exporter over it. |
| `#[track_caller]` creation sites, debug-only ZST | GPUI inspector, `flui-animation::CreationSite` | ACCEPT | Rust-native, zero-sized in release, already proven in-repo. Reach is limited to calls, so constructors/builders are the entry point. |
| Compiler-emitted source strings | Compose `sourceInformation` | REJECT | Needs a compiler plugin; opaque `String` payload; against "types over conventions". |
| Kernel transform adding a location parameter to every widget constructor | Flutter `--track-widget-creation` | REJECT (not available) | No equivalent in rustc; a markup macro would substitute but contradicts the authoring stance. |
| Rebuild-reason attribution | SwiftUI `_printChanges`, Compose recomposition counts | ACCEPT | Already half-built (`RebuildReasons`, ADR-0040); surface `reasons` on spans and in `Inspect` links; add skip/rebuild counters in the profile. |
| Owner stack vs component stack in error reports | React | ACCEPT WITH CHANGE | `TreeContext` carries ancestor path plus creation site, not a second stack object. |
| Error context accumulation by wrapping | `error-stack` | ACCEPT WITH CHANGE | Only the idea: `TreeError<E>` attached at the owning boundary; no new error crate in libraries (thiserror, AGENTS.md). |
| Labelled source spans in diagnostics | `miette` | REJECT | FLUI errors point into a tree, not text. Its `code`/`help`/`related` shape informs driver `Finding`s. |
| Context fields on every error variant | alternative | REJECT | Puts a `Vec` into hot-path error values; attach at the boundary. |
| Compact pinned geometry `Display`; `Debug` not used for output | repo gap, Rust `Debug` docs | ACCEPT | Output stops depending on `Debug` formatting. |
| Role-based styling, colour only on a TTY | `miette` handlers | ACCEPT | Same renderer for tests, CLI and reports. |
| `--json` as the machine path; never parse the human view | repo (`agent_workflow.rs`) | ACCEPT | Removes the only test that depends on rendered text. |

---

# Could not verify

- **SwiftUI `_printChanges`**: Apple's documentation page returned 404; only third-party descriptions
  were found. Behaviour and caveats are second-hand.
- **React DevTools "why did this render"**: the README and `Component` reference did not mention it;
  `displayName` was not in the page read.
- **Slint inspector**: no current doc page for an element-tree inspector was found; only `slint-viewer`
  and older release notes.
- **Flutter `--track-widget-creation` mechanism**: the Dart framework side (`widget_inspector.dart`) was
  read; the kernel transform that adds the location was not.
- **`tracing-chrome` field/args handling and `tracing-tracy` field recording**: the pages read did not
  document them.
- **`EnvFilter` target matching** for `.`-separated targets versus `::`: not checked; the proposed
  renames require a test through the real filter.
- **Android Layout Inspector** recomposition and skip counts: the Studio page confirms the feature
  exists; the Compose-specific "debug your Compose UI" page was not read.
- **Perfetto async `b`/`e`** support: the imported-formats page does not list it.
- **The producer of `tests/snapshots/*.snap`** was inferred from content, not traced.
- **GitHub code search rate limit** was hit near the end of research; GPUI `debug_bounds` and the
  Masonry/egui/Iced findings are from code search results and direct file reads made before that.
  Remaining Masonry, GPUI and Iced details (for example, how Iced's `comet` consumes `iced_beacon`) were
  not read beyond what is cited.
- **Cost claims** in 3.2 and 3.3 (hidden argument, `Location` static per call site, ZST in release) are
  reasoned from the Rust Reference and the in-repo `CreationSite`; no benchmark was run.
- **Nothing was built or run.** No code in this document has been compiled; signatures are a proposal.
