# Painted-Scene Introspection (Diagnosticable-everywhere) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make flui's existing `Diagnosticable`/`DiagnosticsNode` system the one introspection substrate for everything incl. the painted scene — typed `DiagnosticsValue`, `impl Diagnosticable for DrawCommand`, `Layer` command-descent, one tree → golden text + faithful inspector JSON + committed JSON Schema.

**Architecture:** ADR-0005. Upgrade `DiagnosticsProperty.value: String → DiagnosticsValue` (back-compatible); implement `Diagnosticable` on `DrawCommand`; `Layer::to_diagnostics_node` descends into commands; render the painted `DiagnosticsNode` tree via `SnapshotStrategy` (`.text` golden, `.json` inspector); commit a `schemars` schema.

**Tech Stack:** Rust 2024, `serde`, `schemars` (vetted, commit `4c47bf2b`), `insta`.

**Status of foundation (ADR-0004 Tasks 0-2, RETAINED — already committed, do not redo):**
- `4c47bf2b` schemars workspace dep + flui-painting/flui-layer features.
- `239853b7` `#[non_exhaustive]` on `DiagnosticsNode`/`DiagnosticsProperty`.
- `5f7bf9a2` hoisted text-format helpers into `flui-painting::display_list::summary::fmt` (`f`, `hex_color`, `fmt_rect`, …) — reused by `DiagnosticsValue::Display`.

**Authoritative in-repo sources (READ before implementing):**
- `crates/flui-foundation/src/debug.rs` — `DiagnosticsValue` target; `DiagnosticsProperty` (≈321, `value: String`, `new(name, impl Display)` ≈359, `value()` accessor, `kind: DiagnosticsPropertyKind` ≈258); `DiagnosticsNode` (≈602, tree + `value_of`); `Diagnosticable` (≈1047, `to_diagnostics_node`/`debug_fill_properties`); `DiagnosticsBuilder` (≈1081, `add`/`add_flag`/`add_optional`).
- `crates/flui-rendering/src/testing/snapshot.rs` — `summarize_command` (31 arms) = the curated per-command field-set to port into `DrawCommand::debug_fill_properties`; `write_layer` (19 arms) = the per-layer header fields; `serialize_layer_tree` = the walk to retire.
- `crates/flui-painting/src/display_list/command.rs:44` — `DrawCommand` (31 variants + fields).
- `crates/flui-layer/src/layer/mod.rs:395` — the existing `Layer::to_diagnostics_node` override to extend.
- `crates/flui-rendering/tests/snapshots/harness_snapshot__*.snap` — the six goldens to regenerate (content must stay equivalent: clip behavior, paint, transforms, shadow/fill/border, opacity).

**Scope:** ADR-0005 Decisions 1-4. Out: AccessKit wire (roadmap B), pixel/SVG/trace, `flui-testing` umbrella.

---

## Task 3: Typed `DiagnosticsValue` + back-compatible `DiagnosticsProperty`/`DiagnosticsBuilder`

The load-bearing foundation upgrade. **Back-compat is the hard requirement:** the ~49 existing `Diagnosticable` impls and the `debug.rs` doctests must compile and pass unchanged.

**Files:** Modify `crates/flui-foundation/src/debug.rs`; `crates/flui-foundation/Cargo.toml` (add `schemars` optional dep + feature, like flui-painting did in Task 0).

- [ ] **Step 1: Wire schemars into flui-foundation**

`Cargo.toml` `[dependencies]`: `schemars = { workspace = true, optional = true }`. `[features]`: add `schemars = ["dep:schemars", "serde"]` (and forward to any flui-* dep with a schemars feature it carries — verify its deps first). Confirm `serde` feature exists; if not, add it mirroring another crate.

- [ ] **Step 2: Write the failing back-compat + typed tests**

In `debug.rs` tests:

```rust
#[test]
fn string_property_back_compat() {
    let p = DiagnosticsProperty::new("width", 100);
    assert_eq!(p.value(), "100");            // accessor still returns the display string
    assert_eq!(p.to_string(), "width: 100"); // existing format unchanged
}
#[test]
fn typed_rect_value_is_structured() {
    let mut b = DiagnosticsBuilder::new();
    b.add_rect("bounds", 0.0, 0.0, 40.0, 40.0);
    let props = b.build();
    assert!(matches!(props[0].value_typed(), DiagnosticsValue::Rect { w, .. } if *w == 40.0));
}
```

(Use the real accessor names; if `value_typed()` is the chosen getter, define it; adapt the test to the final API.)

