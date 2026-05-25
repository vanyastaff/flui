[← Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md) · [← Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md)

# U17 Spike Report — Option C wrapper feasibility

> **Decision outcome:** **Option D approved as PR 2**. Spike confirmed Option C exceeds the 3× LOC threshold in 4 of 5 scenarios, and surfaced a new architectural blocker (R12: `RenderBox::size` trait surface cascade) that the original research did not account for.
>
> **Spike code:** lost when worktrees auto-cleaned after subagent completion. Findings preserved in this report. If Option C is ever reconsidered, the wrapper sketch can be reconstructed from `~/.pi/agent/sessions/.../7f766d51/run-1/session.jsonl` tool calls or rebuilt fresh in ~5 hours per spike timing.
>
> **Date:** 2026-05-25.
> **Authored by:** worker subagent (Pi-driven 2-day spike).
> **Validation:** orchestrator + advisor (3rd consultation 2026-05-25 confirmed Option D direction).

---

## 1. Measured numbers

### 1a. Wrapper crate LOC

Measured via `wc -l` on each new file in spike worktree:

| Metric                              | Total LOC | Code-only | Comment LOC | Tests LOC | Notes |
|---                                  |     ---:  |     ---:  |       ---:  |      ---: |---|
| `wrappers/length.rs`                |       409 |       198 |         158 |        56 | U1/U2/U4 invariants, mint surprise, bytemuck Pod |
| `wrappers/point.rs`                 |       327 |       187 |          93 |        83 | Flutter parity (`lerp_to`, `distance_to`, `ZERO`); Point-Size arithmetic |
| `wrappers/size.rs`                  |       272 |       182 |          47 |        50 | `area`, `aspect_ratio`, `ZERO`/`INFINITE` consts |
| `wrappers/rect.rs`                  |       265 |       179 |          49 |        69 | `from_origin_size`/`from_ltrb`/`from_xywh`; contains/intersect/union |
| `wrappers/mod.rs`                   |        66 |        21 |          38 |         0 | re-exports + aliases module |
| `wrappers/units.rs`                 |        27 |         4 |          21 |         0 | `PixelsUnit` / `DevicePixelsUnit` zero-sized markers |
| Cargo.toml deltas (geom + rendering)|       ~30 |       ~24 |          ~6 |         0 | spike-wrappers feature, euclid/kurbo/bytemuck opt deps |
| **Wrapper subtotal (file totals)**  | **1,396** | **795**   | **412**     | **258**   | _measured_ |

Advisor estimate (research §VIII R-PreFlight-1): **1,200–2,000 LOC**. Measured **1,396** sits exactly in the middle. Advisor estimate validated.

### 1b. Padding widget migration delta

Cloned `padding.rs` to `padding_spike.rs` and rewrote internals to use wrapper types. Boundary types (`BoxConstraints`, `EdgeInsets`, `BoxParentData`) stayed on `flui_types`; spike added shim helpers at the boundary.

| File                    | Total LOC | Code-only |
|---                      |     ---:  |     ---:  |
| `padding.rs` (baseline) |       190 |       128 |
| `padding_spike.rs`      |       283 |       165 |
| **Δ per widget**        |   **+93** |    **+37** |

Of those +37 code LOC:

- ~30 LOC are **boundary shim helpers** (6 functions + 1 `_size_cache` field). These exist only because spike is partial migration. Under PR 2 Option C the boundary types themselves migrate; shims vanish.
- ~7 LOC are real type-annotation deltas at constructor sites (irreducible per-widget cost of Option C).

Two figures depending on interpretation:

| Question | per_widget_LOC | Comment |
|---|---:|---|
| What does the **measured spike** cost? | **37** | with shims, partial migration |
| What would Option C cost **after PR 2** boundary migration? | **~7** | shims deleted; only construction-site syntax churn |

### 1c. Option C totals against 2,250 LOC threshold

