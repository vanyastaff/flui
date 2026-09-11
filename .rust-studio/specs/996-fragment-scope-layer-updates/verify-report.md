<!-- Rust Code Studio — verification report for a spec. Evidence over assertion: real command output. -->

# Verify Report: update-only composited-layer commits for fragment-scope effects (#996)

- **Spec:** [`spec.md`](spec.md) (v4 Approved)   ·   **Date:** `2026-09-11`   ·   **Verdict:** `COMPLETE` (PR3 pending merge; PR4 = the issue close)

Verified on branch `feat/996-pr3-clip-producers` @ `cf2a79fc` (main @ `114ffa84`, which contains PR1 #1019 `e0f041a2` and PR2 #1020).

## Independent acceptance → original request

- **Applicability:** required — four PRs, cross-crate (flui-layer / flui-rendering / flui-objects / flui-widgets / flui-engine), changed observable behaviour (setter impacts, poison phases).
- **Original input:** `intent.md` "Asked for" (issue #996 body + last comments, verbatim), "What 'fixed' looks like", "Constraints the user owns", "Not this"; user amendment 2026-09-10 "Продолжай" (no scope change). No unrecoverable quotation.
- **Independence:** a fresh `qa-lead` given ONLY the intent text; instructed not to open `.rust-studio/specs/` (spec/tasks/verify-report off-limits); it read the tree and ran filtered tests. Not contaminated by the design.
- **Worker verdict (verbatim):** "**ACCEPT WITH GAPS** — Gaps: (1) `RenderClipPath` lacks a direct layer-content-equals-a-repaint assertion; (2) `RenderClipOval`/`RenderClipRect` get no pipeline-level 'flat + equal' proof, descriptor unit tests only; (3) the captured-origin invariant for the clip family is pinned by a synthetic double whose doc comment is now factually stale."
- **Combined:** all three gaps closed in `cf2a79fc` (`a_path_target_change_updates_the_clip_layer_without_repainting_the_subtree` with probe-point path comparison; `a_clip_shape_change_…` / `an_oval_clip_shape_change_…`; the captured-origin test now drives `RenderClipRect::set_clip_shape`). No retained user requirement is missing; nothing was withdrawn.

| User requirement / amendment | Result | Evidence and provenance |
|---|---|---|
| Clip rect / rrect / oval / path: property tick under a retained boundary → subtree paint count flat, same layer tree as a repaint | implemented | rendering_it: `a_border_radius_change_updates_the_clip_layer_without_repainting_the_subtree`, `a_clip_shape_change_…`, `an_oval_clip_shape_change_…`, `a_path_target_change_…`, `a_clip_layer_update_is_written_back_into_the_retained_capture` — ran (blind worker + QA-GATE + orchestrator `just ci`) |
| Physical model, backdrop filter, image filter, shader mask: served or the record says why not, setters still report the repaint | implemented (deferred with record) | `crates/flui-rendering/ARCHITECTURE.md` "Deferred producers" table (nine rows with triggers); `physical_model.rs`/`backdrop_filter.rs` untouched, setters still `PAINT` — blind worker read the files |
| `paint_layer_blend`'s fate | implemented (deleted) | PR1 `e0f041a2`; CHANGELOG `### Removed`; `rg paint_layer_blend crates --type rust` → 0 |
| Both bench shapes, both arms dirtying the same node, both numbers quoted | implemented | `benches/paint.rs` `bench_clip_rrect_radius_change`, `bench_clip_path_token_change` (lane-entered, clipper-count assert); ARCHITECTURE.md measurement paragraph (N=1 and N=1000, inline and layered) |
| No boundary promotion; captured-origin invariant and per-position guard not weakened; structural transition → `PAINT` | implemented | `RenderClip` never overrides `is_repaint_boundary`; guard at `layer_patches_for` unchanged (reviewer lens 3); `RenderFlow::set_clip_behavior` stays `PAINT \| SEMANTICS` — `a_flow_clip_behavior_change_is_structural_and_refused` |
| `RenderFittedBox` asymmetry untouched and recorded; hit-test/semantics of clips unchanged; patch frame == repaint frame | implemented | `fitted_box.rs` zero diff (doc line only); `hit_test`/`describe_approximate_paint_clip` zero diff; readback `the_clip_update_path_and_a_repaint_produce_the_same_pixels` (local adapter, 6/6) |
| Flutter accounting for the improvement | implemented | ARCHITECTURE.md `## Mapping decisions` clip-family paragraph citing `_RenderCustomClip` setters (`proxy_box.dart`) and `markNeedsCompositedLayerUpdate`/`updateLayerProperties` (`object.dart`) at 3.44.0 |

## Acceptance criteria → result

| # | Criterion | Result | Evidence |
|---|---|---|---|
| AC0 | PR1 behaviour-neutral | ✅ | PR1 `just ci` 9822+40+273; SCOPE-GATE PASS (reviewer read every test hunk); counts rendering 928→931, objects 954→953 |
| AC1 | five setters `NONE`/`COMPOSITED_LAYER_UPDATE` | ✅ | `clip.rs` five impact tests, red-first (`PAINT\|SEMANTICS (10)` vs `24`) |
| AC2 | update without repaint | ✅ | `a_border_radius_change_…` (count flat, equal to fresh at new radius, ≠ old); mutation setter→`PAINT` gives `left: 2 right: 1` |
| AC3 | write-back, three frames | ✅ | `a_clip_layer_update_is_written_back_into_the_retained_capture` |
| AC4 | captured origin | ✅ | `a_same_frame_layout_change_forces_the_repaint_a_clip_patch_relies_on` on the real `RenderClipRect`; calibration → `(32,12)` vs `(102,12)` |
| AC5 | structural refusal, production type | ✅ | `a_flow_clip_behavior_change_is_structural_and_refused` (count guard; flag cleared; `ClipRect` gone) |
| AC6 | panic on either arm | ✅ | `tests/effect_descriptor_poison.rs` ×5 (`PoisonPhase::LayerUpdate`/`Paint`, exactly one call, retry keeps the queued value; both gates) |
| AC7 | widget wiring | ✅ | `rebuilding_a_clip_rrect_widget_updates_its_layer` (says what it cannot see) |
| AC8 | pixels | ✅ | readback `the_clip_update_path_and_a_repaint_produce_the_same_pixels`, `a_different_radius_produces_different_pixels` at (21,21): white at r=8, red at r=2 |
| AC9 | measurement | ✅ | two groups, both shapes, N=1 and N=1000 quoted (`885a0d6a`); clipper-count assert outside the timed closure, proven live |
| AC10 | accounting | ✅ | rule + enumeration command, improvement paragraph (word-for-word the spec's), per-producer entries, Deferred table (9 rows), market citation, CHANGELOG Added/Changed/Removed |
| AC11 | no user code / no copy on coordinate queries | ✅ | `harness_transform_to_through_a_path_clip_runs_no_registered_clipper` (count 0); `clip_descriptor_fixed_path_shares_the_arc_without_copying` (`ptr::eq`) — the no-copy half is unobservable through `transform_to` (reads only `.transform`) |
| AC12 | patch-arm order = paint order | ✅ | `two_path_clips_under_one_boundary_resolve_in_paint_order` — marked in REVERSE tree order, `[A, B]` on 3 runs, leaves flat |

Intent trace: every line of "What 'fixed' looks like" and every owned constraint has a criterion; AC11/AC12 trace to risks the chosen mechanism introduces (walk-resolved `PathTarget`, capture-ordered `effect_slots`) rather than to the intent directly — a finding, not a failure.

## Commands run (evidence)

```
$ just ci                                   # branch @ cf2a79fc
     Summary [  15.135s] 9847 tests run: 9847 passed, 4 skipped
     Summary [   0.085s] 40 tests run: 40 passed, 0 skipped       # flui cupertino/localizations
     Summary [   0.359s] 273 tests run: 273 passed, 8 skipped     # flui-platform headless
just ci exit: 0

$ cargo nextest run -p flui --features gpu-readback-tests --no-default-features \
    --test composited_layer_update_readback --locked --test-threads 1
     6 tests run: 6 passed, 0 skipped                              # local adapter

$ cargo bench -p flui-rendering --bench paint -- '_change/(inline|layered)/(update|repaint)/(1|1000)$' \
    --warm-up-time 1 --measurement-time 3                          # 2026-09-11, 32 cores
   clip_rrect  inline/1000 220x  layered/1000 1.71x  inline/1 1.54x  layered/1 1.29x
   clip_path   inline/1000 192x  layered/1000 1.69x  inline/1 1.47x  layered/1 1.27x
```

Skipped tests in the denominator: 4 in the workspace default run and 8 in flui-platform are the pre-existing host-conditional skips (unchanged by this work); none of this spec's tests is `#[ignore]`d.

## Gates cleared
- [x] QA-GATE (`qa-lead`) — FAIL on AC9 (N=1 not quoted) → closed in `885a0d6a`; re-checked by reading
- [x] PERF — bench driver + numbers (3.6 report; `systems-perf-lead` PERF-GATE PASS at task 1.5; the structural +4–5 % repaint-arm rise recorded in spec §Risks)
- [x] API — impact bits and the sealed hook `ClipGeometry::path_target_descriptor` (reviewer lens 2, no findings); D6 API-GATE ACCEPTABLE
- [x] `rust-reviewer` diff audit — NEEDS WORK (minor) → all findings applied in `cf2a79fc`; declined with reasons: the `clip_descriptor`/`resolve_clip` source-selector reshape, the default-shaped `RenderClipPath` allocation on coordinate queries

## Follow-ups / left out of scope
- Deferred producers per the spec table (backdrop filter, shader mask, image filter, physical model, Flow per-child transforms, FittedBox, overflow-gated clips, ClipSuperellipse, blend) — each with its trigger in ARCHITECTURE.md.
- `set_clip_behavior` reports no `SEMANTICS` while the a11y clip gates on `Clip::None` — pre-existing, identical upstream; recorded, not fixed.
- `viewport.rs` `let _ = set_clip_behavior(..)`: read — both discards are in `create_render_object` (pre-tree, documented); not a defect.

## On pass
Spec marked Done once PR3 merges; PR4 closes #996 quoting the Deferred table.