Run: `cargo test -p flui-foundation --features serde debug::` → FAIL (`DiagnosticsValue`/`add_rect` undefined).

- [ ] **Step 3: Define `DiagnosticsValue` + retype the property**

Add `DiagnosticsValue` (ADR-0005 Decision 1 shape: `Null|Bool|Int|Float|Str|Color|Rect|Offset|Size|List|Nested`) with `#[non_exhaustive]` + `Debug,Clone,PartialEq` + `#[cfg_attr(feature="serde", derive(Serialize,Deserialize))]` + `#[cfg_attr(feature="schemars", derive(JsonSchema))]` + `impl Display` (Float→2-dec via the hoisted `flui_painting::display_list::summary::fmt::f` IF a dep edge exists; else a local 2-dec formatter — flui-foundation must NOT depend on flui-painting if that would invert layering, so prefer a local `fmt` in foundation) + `From<&str>/From<String>/From<f64>/From<i64>/From<bool>`.

Change `DiagnosticsProperty.value` to `DiagnosticsValue`. Keep `new(name, value: impl Display)` → `DiagnosticsValue::Str(value.to_string())`. Keep `value(&self) -> String` returning `self.value.to_string()` (Display). Add `value_typed(&self) -> &DiagnosticsValue`. Update `value_of` on `DiagnosticsNode` if it returns the string (keep returning the display string for back-compat).

> Layering note: `flui-foundation` is low in the stack and likely must NOT depend on `flui-painting`. So the 2-dec/`#RRGGBBAA` formatting for `DiagnosticsValue::Display` lives in `flui-foundation` (a tiny local helper), NOT the hoisted flui-painting one. The hoisted flui-painting helpers stay for the `DrawCommand` impl's own use (Task 4). Verify the dep direction before importing.

- [ ] **Step 4: Add typed `DiagnosticsBuilder` methods**

`add_f64(name, f64)`, `add_int(name, i64)`, `add_bool(name, bool)`, `add_color(name, r,g,b,a: u8)`, `add_rect(name, x,y,w,h: f64)`, `add_offset`, `add_size`, `add_typed(name, DiagnosticsValue)` — each sets the matching `DiagnosticsPropertyKind`. Keep `add(name, impl Display)` (→ `Str`, Generic kind) unchanged.

- [ ] **Step 5: Verify back-compat across the workspace**

Run: `cargo build --workspace` (the ~49 impls must compile unchanged) AND `cargo test -p flui-foundation --features serde debug::` (PASS, incl. the back-compat + typed tests) AND `cargo test -p flui-foundation --doc` (the doctests pass) AND `cargo check -p flui-foundation --features schemars` (derives compile).

- [ ] **Step 6: Commit**

```bash
git add crates/flui-foundation/Cargo.toml crates/flui-foundation/src/debug.rs
git commit -m "feat(foundation): typed DiagnosticsValue (faithful inspector JSON) — back-compatible String path preserved"
```

---

## Task 4: `impl Diagnosticable for DrawCommand`

**Files:** Modify `crates/flui-painting/src/display_list/` (a new `diagnostics.rs` module, or extend `summary.rs`); `command.rs`/`mod.rs` for the `impl`. Add `flui-foundation` dep if not present (flui-painting → flui-foundation should already exist; verify).

- [ ] **Step 1: Write the failing per-command property tests**

```rust
#[test]
fn draw_rect_diagnostics_has_typed_rect_and_color() {
    let cmd = /* DrawCommand::DrawRect { rect 0,0,40,40; paint fill red; identity } */;
    let node = cmd.to_diagnostics_node();
    assert_eq!(node.name(), Some("DrawRect"));
    assert!(matches!(node.value_of_typed("rect"), Some(DiagnosticsValue::Rect { w, .. }) if *w == 40.0));
    assert!(matches!(node.value_of_typed("color"), Some(DiagnosticsValue::Color { r: 255, .. })));
}
```

Run → FAIL (no `Diagnosticable for DrawCommand`).

- [ ] **Step 2: Implement `Diagnosticable for DrawCommand`**

`impl Diagnosticable for DrawCommand { fn debug_fill_properties(&self, b: &mut DiagnosticsBuilder) { match self { … } } }` — one arm per the 31 variants, pushing TYPED properties that mirror the curated fields the existing `summarize_command` arm exposes (read each arm in snapshot.rs). Override `to_diagnostics_node` so the node `name` is the command kind string (`"DrawRect"`, `"ClipRect"`, …) rather than the Rust type name. Transform: push as `DiagnosticsValue::List` of 16 floats under a `"transform"` property only when non-identity (the text renderer omits it; the property simply absent when identity — that omission is a curation choice acceptable here, OR always present and the renderer hides identity; pick one and document).

