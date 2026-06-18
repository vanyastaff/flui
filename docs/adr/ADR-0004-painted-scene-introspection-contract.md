# ADR-0004: Painted-scene introspection & serialization contract — a curated, schema-committed sibling IR with pluggable snapshot-strategy renderers (not a `DiagnosticsNode` unification, not raw `Layer` serde)

*Give the painted scene (layer tree + per-`PictureLayer` draw-command stream) its own **curated, faithfully-typed IR** that derives `Serialize` and commits a **versioned, language-agnostic JSON Schema** — rendered to any output format through **first-class pluggable snapshot-strategy witnesses** (text golden, inspector JSON, future AccessKit / pixel / SVG) — so one data model serves snapshot tests, a live devtools inspector, and debug output without coupling a frozen test oracle to an evolving wire format, and without re-deriving the contract from internal types. The existing `DiagnosticsNode` stays the render-object introspection IR; the painted-scene IR is its sibling, not its replacement.*

---

- **Status:** Superseded by [ADR-0005](ADR-0005-diagnosticable-everywhere-introspection.md) (2026-06-14, same day, before implementation) — the sibling-IR was reversed in favour of a uniform `Diagnosticable`-everywhere substrate. This record is kept because its source-verified refutation of the *naive* `DiagnosticsNode` unification (stringly `DiagnosticsProperty.value`, no `impl Diagnosticable for DrawCommand`) still holds and motivates ADR-0005's *evolved* form (typed-value `DiagnosticsProperty` + `impl Diagnosticable` on the painted types). See ADR-0005 §Context for why the decision flipped.
- **Date:** 2026-06-14
- **Deciders:** @vanyastaff
- **Scope:** new painted-scene IR types in `flui-painting` (`DrawCommandSummary`, `DrawKind`, `PaintSummary`) and `flui-layer` (`SceneSnapshot`, `LayerSummary`, `LayerDescriptor`); the committed JSON Schema artifact + CI gate; a pluggable `SnapshotStrategy<Scene, Format>` renderer layer; migration of the PR #213 hand-rolled text serializer (`crates/flui-rendering/src/testing/snapshot.rs`) onto the IR's `Display`/`.text` strategy. **Out of scope (deferred):** the AccessKit `TreeUpdate` live-inspector wire (roadmap B), pixel/SVG goldens, a frame-trace artifact, and any `flui-testing` umbrella crate.
- **Relates to:** the render-harness 2.0 layered roadmap (sub-project A shipped in PR #213, squash `30bb0411`); ADR-0003 (virtualization) — the lazy `SliverList` it shipped is one of the dogfood subjects of the painted-scene snapshot.

---

## Verdict

**Target architecture (one paragraph).** flui gains a **painted-scene introspection IR** that is a *curated, faithfully-typed* projection of the composited output: `SceneSnapshot { format_version, root: Option<LayerSummary> }`, where `LayerSummary { kind, descriptor: LayerDescriptor, commands: Vec<DrawCommandSummary>, children }` and `LayerDescriptor` projects each `Layer` variant to serde-safe fields (opaque GPU/platform handles become id-or-descriptor, never the raw resource). The IR holds **faithful** values (full-precision `f32`, `Color`, `Rect`) — **normalization is not baked into the data**; it lives in the renderer. Output is produced through **pluggable first-class snapshot-strategy witnesses** (`SnapshotStrategy<Scene, Format>`, after PointFree `swift-snapshot-testing`): the built-in `.text` strategy reproduces the PR #213 insta golden **byte-for-byte** (2-decimal floats, identity-transform omitted, `#RRGGBBAA`, clip-behaviour names), the built-in `.json` strategy emits the **faithful** serde form for a devtools inspector, and future `.accesskit` / `.pixel` / `.svg` strategies are *additive* — they wrap the IR and never force a change to it. The contract committed for the long term is not the Rust types alone but a **`schemars`-generated, versioned JSON Schema document checked into the repo and CI-gated** (a Python/TS devtools client consumes the schema, not hand-written types). The existing `DiagnosticsNode` (`flui-foundation`) remains the **render-object** introspection IR; the painted-scene IR is its **sibling** — the two model different trees (render input vs paint output) and must not be welded together.

**Why this is decided now, and recorded durably.** flui is a Flutter→Rust framework built to *beat* Flutter, and breaking changes are allowed **now** while the contracts are not yet locked. PR #213 shipped a hand-rolled text serializer as a deliberate first step; the question this ADR settles is whether to (a) unify it onto the existing `DiagnosticsNode`, (b) expose raw `serde` on the real `Layer`/`DrawCommand` types, or (c) give the painted scene its own curated IR. A two-wave agent planning session (in-repo substrate audit, cross-ecosystem market research, an adversarial critic, and an API-contract design pass) **refuted (a) and (b) at source** and surfaced the 2026 best-in-class shape — the pluggable-strategy + schema-first contract. The load-bearing commitments — **(1)** the painted-scene IR is *curated and faithfully-typed* (normalization in the renderer, not the data); **(2)** the committed contract is a *versioned JSON Schema*, not the internal types; **(3)** renderers are *pluggable strategies*, so new output formats are additive forever — are exactly the commitments that keep the future-proof, market-best abstraction reachable. They are recorded here so a future zero-memory session cannot accidentally re-couple the test oracle to the inspector wire (Flutter's documented layer-tree regret) or settle for a stringly-typed property-bag projection.

---

## Context

### Current state of the code (verified by the planning session, 2026-06-14)

- **`DiagnosticsNode` / `Diagnosticable` / `DiagnosticsProperty`** exist in `crates/flui-foundation/src/debug.rs` (≈318 / 582 / 1020), derive `serde` behind a feature, are populated via `debug_fill_properties`, and are implemented on **49+ `RenderObject` types** plus `Layer` (`crates/flui-layer/src/layer/mod.rs:395`) and `DisplayList`. This is a real, production-ready **render-object** introspection substrate.
- **But `DiagnosticsNode` cannot represent the painted command stream.** There is **no `impl Diagnosticable for DrawCommand`** anywhere in the workspace; `DisplayList::debug_fill_properties` (`display_list/mod.rs:79`) emits exactly `commands: <len>` + `bounds`; `Layer::debug_fill_properties` for a `Picture` emits only `commands: <len>`. `DiagnosticsProperty.value` is a `String` built via `to_string()` — today holding Rust `Debug` strings, not normalized geometry. The IR provably discards the exact data the PR #213 snapshots assert on (per-command paint + `xf=[…]` transforms).
- **The PR #213 text serializer** (`crates/flui-rendering/src/testing/snapshot.rs`) is a separate, deliberately-normalized walk: `DrawCommandSummary { kind: DrawKind, line: String }`, with float→2-dec, identity-transform omitted, `#RRGGBBAA`, clip-behaviour names, deterministic order. Six committed `.snap` files define its format.
- **`DrawCommand` / `DisplayList` derive `serde`** (behind a feature); **`Layer` does not**. Whether `Layer` *can* derive faithfully is contested (the substrate scout flagged GPU/handle blockers; the API designer found `TextureId`/`PlatformViewId` are `i64`, `LayerLink` is `u64`, `Shader` is a value enum) — **this conflict is moot** under the decision below, because the curated IR projects `Layer` and `Layer` itself never derives `serde`.
- **The two introspection surfaces already exist, intentionally separate**, in `crates/flui-rendering/src/testing/inspect.rs`: `diagnostics()` (render-object tree, via `DiagnosticsNode`) and `snapshot()` (painted layer/command tree, via the #213 serializer). They describe different trees.
- **Four crates carry `pub mod testing`** (`flui-painting`, `flui-layer`, `flui-interaction`, `flui-rendering`); there is **no umbrella crate**, and `flui-rendering::testing` already consumes `flui-layer::testing::inspect` internally.

### Competitive landscape (what to beat, borrow, ignore)

Cross-ecosystem market research (2026-06-14; primary sources in References), read adversarially:

| System | Introspection model | One-source-many-sinks? | Contract versioning | Verdict for flui |
|---|---|---|---|---|
| **Flutter `DiagnosticsNode`** | curated typed IR → `toStringDeep` (text) + `toJsonMap` (DevTools) | yes, for widget/render trees | **unversioned JSON**, broke across SDK majors | borrow the *curated-IR* shape; **beat** the unversioned wire and the *layer-tree gap* (#46992 — never extended to the layer tree → text and inspector diverged) |
| **AccessKit `Node` + `TreeUpdate`** | typed, serde-native, push-diff with stable `NodeId`; builder vs read-only node split | yes (screen readers, kittest, future inspectors) | schema broke once on a memory rewrite (honest) | **adopt** as the *live-inspector wire* (roadmap B); it is the Rust-ecosystem standard (egui/Masonry/Slint) |
| **PointFree `swift-snapshot-testing`** | `Snapshotting<Value, Format>` — strategies as first-class composable **values** (`.image`/`.json`/`.dump`/`.recursiveDescription`), custom via `contramap` | **yes — the paradigm** | per-strategy file, no schema version field | **adopt the witness pattern** for the renderer layer; add the schema-version discipline it lacks |
| **Pydantic v2** | schema-first; **serialization context**; JSON Schema generation; validation-schema ≠ serialization-schema | yes (multiple `model_dump` modes from one type) | JSON Schema document + explicit modes | **borrow** serialization-context (thread `output_mode`/`format_version`) and *schema-as-committed-contract* |
| **schemars 1.x (Rust)** | derives JSON Schema from the same serde types | n/a | **generator output not semver-stable** — must snapshot it | **adopt**, but own the gate: commit + CI-diff the generated schema |
| **Jetpack Compose** | semantics tree as dual test+inspector substrate; pixel image separate | yes | Layout Inspector protobuf | confirms *structural-primary, pixel-separate* |
| **egui `kittest`** | AccessKit query layer + opt-in wgpu pixel snapshot | two layers | — | confirms practitioners *prefer structural insta over pixel* |
| **Jupyter `_repr_mimebundle_`** | one object → `{mime: repr}` dict, frontend negotiates | yes, open-ended | **none** | **reject** as a contract — no versioning/discovery, artifact bloat |
| **Textual SVG snapshot** | render UI to SVG, visual diff | — | — | **reject** — pixel-diff in SVG clothing, not structural |
| **Playwright trace `.zip`** | replayable DOM+actions+network archive | — | unversioned | **reject** as a golden — 50–200 MB, non-deterministic, debug-only |
| **Bevy `bevy_reflect` + RON scene** | raw reflection output as the wire format | yes | **broke every major** (#4561/#13041) | **reject** — coupling wire to internal types is the ossification trap |

What to **borrow / beat / ignore**: borrow Flutter's curated-IR shape and *beat* its unversioned wire + layer-tree gap; adopt swift-snapshot-testing's **pluggable-strategy witness** (the genuine paradigm beyond hard-coded `Display`+`Serialize`); borrow Pydantic's **serialization context** + **schema-as-contract** and realise them with `schemars` + a committed, CI-gated schema document; adopt **AccessKit** as the live-inspector wire (deferred to roadmap B, IR designed compatible); reject MIME-bundle, SVG-golden, trace-zip, and raw-reflection-as-wire as either hype or the ossification trap.

---

## Decision

Five decisions. D1 (the IR) and D2 (the schema) are the load-bearing, decide-now contract; D3 (strategies) is the renderer architecture; D4 (AccessKit) is deferred; D5 (topology) prevents an over-build.

### Decision 1 — A curated, faithfully-typed *sibling* IR, not a `DiagnosticsNode` unification, not raw `Layer` serde

The painted scene gets its own IR: `DrawCommandSummary` / `DrawKind` / `PaintSummary` (in `flui-painting`) and `SceneSnapshot` / `LayerSummary` / `LayerDescriptor` (in `flui-layer`). It is **curated** (only the fields a test or inspector should see, not a mechanical dump of internal structs) and **faithfully-typed** (full-precision `f32`, `Color`, `Rect` — *not* a pre-formatted `String`, and *not* a stringly-typed property bag). `DiagnosticsNode` is **untouched** in role — it remains the render-object introspection IR. The two are siblings because they model different trees (render input vs paint output) with different lifetimes (a `.snap` is a freely-regenerated tripwire; an inspector schema is a compatibility surface).

### Decision 2 — The committed contract is a versioned JSON Schema, not the Rust types

`SceneSnapshot` carries a `format_version: u32` (starts at `1`). All IR types are `#[non_exhaustive]`; the pre-existing `DrawKind`, `DiagnosticsNode`, and `DiagnosticsProperty` gaps in `#[non_exhaustive]` are closed in the same pass. A `schemars`-generated JSON Schema (Draft 2020-12) is **checked into the repo** (e.g. `schema/scene-snapshot.v1.json`) and **CI-gated**: an unreviewed diff to the generated schema fails the build (because `schemars` output is not itself semver-stable, *we* own the gate). Additive evolution rules: new variant / new `#[non_exhaustive]` field = minor; any change to the `.text` golden format or a field type = major + `format_version` bump. Multi-consumer shaping (test-curated vs inspector-faithful) uses a **serialization-context `output_mode`** threaded through the root serializer, not per-field `skip_if` sprawl.

### Decision 3 — Renderers are pluggable snapshot-strategy witnesses

Output formats are first-class composable `SnapshotStrategy<Scene, Format>` values (after `swift-snapshot-testing`), not hard-coded trait impls. Built-ins ship now: `.text` (reproduces the PR #213 golden byte-for-byte — normalization lives **here**, in the renderer, over the faithful IR) and `.json` (faithful serde, the inspector form, and the **synthetic first consumer** that de-risks the serde path via its own snapshot test). `.accesskit`, `.pixel`, and `.svg` are additive future strategies that wrap the IR without changing it. This is the layer that makes "one IR, many lossless projections" true and keeps the rendering side open for a decade.

### Decision 4 — The AccessKit live-inspector wire is deferred to roadmap B

The painted-scene IR is designed to be *compatible* with an AccessKit `TreeUpdate` push-diff emission (stable ids, builder/read-only split), but the wire itself is **not built here** — it lands with roadmap B (semantics / AccessKit), where it doubles as the accessibility tree. This keeps the present change scoped to the data contract + renderers, while not foreclosing the proven inspector wire.

### Decision 5 — IR lives in the producer crates; no `flui-testing` umbrella

`DrawCommandSummary` / `DrawKind` move from `flui-rendering::testing` into `flui-painting` as **non-test** types (the inspection IR is used by devtools/dev builds, not only tests); `SceneSnapshot` / `LayerSummary` / `LayerDescriptor` live in `flui-layer`. The **inspection contract** is producer-crate, feature-gated `serde`, always-available types; the **test harness** (`RenderTester`, `LayerTester`, …) stays per-crate behind `cfg(test)`/`feature = "testing"`. A `flui-testing` re-export umbrella is **rejected** (see below).

---

## Consequences

- **One data model, many sinks — without coupling lifetimes.** The frozen `.text` golden and the evolving `.json` inspector are *two strategies over one IR*, not two hand-rolled serializers and not one welded surface. A snapshot-format change and an inspector-wire change are independent edits.
- **A machine-checked, language-agnostic contract.** The committed JSON Schema lets a non-Rust devtools client (Python/TS) consume the contract directly; CI blocks silent drift.
- **PR #213 is preserved, not regressed.** The `.text` strategy must reproduce the six `.snap` files byte-for-byte (verified by `cargo insta`); `serialize_layer_tree` becomes a `#[deprecated]` shim for one cycle, then is removed.
- **New cost surface.** A curated rich IR is more types to maintain than a `line: String`; the `schemars` schema gate is new CI; the strategy layer is moderate Rust machinery (a struct of closures, not Swift's protocol sugar). Accepted because four output formats are already on the roadmap (text, json, accesskit, pixel) — the witness pattern pays for itself, and the schema gate is the price of a decade-stable contract.
- **`Layer` never derives `serde`.** The GPU/handle-serializability conflict is rendered moot; opaque resources are projected to id/descriptor in `LayerDescriptor` and the raw resource never escapes.

## Rejected alternatives

- **Unify the paint snapshot onto `DiagnosticsNode`.** Refuted at source: no `impl Diagnosticable for DrawCommand` exists; the `Layer`/`DisplayList` impls emit only `commands: <len>`, so the IR cannot hold a draw command or its transform — routing #213 through it regresses the just-shipped goldens (the lazy-sliver test's 9 distinct rects collapse to one `commands: 9` line). `DiagnosticsProperty.value: String` would also leak 2-decimal lossy rounding into a production inspector that wants full precision, and welding the frozen test oracle to the evolving devtools wire **re-creates Flutter's documented layer-tree regret**.
- **Raw `serde` on `Layer` / `DrawCommand` as the wire (Bevy model).** An internal field rename or variant reorder silently breaks every consumer; Bevy broke its scene format in every major. The whole point of a curated IR is to decouple the contract from internal representation.
- **Thin `DrawCommandSummary { kind, line: String }` IR.** Minimal work, but the `.json` output is then a stringly-typed `line` with 2-decimal lossy values baked in — the exact property-bag weakness that disqualifies `DiagnosticsNode`. The rich, faithfully-typed IR is the price of a real inspector contract.
- **Jupyter MIME-bundle (`_repr_mimebundle_`).** A display convenience, not a durable machine contract: no versioning, no capability discovery, artifact bloat at scale.
- **Textual SVG golden.** A pixel diff in SVG clothing; does not unify structural + visual and maps poorly onto a GPU-rendered tree.
- **Playwright-style replayable trace `.zip`.** 50–200 MB, non-deterministic timing, DOM-not-IR; even Playwright does not commit traces as regression baselines. A frame-trace is a possible *debug-only* devtools feature, never a test primitive.
- **`flui-testing` re-export umbrella.** YAGNI (no external consumer; workspace tests already `use flui_rendering::testing::{…}` directly) and a feature-unification footgun (forces painting+layer+interaction+semantics harnesses to compile as one bundle with no opt-out). The only real duplication — the float/colour/rect formatting helpers — is killed by hoisting them into a `flui-painting` util, not by an umbrella crate.

## References

### In-repo (verified during the planning session)

- `crates/flui-foundation/src/debug.rs` — `DiagnosticsNode` (≈582), `DiagnosticsProperty.value: String` (≈318), `DiagnosticsBuilder::add` (≈1103); `DiagnosticLevel` already `#[non_exhaustive]`, `DiagnosticsNode`/`DiagnosticsProperty` not.
- `crates/flui-painting/src/display_list/command.rs:42` (`DrawCommand`, derives serde), `…/mod.rs:79` (`DisplayList::debug_fill_properties` — 2 scalar props, no command iteration).
- `crates/flui-layer/src/layer/mod.rs:187` (`Layer`, no serde), `:395` (`Diagnosticable` impl — `commands: <len>` only).
- `crates/flui-rendering/src/testing/snapshot.rs` (PR #213 serializer + stability contract), `…/inspect.rs` (the two already-separate surfaces), `tests/snapshots/harness_snapshot__*.snap` (the committed `.text` format).

### External prior art / research citations (web research, 2026-06-14)

- Flutter `DiagnosticsNode` (foundation) + DevTools VM-service JSON; layer-tree gap [flutter#46992](https://github.com/flutter/flutter/issues/46992).
- AccessKit — [how-it-works](https://accesskit.dev/how-it-works/), [memory rewrite / schema break](https://accesskit.dev/dramatically-reducing-accesskits-memory-usage/); `kittest` / `egui_kittest` (structural insta preferred over pixel).
- PointFree [`swift-snapshot-testing`](https://github.com/pointfreeco/swift-snapshot-testing) — `Snapshotting<Value, Format>` witness pattern; [witness-oriented design](https://www.pointfree.co/episodes/ep39-witness-oriented-library-design).
- Pydantic v2 — [serialization (context, modes)](https://pydantic.dev/docs/validation/latest/concepts/serialization/), [JSON Schema](https://pydantic.dev/docs/validation/latest/concepts/json_schema/).
- [`schemars`](https://github.com/GREsau/schemars) (Draft 2020-12; output not semver-stable); [Rust and JSON Schema, A. Leventhal](https://ahl.dtrace.org/2024/01/22/rust-and-json-schema/) (commit types, generate+snapshot schema).
- [Playwright ARIA snapshots](https://playwright.dev/docs/aria-snapshots) (structural YAML golden) vs [trace viewer](https://playwright.dev/docs/trace-viewer) (debug artifact); [WebDriver BiDi](https://developer.chrome.com/blog/webdriver-bidi) (versioned schema discipline); Bevy scene format breaks [#4561](https://github.com/bevyengine/bevy/pull/4561), [#13041](https://github.com/bevyengine/bevy/issues/13041).

## Implementation

See the design spec `docs/plans/2026-06-14-scene-snapshot-contract-design.md` and the implementation plan `docs/plans/2026-06-14-scene-snapshot-contract-plan.md`. Sequencing: (1) close `#[non_exhaustive]` gaps + hoist formatting helpers into `flui-painting`; (2) move `DrawCommandSummary`/`DrawKind` to `flui-painting` and grow them rich; (3) `SceneSnapshot`/`LayerSummary`/`LayerDescriptor` in `flui-layer`; (4) `.text` strategy reproducing the six `.snap` byte-for-byte (gate: `cargo insta`); (5) `.json` strategy + its synthetic-inspector snapshot test; (6) `schemars` schema + CI diff-gate; (7) deprecate `serialize_layer_tree`. AccessKit wire = roadmap B.