| Scenario | Wrapper | per_widget | Total | vs. 2,250 |
|---|---:|---:|---:|---|
| A — Raw spike, optimistic per-widget | 1,396 | 7 | **1,956** | ✅ under by 294 |
| B — Raw spike, measured per-widget | 1,396 | 37 | **4,356** | ❌ over by 2,106 |
| C — Spike + 50% production polish, optimistic | 2,094 | 7 | **2,654** | ❌ over by 404 |
| D — Spike + 50% production polish, measured | 2,094 | 37 | **5,054** | ❌ over by 2,804 |
| E — Central realistic estimate | 1,700 | 15 | **2,900** | ❌ over by 650 |

**4 of 5 scenarios fail the threshold.** Mechanical decision rule → **Option D**.

---

## 2. New architectural blocker — R12

**`RenderBox::size(&self) -> &Size` trait surface forces parallel cache field under wrapper backing.**

The existing trait returns a borrow of `flui_types::Size`. Under wrapper backing, `RenderPaddingSpike` cannot return `&WSize` without changing the trait. Spike pays this with a parallel `_size_cache: flui_types::Size` field mirroring the wrapper-side `size: WSize`. Per-widget memory: +16 bytes.

Under PR 2 Option C, the trait signature itself must migrate: `fn size(&self) -> &WSize`. That's a **cascade across the renderer** — every `RenderBox` impl, every caller that takes `&Size`. **Not done in spike** because it's outside scope.

**Severity:** **blocker** for naive per-widget migration. Mitigated only by:
- (a) keeping the cache field forever (REJECTED: doubles memory per widget across thousands of render objects)
- (b) migrating the boundary types simultaneously — **the actual PR 2 commitment**

This reshapes cost calculus: Option C is **not** "wrapper layer over euclid", it's "polymorph cascade across the whole renderer trait surface." Monolithic PR ~3,000–4,000 LOC OR phased migration with shim-tax window (creates parallel-type SP-3 violation during transition).

**Neither shape is foundation-quality.** This is the decisive evidence against Option C.

---

## 3. Ergonomic surprises (R13, R14, R15, R16)

### R13 (ANNOYING) — `Point::from_lengths` inference fails when unit is ambiguous

```rust
// Today (polish-pass):
let p = Point::<Pixels>::new(px(3.0), px(4.0));   // ✓

// Option C (spike):
let p: PixelPoint = Point::from_lengths(
    Length::<f32, PixelsUnit>::new(3.0),           // <- explicit Length annotation
    Length::<f32, PixelsUnit>::new(4.0),           //    required, inference fails
);
```

Cause: `Point::from_lengths` is generic over `U`, `Length::new(scalar)` cannot infer `U` until outer type is fully resolved. Today's API doesn't have this issue because `Pixels` is both scalar type AND unit — no second type parameter.

Workaround: per-alias convenience constructors (`PixelPoint::from_lengths(px(3.0), px(4.0))`). Restores ergonomics but requires per-alias surface — wrapper-side cost not counted in 1,396 LOC.

**Tax:** every `Point`/`Size` construction site needs type annotation OR per-alias constructor.

### R14 (ANNOYING) — `Point::ZERO` / `Size::ZERO` ambiguate under multi-scalar wrappers

```rust
let r = Rect::from_origin_size(Point::ZERO, Size::ZERO);
// ↑ compiler rejects: both <f32, _> and <i32, _> impls define ZERO

let r = Rect::from_origin_size(PixelPoint::ZERO, PixelSize::ZERO); // ← required
```

Today's API doesn't have this because `Pixels` and `DevicePixels` are distinct scalar newtypes — `Point::ZERO` is unambiguously `Point<Pixels>::ZERO` via inference.

**Tax:** ~2 extra tokens at every `*::ZERO` site. Codebase has 30+ such sites.

### R15 (COSMETIC) — `mint 0.5` has no `Vector1<T>`

`Length`-to-mint bridge impossible. Deleted from spike. The kurbo/glam bridge mint enables happens at `Point`/`Vector`/`Size` boundaries instead — which is where GPU/curves consumers want it anyway. **Zero code cost** beyond a doc note.

### R16 (COSMETIC) — `kurbo 0.13` requires `std` or `libm` feature