- [ ] **Step 3: Verify**

Run: `cargo test -p flui-painting diagnostics::` (PASS) + `cargo clippy -p flui-painting --all-targets --all-features -- -D warnings` (clean — the schemars path compiles).

- [ ] **Step 4: Commit**

```bash
git add crates/flui-painting/src/display_list/
git commit -m "feat(painting): impl Diagnosticable for DrawCommand (typed per-command properties)"
```

---

## Task 4.5: `From<value-type> for DiagnosticsValue` + generic `add_value` — diagnostics through the leaf types

Make the geometry/color value types self-convert to `DiagnosticsValue` so the `DrawCommand`/`Layer` arms compose via one generic `add_value` instead of destructuring fields at every call-site. This realizes "go through the types and wire diagnostics" at the **correct layer**: the conversions live in `flui-foundation` (where `DiagnosticsValue` is local — orphan-rule legal) reaching *down* to the `flui-types`/`flui-geometry` types it already depends on. The literal alternative — `flui-types` depending on the diagnostics crate — is a **dependency cycle** (`flui-foundation` already depends on `flui-types`) and is rejected. Value types get `From<T> for DiagnosticsValue` (a `Rect` as a *property* is a `DiagnosticsValue::Rect`, not a node); only *objects* (RenderObject/Layer/DrawCommand) are `Diagnosticable` (→ node).

**Files:** `crates/flui-foundation/src/debug.rs` (From impls + `add_value`); `crates/flui-painting/src/display_list/command_ops.rs` (simplify arms); later `crates/flui-layer` arms.

- [ ] **Step 1: Failing conversion + ergonomics test**

```rust
#[test]
fn rect_converts_to_typed_value() {
    let r = flui_types::geometry::Rect::from_ltwh(/* 0,0,40,40 in Pixels */);
    assert!(matches!(DiagnosticsValue::from(r), DiagnosticsValue::Rect { w, .. } if w == 40.0));
}
#[test]
fn add_value_matches_explicit() {
    let mut a = DiagnosticsBuilder::new(); a.add_rect("r", 0.0,0.0,40.0,40.0);
    let mut b = DiagnosticsBuilder::new(); b.add_value("r", /* Rect 0,0,40,40 */);
    assert_eq!(a.build(), b.build());
}
```

Run → FAIL.

- [ ] **Step 2: Add the From impls (flui-foundation, orphan-rule)**

`impl From<Rect<Pixels>> for DiagnosticsValue` (→ `Rect{x,y,w,h}` via `.left().get()` etc.), `From<Color>` (→ `Color{r,g,b,a}`), `From<Point<Pixels>>`/`From<Offset<Pixels>>` (→ `Offset`), `From<Size<Pixels>>` (→ `Size`), `From<RRect>` (→ a typed form that preserves the bounds **and** per-corner radii — e.g. `Nested([rect, radii-List])`), `From<&Matrix4>` (→ `List` of the 16 elements). Verify each source type's real accessors before writing. (`Matrix4` stays by-ref since it isn't `Copy`-cheap.)

- [ ] **Step 3: `DiagnosticsBuilder::add_value`**

`pub fn add_value(&mut self, name: impl Into<String>, value: impl Into<DiagnosticsValue>) -> &mut Self` — sets the matching `DiagnosticsPropertyKind` from the resulting variant. Keep the explicit `add_rect`/`add_color_rgba`/… (they delegate to `add_value` or stay).

- [ ] **Step 4: Simplify the `DrawCommand` arms**

In `command_ops.rs`, replace manual `add_rect("rect", r.left().get(), …)` / `add_color_rgba(…)` with `add_value("rect", *rect)` / `add_value("color", color)` etc. **Behaviour identical** — the typed values are the same; this is ergonomics + uniformity, not a fidelity change. The Task 4 fix (transform/radii/bounds/text) must remain intact; the `RRect` radii now flow through `From<RRect>`.

- [ ] **Step 5: Gates**

`cargo test -p flui-foundation --features serde` + `cargo test -p flui-painting` + `cargo build --workspace` + `cargo clippy -p flui-foundation -p flui-painting --all-targets --all-features -- -D warnings` + `cargo fmt`.

- [ ] **Step 6: Commit**

