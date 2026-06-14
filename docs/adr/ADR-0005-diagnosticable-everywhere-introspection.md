# ADR-0005: One uniform introspection substrate — `Diagnosticable` everywhere with a typed-value `DiagnosticsProperty`, the painted scene included

*Make the **existing `Diagnosticable` / `DiagnosticsNode` system** the single, uniform introspection substrate for the whole framework — render objects, layers, **and the painted draw-command stream** — by **upgrading `DiagnosticsProperty.value` from `String` to a typed `DiagnosticsValue` sum** (the completion of the half-built `DiagnosticsPropertyKind` design, and the Rust realization of Flutter's generic `DiagnosticsProperty<T>`), implementing `Diagnosticable` on `DrawCommand` and having `Layer` describe its commands as child nodes. One `DiagnosticsNode` tree then feeds **all three** sinks from one source: the golden snapshot text (its `Display`), the devtools inspector (its faithful serde JSON), and debug dumps — with a committed, versioned JSON Schema as the contract. No parallel snapshot taxonomy. Supersedes the sibling-IR of ADR-0004.*

---

- **Status:** Accepted
- **Date:** 2026-06-14
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-foundation/src/debug.rs` (`DiagnosticsValue` sum + typed `DiagnosticsProperty` + typed `DiagnosticsBuilder` methods, back-compatible); `flui-painting` (`impl Diagnosticable for DrawCommand`, all 31 variants); `flui-layer` (`Layer::to_diagnostics_node` descends into the `DisplayList` as child nodes; opaque handles → id/descriptor properties); `flui-rendering::testing` (the painted-`LayerTree` → `DiagnosticsNode` snapshot; `SnapshotStrategy` over `DiagnosticsNode`; retire the PR #213 string serializer); the committed JSON Schema + CI gate. **Out of scope (deferred):** AccessKit `TreeUpdate` live wire (roadmap B), pixel/SVG/trace goldens.
- **Supersedes:** [ADR-0004](ADR-0004-painted-scene-introspection-contract.md) (sibling curated IR) — same day, before any implementation landed. Tasks 0-2 of the ADR-0004 plan (schemars dependency, `#[non_exhaustive]` on `DiagnosticsNode`/`DiagnosticsProperty`, hoisted text-format helpers) are **retained** — they are foundation for this decision too.
- **Relates to:** the render-harness 2.0 roadmap (sub-project A shipped in PR #213, squash `30bb0411`); roadmap B (semantics / AccessKit) will reuse this same `DiagnosticsNode` tree as the inspector's push-diff source.

---

## Verdict

**Target architecture (one paragraph).** flui already has a Flutter-style introspection system in `flui-foundation` — `Diagnosticable` (a trait with `debug_fill_properties` + an overridable `to_diagnostics_node`), `DiagnosticsNode` (a real tree: `name` + `properties: Vec<DiagnosticsProperty>` + `children: Vec<DiagnosticsNode>`, serde-derived, `#[non_exhaustive]`), and `DiagnosticsProperty` (which already carries a rich `DiagnosticsPropertyKind` discriminant — `Rect`/`Color`/`Double{unit}`/… — but stores its `value` as a `String`). This decision makes that system the **one** introspection substrate for everything, including the painted scene, by three moves: **(1)** upgrade `DiagnosticsProperty.value` from `String` to a typed `DiagnosticsValue` sum (`Null|Bool|Int|Float|Str|Color|Rect|Offset|Size|List|Nested`), keeping `DiagnosticsProperty::new(name, impl Display)` constructing `DiagnosticsValue::Str` so **all ~49 existing `Diagnosticable` impls compile unchanged**, and adding typed `DiagnosticsBuilder` methods (`add_f64`, `add_rect`, `add_color`, …) for the paint path; **(2)** `impl Diagnosticable for DrawCommand` (all 31 variants, typed properties); **(3)** `Layer::to_diagnostics_node` (already overridden) descends a `PictureLayer` into per-`DrawCommand` **child nodes**, projecting opaque GPU/platform handles to id/descriptor properties. The painted `LayerTree` then projects to a `DiagnosticsNode` tree that serves **all three** consumers: the golden snapshot is its `Display` (normalized text, via a compact tree style), the inspector is its **faithful** serde JSON (full-precision typed values), and debug dumps are the same. A committed, CI-gated `schemars` JSON Schema over `DiagnosticsNode`/`DiagnosticsValue` is the years-long, language-agnostic contract (`format_version` + `#[non_exhaustive]`). The parallel snapshot taxonomy proposed in ADR-0004 (`SceneSnapshot`/`LayerSummary`/`DrawCommandSummary`) is **not built**.

**Why this is decided now, and why it reverses ADR-0004.** flui is a Flutter→Rust framework built to *beat* Flutter; breaking changes are cheap **now** and the contracts are not yet locked. ADR-0004 chose a *decoupled sibling IR* for the painted scene — lower-risk, byte-stable goldens, but it introduces a **second** introspection taxonomy alongside the framework's existing `Diagnosticable` system, and a `DrawCommandSummary` enum that must mirror `DrawCommand` variant-for-variant forever. The decisive counter-argument (raised by the human decider, and correct): a UI framework should have **one** way to introspect its types, not two. The planning session's adversarial critic refuted *unifying onto the `DiagnosticsNode` system **as it stands*** — and that refutation is real and still recorded in ADR-0004 — but its two load-bearing objections are **artifacts of `DiagnosticsProperty` being stringly-typed and of `DrawCommand` not yet implementing `Diagnosticable`**, both of which this ADR fixes at the source. With a typed-value `DiagnosticsProperty` (which is what Flutter's generic `DiagnosticsProperty<T>` already is, and what flui's existing `DiagnosticsPropertyKind` was clearly built to grow into), the inspector JSON is faithful, the painted command stream is representable, and the uniformity win is unlocked. The load-bearing commitments — **(1)** introspection is *one* trait/node for the whole framework; **(2)** property values are *typed* (faithful JSON, normalization only in the text renderer); **(3)** the contract is a *versioned committed JSON Schema* over the node, not the internal types — are recorded here so a future session cannot regress to a stringly node or a second parallel taxonomy.

---

## Context

### What already exists (verified at source, 2026-06-14, `crates/flui-foundation/src/debug.rs`)

- **`Diagnosticable`** (≈1047): `fn debug_fill_properties(&self, &mut DiagnosticsBuilder)` + a default `fn to_diagnostics_node(&self) -> DiagnosticsNode` that names the node by type and fills *properties* (not children). Overridable — `Layer` already overrides it (`crates/flui-layer/src/layer/mod.rs:395`).
- **`DiagnosticsNode`** (≈602): `{ name: Option<String>, properties: Vec<DiagnosticsProperty>, children: Vec<DiagnosticsNode>, level, style }` — a real tree with `properties_mut`/`children_mut` and a `value_of(name)` structured-assert helper; serde-derived; `#[non_exhaustive]` (closed in ADR-0004 Task 1); `Display` = `format_deep` (the tree dump).
- **`DiagnosticsProperty`** (≈321): `{ name: String, value: String, level, kind: DiagnosticsPropertyKind, show_name, show_separator, default_value, tooltip }`; serde-derived; `#[non_exhaustive]`. `new(name, value: impl Display)` stringifies. **`value` is the only stringly part — and `kind` already discriminates `Rect`/`Color`/`Double{unit}`/`Int{unit}`/`Offset`/`Size`/`Enum`/`Flag`/`Iterable{count}`/…**, i.e. the design already *knows* a property's type but throws away the typed value.
- **`DiagnosticsBuilder`** (≈1081): `add(name, impl Display)`, `add_with_level`, `add_flag`, `add_optional`, … — all stringify.
- **Coverage:** ~49 `Diagnosticable` impls across render objects + `Layer` + `DisplayList`. **No `impl Diagnosticable for DrawCommand`**; `DisplayList`/`Layer` `debug_fill_properties` emit only `commands: <len>` — so the painted command stream is *not* in the tree today (the gap this ADR closes).

### Why the critic's refutation of ADR-0004's rejected "unify" option does not block this ADR

ADR-0004 §Rejected records, source-verified: unifying onto `DiagnosticsNode` fails because (a) `DiagnosticsProperty.value: String` would leak lossy text into the inspector, and (b) there is no `impl Diagnosticable for DrawCommand`, so the IR cannot hold the command stream. **Both are fixed here, not worked around:** (a) → `DiagnosticsValue` typed sum (faithful JSON; the text renderer normalizes, the data does not); (b) → `impl Diagnosticable for DrawCommand` + `Layer` child-descent. The critic evaluated the *naive* unification (the system unchanged); this ADR adopts the *evolved* one. The third concern — coupling the golden test oracle to the inspector wire — is **accepted as a deliberate tradeoff**: it is exactly Flutter's shipped architecture for the widget/render trees (`DiagnosticsNode` powers both `toStringDeep` and the DevTools JSON), it is versioned (`format_version`), and the uniformity it buys is the explicit goal. (Flutter's documented regret, issue #46992, was *not* extending `DiagnosticsNode` to the **layer** tree — this ADR extends it to the layer **and** command tree, i.e. it does the thing Flutter regretted *not* doing.)

### Market grounding (unchanged from the planning session)

The cross-ecosystem research already concluded that a **curated typed introspection node** (Flutter `DiagnosticsNode`, AccessKit `Node`) is the decade-stable pattern, and that the durable upgrades are *typed property values* (Pydantic-style faithful serialization), a *committed JSON Schema* (`schemars`, snapshotted + CI-gated — the generator's output is not itself semver-stable, so we own the gate), and *pluggable render strategies* (the swift-snapshot-testing witness pattern). ADR-0004 applied these to a bespoke sibling IR; ADR-0005 applies the **same** upgrades to the framework's existing `DiagnosticsNode`, which is the market-proven node itself.

---

## Decision

### Decision 1 — Typed `DiagnosticsValue`, back-compatible

Introduce in `flui-foundation`:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DiagnosticsValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Color { r: u8, g: u8, b: u8, a: u8 },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Offset { x: f64, y: f64 },
    Size { w: f64, h: f64 },
    List(Vec<DiagnosticsValue>),
    Nested(Vec<DiagnosticsProperty>),
}
```

`DiagnosticsProperty.value` becomes `DiagnosticsValue`. **Back-compat is mandatory and load-bearing:** `DiagnosticsProperty::new(name, value: impl Display)` constructs `DiagnosticsValue::Str(value.to_string())`, and `DiagnosticsProperty::value()` returns the *display string* (via `DiagnosticsValue`'s `Display`), so every existing caller and the existing doctests (`assert_eq!(prop.value(), "100")`) keep working unchanged. New typed constructors + `DiagnosticsBuilder` methods (`add_f64`, `add_rect`, `add_color`, `add_typed`) feed faithful values. The `DiagnosticsPropertyKind` discriminant stays (it drives text display); `DiagnosticsValue` carries the data. serde serializes the typed value → the inspector JSON is faithful (full precision); text rendering applies normalization (2-dec, `#RRGGBBAA`) at the `Display` boundary only.

### Decision 2 — `impl Diagnosticable` on the painted types

`impl Diagnosticable for DrawCommand` in `flui-painting` (all 31 variants; `debug_fill_properties` pushes typed properties — `Rect`, `Color`, command-specific scalars; the recording transform omitted-when-identity in the text renderer, present in JSON). `Layer::to_diagnostics_node` (already overridden in `flui-layer`) is extended so a `PictureLayer` contributes its `DisplayList` commands as **child** `DiagnosticsNode`s (each via `DrawCommand::to_diagnostics_node`), and opaque GPU/platform handles (`TextureId`, `PlatformViewId`, `LayerLink`) project to id/descriptor properties — the raw resource never enters the tree.

### Decision 3 — One tree, three sinks, via pluggable strategies

The painted `LayerTree` projects to a `DiagnosticsNode` tree (`fn scene_diagnostics(&LayerTree) -> DiagnosticsNode` in `flui-rendering::testing`, or on the layer tree itself). `SnapshotStrategy<DiagnosticsNode, Format>` witnesses render it: `.text` (the node's `Display` / compact tree style — the golden), `.json` (faithful `serde_json` — the inspector and the synthetic first consumer that de-risks the contract), with `.accesskit`/`.pixel` additive later. The PR #213 string serializer (`crates/flui-rendering/src/testing/snapshot.rs` `summarize_command`/`serialize_layer_tree`) is **retired**; `serialize_layer_tree` becomes a `#[deprecated]` shim over the new path for one cycle. The six committed `.snap` goldens are **regenerated** to the `DiagnosticsNode` text format (allowed — breaking is cheap now), and a new `.json` snapshot is added; the regenerated goldens must still assert the same discriminating facts (clip behavior, per-command paint, the lazy-sliver transforms, the decorated-box shadow/fill/border) — verified by review against the old goldens.

### Decision 4 — Committed JSON Schema contract

`DiagnosticsNode` + `DiagnosticsProperty` + `DiagnosticsValue` derive `schemars::JsonSchema` (feature-gated). A root introspection envelope carries `format_version: u32` (start `1`). The generated Draft-2020-12 schema is checked in (`schema/diagnostics.v1.json`) and CI-gated on an unreviewed diff. This is the framework-wide introspection contract, not a paint-only one.

### Decision 5 — Retire the sibling IR; keep the foundation

`SceneSnapshot`/`LayerSummary`/`LayerDescriptor`/`DrawCommandSummary` (ADR-0004) are **not built**. The completed ADR-0004 foundation tasks are kept: the `schemars` dependency (Task 0), `#[non_exhaustive]` on `DiagnosticsNode`/`DiagnosticsProperty` (Task 1), and the hoisted text-format helpers in `flui-painting` (Task 2 — reused by the `DrawCommand` `Diagnosticable` text path).

---

## Consequences

- **One introspection system.** Render objects, layers, and draw commands all self-describe through `Diagnosticable` into one `DiagnosticsNode` tree. New types join by implementing one trait — no parallel taxonomy to keep in sync. This is the uniformity the decision is for.
- **The typed-value upgrade improves the whole framework, not just paint.** All ~49 existing render-object diagnostics become faithfully serializable for the inspector for free; the half-built `DiagnosticsPropertyKind` design is completed.
- **One tree feeds golden text + inspector JSON + debug** — the "serialize once, render many" goal, realized on the framework's own node.
- **Costs, stated honestly:** (a) the six `.snap` goldens are **regenerated** (the byte-stable #213 format is replaced by the `DiagnosticsNode` text format) — content equivalence is verified by review, not by byte-identity; (b) `DiagnosticsProperty.value: String → DiagnosticsValue` is a `flui-foundation` change whose serde representation shifts (no consumer yet, so safe) and which touches the value accessor + doctests (mitigated by the back-compat `Str`/`Display` path); (c) the golden test oracle is now **coupled** to the framework introspection contract (accepted; versioned; Flutter-validated).
- **The witness/strategy and schema/versioning machinery from ADR-0004 survive**, now over `DiagnosticsNode`.

## Rejected alternatives

- **The sibling curated IR (ADR-0004).** Decoupled and byte-stable, but introduces a *second* introspection taxonomy and a `DrawCommandSummary` that mirrors `DrawCommand` forever. Reversed in favour of one uniform system; the human decider's "one way to introspect, not two" is the deciding argument, and breaking is cheap now.
- **Naive unification onto the *current* `DiagnosticsNode`** (stringly `value`, no `DrawCommand` impl). Still rejected — it is what the critic refuted. This ADR adopts the *evolved* form (typed value + painted `Diagnosticable` impls), not the naive one.
- **Raw `serde` on `Layer`/`DrawCommand` as the wire (Bevy model).** Still rejected — couples the contract to internal types; Bevy broke its scene format every major. The `DiagnosticsNode` projection is the curated decoupling layer.
- **Keeping `DiagnosticsProperty.value` as `String` and post-hoc parsing for the inspector.** Rejected — stringly JSON is the exact weakness; the inspector must get typed values from the node.

## References

- In-repo (verified): `crates/flui-foundation/src/debug.rs` — `DiagnosticsValue` target / `DiagnosticsProperty` (≈321, `value: String`, `kind` discriminant ≈258), `DiagnosticsNode` (≈602, tree + `value_of`), `Diagnosticable` (≈1047), `DiagnosticsBuilder` (≈1081); `crates/flui-layer/src/layer/mod.rs:395` (`Layer::to_diagnostics_node` override); `crates/flui-painting/src/display_list/command.rs:44` (`DrawCommand`, 31 variants); `crates/flui-rendering/tests/snapshots/harness_snapshot__*.snap` (goldens to regenerate); ADR-0004 (the superseded sibling-IR + its source-verified critic findings).
- External: Flutter `DiagnosticsNode`/`DiagnosticsProperty<T>` + `toJsonMap` + DevTools; layer-tree gap [flutter#46992](https://github.com/flutter/flutter/issues/46992); Pydantic v2 typed serialization; [`schemars`](https://github.com/GREsau/schemars) + [commit-and-snapshot-the-schema](https://ahl.dtrace.org/2024/01/22/rust-and-json-schema/); PointFree [`swift-snapshot-testing`](https://github.com/pointfreeco/swift-snapshot-testing) witness strategies; AccessKit (the roadmap-B inspector wire that will reuse this node tree).

## Implementation

See the rewritten design spec `docs/plans/2026-06-14-scene-snapshot-contract-design.md` and plan `docs/plans/2026-06-14-scene-snapshot-contract-plan.md`. Sequencing: foundation Tasks 0-2 (done) stay; then (3) `DiagnosticsValue` + typed `DiagnosticsProperty`/`DiagnosticsBuilder`, back-compat green across the workspace; (4) `impl Diagnosticable for DrawCommand`; (5) `Layer` command-descent + opaque-handle projection; (6) painted-`LayerTree` → `DiagnosticsNode` snapshot + `SnapshotStrategy` `.text`/`.json`, regenerate goldens, retire `serialize_layer_tree`; (7) `schemars` schema + CI gate; (8) final gates. AccessKit wire = roadmap B.
