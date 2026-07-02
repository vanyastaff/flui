# C1.13 — Flutter parity-test scaffolding plan (Core.1 exit gate)

De-risking plan for `crates/flui-widgets/tests/parity/`. Gate = "the scaffolding
exists + a first slice of parity tests passes," NOT "port all of Flutter's tests."

## Headline: FLUI already has a widget-level test harness
The `WidgetTester` equivalent exists as a pair — no new subsystem needed:
- `crates/flui-binding/src/lib.rs` — `HeadlessBinding::pump_frame(dt)` (`:333`): the
  FLUI port of `TestWidgetsFlutterBinding.pump` (advance clock → gesture deadlines →
  tick controllers → build_scope → run_frame → service_child_requests). Non-singleton.
- `crates/flui-widgets/tests/common/mod.rs` — `lay_out(root, constraints)` (`:60`) +
  `LaidOut`/`LaidOutScoped`: mount a View, geometry readers (`size`/`offset`/`child`/
  `root`/`render_node_count`), re-pump (`pump`=setState, `tick`/`pump_for`=animation),
  animation readers, gesture dispatch. Its own doc calls it "the Core.1 parity-oracle
  infrastructure."

## The gap — 3 modest, ADDITIVE, test-only primitives (Phase 1)
Build in `tests/parity/harness.rs` on top of the existing `LaidOut`:
1. `screen()` = `BoxConstraints::tight(800×600)` — Flutter's default test surface, so
   ported pixel literals are mechanical.
2. `find_by_render_type(&str) -> RenderId` (+ `find_all_by_render_type`) — walk
   `inspect::render_diagnostics(&owner)` (`flui-rendering/src/testing/inspect.rs:135`)
   with `DiagnosticsNode::find_descendant_unique(name)` (`flui-foundation/src/debug.rs:703`);
   bridge the diagnostics node → `RenderId` (may need a tiny `render_diagnostics`
   addition to carry the id).
3. `pump_widget(new_root: impl View)` — root-swap re-pump via
   `ElementTree::update(root_id, &new_root, owner)` (`flui-view/src/tree/element_tree.rs:1180`)
   then `pump_frame(ZERO)` — Flutter's `pumpWidget(w2)`.
4. `find_text(&str)` — VERIFY `RenderParagraph::to_diagnostics_node` emits its text
   (`flui-objects/src/text/paragraph.rs`); if absent, add a `text` diagnostics property
   (small production win) OR fall back to type/positional finders for the first cut.
5. Self-test (`harness_finds_and_measures`) proving pump_widget + find + size agree
   before any port relies on them (mirror `flui-rendering/tests/harness_self_test.rs`).
No new deps; no production API change (except the optional RenderParagraph text prop).

## Directory structure
```
crates/flui-widgets/tests/
├── common/mod.rs        # EXISTING harness (reused verbatim via #[path])
└── parity/
    ├── main.rs          # `mod harness; mod sized_box_test; ...` (single `parity` test binary)
    ├── harness.rs       # WidgetTester-shim: screen() + find_by_render_type + pump_widget
    ├── sized_box_test.rs center_test.rs container_test.rs flex_test.rs
    ├── text_test.rs list_view_test.rs stateful_test.rs
```
If auto-discovery of `tests/parity/main.rs` isn't picked up, add `[[test]] name="parity"
path="tests/parity/main.rs"` to flui-widgets/Cargo.toml. Each file opens with a
`## Test parity notes` block citing the `.flutter/` source + any documented divergence.

## Phase 2 — 8 first-cut parity ports (the passing gate)
Geometry-only where possible; each cites the Flutter source. 3/5/7/8 are portable with
the existing harness (land green first); 1/2/4/6 exercise the new finder/root-swap/surface.
1. `sized_box_no_child` — `sized_box_test.dart:39` (SizedBox 100×100 → size; expand → 800×600). [screen, find, pump_widget]
2. `center_zero_area` — `center_test.dart:17` (Center zero area → Size::ZERO). [find]
3. `container_layout` — `container_test.dart:52` layout half (Align.topLeft + margin/padding/size → child rect). [positional]
4. `flexible_defaults_to_loose` — `flex_test.dart:70` (Flexible loose child width==100; substitute a Row fixed-child geometry port if Flexible absent). [find]
5. `column_no_overflow_fp` — `flex_test.dart:84` (6× Expanded in 400/199px column, no overflow). [positional]
6. `text_measures_nonempty` — `text_test.dart` (RenderParagraph non-degenerate box). [find RenderParagraph]
7. `list_view_builds_visible` — `list_view_test.dart` (dynamic Vec list populates viewport; C1.7/C2-dynamic). [existing lazy_list two-tick + render_node_count]
8. `stateful_rebuild_on_setstate` — counter idiom (setState re-lays subtree, element identity kept; C1). [existing stateful pump]
Gate: `cargo nextest run -p flui-widgets --test parity` green + each file cites Flutter source.

## Side-by-side example (sized_box_test.dart:39 → FLUI)
```rust
#[test]
fn sized_box_no_child() {
    let mut t = harness::pump_widget(Center::new().child(SizedBox::new(100.0, 100.0)), harness::screen());
    let sb = t.find_by_render_type("RenderConstrainedBox"); // SizedBox → RenderConstrainedBox
    assert_eq!(t.size(sb), size(100.0, 100.0));
    t.pump_widget(Center::new().child(SizedBox::expand()));  // root swap over ElementTree::update
    assert_eq!(t.size(t.find_by_render_type("RenderConstrainedBox")), size(800.0, 600.0));
}
```

## No ADR required
The finder + pump_widget + screen() are a test-only additive layer over existing primitives
(render_diagnostics, find_descendant_unique, ElementTree::update, HeadlessBinding). One
documented design choice: the finder matches RENDER-object type names (what diagnostics
expose), not widget types — record the widget→render mapping in each parity file's notes;
widget-type + by-key finders + paint/semantics assertions are deferred (Phase 3).

## Phase 3 (deferred, out of C1.13 scope)
find.byKey (element key→RenderId bridge), paint-pattern assertions (insta snapshots +
inspect::layer_structure), semantics-tree assertions, the broader ~150-test corpus.