```bash
git add crates/flui-foundation/src/debug.rs crates/flui-painting/src/display_list/command_ops.rs
git commit -m "feat(foundation): From<geometry/color> for DiagnosticsValue + add_value — uniform diagnostics through the value types"
```

> `Layer` arms (Task 5) are authored directly against `add_value` from the start (no double-write).

---

## Task 5: `Layer` command-descent + opaque-handle projection

**Files:** Modify `crates/flui-layer/src/layer/mod.rs` (the `to_diagnostics_node` override at ≈395) + per-variant layer files as needed.

- [ ] **Step 1: Write the failing descent test**

```rust
#[test]
fn picture_layer_diagnostics_has_command_children() {
    let tree = /* OffsetLayer over PictureLayer with one DrawRect */;
    let node = /* root Layer */.to_diagnostics_node();
    // Offset node → Picture child → DrawRect grandchild
    let picture = &node.children()[0];
    assert_eq!(picture.name(), Some("Picture"));
    assert_eq!(picture.children()[0].name(), Some("DrawRect"));
}
```

Run → FAIL (Picture node has no command children today — only `commands: <len>`).

- [ ] **Step 2: Extend `Layer::to_diagnostics_node`**

For each `Layer` variant, build the node: name = layer kind (`"Offset"`, `"Picture"`, `"ClipRect"`, …), typed properties mirroring the existing `write_layer` header (e.g. `Offset` → `add_f64("dx", …)`+`add_f64("dy", …)`; `ClipRect` → `add_rect("rect", …)`+`add("clip", behavior)`; `Picture` → `add_rect("bounds", …)`), and **children**: for `PictureLayer`, push one `cmd.to_diagnostics_node()` per `DisplayList` command; for all layers, recurse into sublayers. Opaque handles → id properties (`Texture` → `add_int("id", texture_id)`, `PlatformView` → `add_int("id", …)`, `Leader`/`Follower` → `add_int("link_id", …)`); never the raw resource.

- [ ] **Step 3: Verify**

Run: `cargo test -p flui-layer` (PASS) + `cargo clippy -p flui-layer --all-targets --all-features -- -D warnings` (clean).

- [ ] **Step 4: Commit**

```bash
git add crates/flui-layer/src/layer/
git commit -m "feat(layer): to_diagnostics_node descends PictureLayer into per-command child nodes (opaque handles → ids)"
```

---

## Task 6: Painted snapshot via `DiagnosticsNode` + `SnapshotStrategy`; regenerate goldens; retire the #213 serializer

**Files:** Modify `crates/flui-rendering/src/testing/snapshot.rs` (retire `summarize_command`/`serialize_layer_tree`/`DrawCommandSummary`/`DrawKind`; add `scene_diagnostics` + `SnapshotStrategy`); `harness.rs` (`snapshot()` → new path); `tests/harness_snapshot.rs` (+`.json` test); `tests/snapshots/*.snap` (regenerate); `docs/TESTING.md`.

- [ ] **Step 1: Add `scene_diagnostics` + `SnapshotStrategy`**

`pub fn scene_diagnostics(tree: &LayerTree) -> DiagnosticsNode` (walk the painted tree via `Layer::to_diagnostics_node` from the root). `pub struct SnapshotStrategy<V, F> { render: fn(&V) -> F }` with `SnapshotStrategy::<DiagnosticsNode, String>::{text, json}` (`text` = `node.to_string()`; `json` = `serde_json::to_string_pretty(node)` — add `serde_json` dep behind the testing/serde path).

- [ ] **Step 2: Repoint `harness.snapshot()` + deprecate the shim**

`harness.rs::snapshot()` → `scene_diagnostics(&tree).to_string()`. `serialize_layer_tree` → `#[deprecated(note = "use scene_diagnostics + SnapshotStrategy::text")]` delegating to the new path; remove `summarize_command`/`DrawCommandSummary`/`DrawKind` (now replaced by `DrawCommand: Diagnosticable`) and update any `.kind`/`.line` consumers in harness.rs/TESTING.md to the node API (`value_of`/`name`).

- [ ] **Step 3: Regenerate the six goldens (reviewed)**

Run `cargo test -p flui-rendering --test harness_snapshot` → the 6 `.snap` fail (format changed). Run `cargo insta accept --all`. Then **diff each regenerated `.snap` against its git-previous version** and confirm it still asserts the same discriminating facts: `clip_layer`/`lazy_sliver` clip behavior; `decorated_box` shadow→fill→border + colors; `lazy_sliver` nine distinct per-rect transforms; `opacity_layer` alpha. If any fact is lost, fix the `Diagnosticable` impl (Task 4/5), not the golden.