Resolved by enabling `std` feature in spike Cargo.toml. Real PR 2 (Option D) does same.

---

## 4. Time consumed

- Reading research §VIII + units.rs/lib.rs/Padding: ~30 min
- Adding deps + spike-wrappers feature: ~10 min
- Writing 5 wrapper files: ~3 hours
- Iteration on compile errors: ~45 min
- Padding_spike.rs + trait-surface blocker: ~45 min
- Validation runs: ~15 min
- Report writing: ~30 min
- **Total: ~5.5 hours**, half of 2-day budget

Under-spend because euclid did more zero-author work than estimated (`lerp`, `distance_to`, `cast_unit`, `intersection`, `union`, `contains` all came free), and wrapper API was minimum-viable for Padding migration (production-ready surface would be wider — scenario E +25% accounts for this).

---

## 5. Validation runs (all green in spike worktree)

```
cargo build --workspace                                          # default,   OK
cargo build --workspace --features flui-geometry/spike-wrappers  # spike on,  OK
cargo build --workspace --features flui-rendering/spike-wrappers # spike on,  OK
cargo test  -p flui-geometry --features spike-wrappers wrappers  # 28 pass,   OK
cargo test  -p flui-geometry --features spike-wrappers --doc     # 6 compile_fail pin pass, OK
cargo test  -p flui-rendering --features spike-wrappers padding  # OK
cargo clippy -p flui-geometry --features spike-wrappers          # 0 warnings, OK
```

(`flui-platform::test_unicode_support` is flaky on Windows, unrelated to spike.)

---

## 6. Decision — Option D selected as PR 2

**Both lines of evidence point to Option D:**

1. **Quantitative:** 4 of 5 scenarios over 3× threshold. Central estimate (E) over by 650 LOC.
2. **Qualitative:** R12 blocker requires monolithic cascade migration OR shim-tax window — neither is foundation-quality.

**Ergonomic tax (R13 + R14)** multiplied × 80 widgets = codebase-wide friction. Foundation-quality argument for C assumed clean payoff; spike showed payoff requires either:
- Monolithic ~3,000–4,000 LOC PR (violates 400-LOC review-workload guard)
- In-flight shim window (violates SP-3 parallel-types refusal trigger)

**Option D under stable API IS foundation-quality** when presented as deliberate choice in doc-comments. `flui::Matrix4(glam::Mat4)` with note "we own unit-typed wrappers for polish discipline; glam handles SIMD math" — future contributors see deliberate scope, not arrested migration.

**Option D still strict improvement:**
- SIMD (SSE2/NEON/wasm-simd128) free via glam
- Pod compatibility free via `feature = "bytemuck"`
- mint cascade auto-bridges kurbo (PR 3 ~5 lines)
- 3,833 LOC of own math reduced to ~750 LOC delegation

---

## 7. Suggestions for research doc and tracker updates

- Add R12 (RenderBox::size cascade), R13 (from_lengths inference), R14 (ZERO ambiguity), R15 (no mint::Vector1), R16 (kurbo std feature) to research doc Part VIII risks section as **decisive evidence for Option D**.
- Update tracker: U17 → `✓ done` with decision recommendation = D. U14C → `🛇 not selected`. U14 (Option D) → primary path forward as PR 2.
- Update research doc Part VIII "Recommended sequencing" diagram: PR 2 = Option D (was: Option C default).

These updates are appended in a separate commit as part of synthesizing the spike outcome.

---

## 8. References

- `docs/research/2026-05-24-flui-geometry-polish-pass-research.md` Part VIII (the spike scope and decision rule it implements)
- euclid 0.22 docs (https://docs.rs/euclid/0.22)
- kurbo 0.13 docs (https://docs.rs/kurbo/0.13)
- mint 0.5 docs (https://docs.rs/mint/0.5)
- Spike session logs: `~/.pi/agent/sessions/--C--Users-vanya-RustroverProjects-flui--/2026-05-25T03-26-08-328Z_019e5d2b-21c8-7b17-b18b-d023c899b410/7f766d51/run-1/session.jsonl`

---

[← Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md) · [← Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md)