- [ ] **Step 4: Add the `.json` synthetic-inspector test**

In `tests/harness_snapshot.rs`, `assert_snapshot!` over `SnapshotStrategy::json().render(&scene_diagnostics(&tree))` for the 4 painted subjects. Accept.

- [ ] **Step 5: Verify**

Run: `cargo test -p flui-rendering` (PASS) + `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings` (clean) + `rg "summarize_command|DrawCommandSummary" crates --glob '*.rs'` (only the `#[deprecated]` shim, no live callers).

- [ ] **Step 6: Commit**

```bash
git add crates/flui-rendering/ 
git commit -m "feat(rendering): painted-scene snapshot via the DiagnosticsNode tree + SnapshotStrategy; retire the #213 string serializer; regenerate goldens"
```

---

## Task 7: Committed JSON Schema + CI diff-gate

**Files:** Create `schema/diagnostics.v1.json`; a generator (xtask or a gen test); `crates/flui-foundation/tests/schema_stability.rs`; CI workflow.

- [ ] **Step 1: Add the `format_version` envelope**

In `flui-foundation`: `#[non_exhaustive] pub struct DiagnosticsEnvelope { pub format_version: u32, pub root: DiagnosticsNode }` (serde + schemars), `pub const DIAGNOSTICS_FORMAT_VERSION: u32 = 1;`. The `.json` strategy wraps the node in the envelope.

- [ ] **Step 2: Write the failing schema-stability test**

```rust
#[test]
fn committed_schema_matches_generated() {
    let gen = serde_json::to_string_pretty(&schemars::schema_for!(flui_foundation::DiagnosticsEnvelope)).unwrap();
    let committed = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/diagnostics.v1.json")).unwrap();
    assert_eq!(gen.trim(), committed.trim(),
        "diagnostics schema drifted — review the diff; if intended, regenerate + bump DIAGNOSTICS_FORMAT_VERSION per ADR-0005");
}
```

Run → FAIL (file missing).

- [ ] **Step 3: Generate + commit the schema; verify**

Generate `schema/diagnostics.v1.json` from `schema_for!(DiagnosticsEnvelope)`; commit it. Run the stability test → PASS.

- [ ] **Step 4: Wire the CI gate**

Add a CI step running the stability test with `--features schemars` (document: schemars output isn't semver-stable → the committed file is the owned gate).

- [ ] **Step 5: Commit**

```bash
git add schema/diagnostics.v1.json crates/flui-foundation/ .github/ xtask/ 2>/dev/null
git commit -m "feat(foundation): committed diagnostics JSON Schema + CI diff-gate (the years-long contract)"
```

---

## Task 8: Final gates

- [ ] **Step 1: Full verification**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p flui-foundation -p flui-painting -p flui-layer -p flui-rendering && bash scripts/port-check.sh`
Expected: all clean; regenerated goldens + `.json` + schema tests green; the ~49 existing diagnostics impls untouched and passing; port-check exit 0.

- [ ] **Step 2: Confirm retirement**

Run: `rg "DrawCommandSummary|summarize_command|serialize_layer_tree" crates --glob '*.rs'`
Expected: only `#[deprecated]` shims; no live internal callers.

- [ ] **Step 3: Commit any fixups**

```bash
git add -A && git commit -m "chore(diagnostics): fmt/clippy/port-check green"
```

---

## Self-review notes (author)

- **Coverage:** Task 3 = ADR-0005 D1 (typed value); 4 = D2 (DrawCommand Diagnosticable); 5 = D2 (Layer descent); 6 = D3 (one tree → strategies, goldens, retire sibling); 7 = D4 (schema + version); 8 = gates. Foundation D5 (keep Tasks 0-2) already done.
- **Back-compat is the Task-3 gate** (`cargo build --workspace` + the foundation doctests) — the ~49 impls must not change.
- **Goldens regenerate** (no byte-identity here, unlike ADR-0004) — the gate is reviewed content-equivalence vs the git-previous `.snap`, Task 6 Step 3.
- **Layering caveat flagged** (Task 3 Step 3): `flui-foundation` must not depend on `flui-painting`; `DiagnosticsValue::Display` formatting lives in foundation.
- **Faithful-vs-normalized invariant** tested (Task 3: typed JSON full-precision vs text 2-dec).
