# ADR-0077: Migrate from cosmic-text to parley

Status: **Proposed** (2026-09-22). Backed by the B7 research spike:
[`docs/research/text-stack-2026.md`](../research/text-stack-2026.md), code in `tools/text-spike/`
(standalone crate, not a flui workspace member). Nothing in `crates/flui-*` has changed as part of
this spike or this ADR -- migration is a separate follow-up.

## Context

`flui-painting` depends on cosmic-text 0.19.0 for all text shaping and layout today
(`crates/flui-painting/Cargo.toml:30`; the module's own doc comment: "Text shaping and layout
over cosmic-text"). This decision affects three other in-flight tracks: B1 (FontSystem-per-realm),
B3 (BiDi), and the B2 continuation (further editor text-editing work) — all three build on
whichever shaping stack FLUI settles on.

The B7 spike ran the same six original corpora (Latin, pure-RTL Arabic, bidirectional
Arabic+Latin, CJK, ZWJ emoji, Devanagari) through both cosmic-text 0.19.0 and parley 0.11.1 (the
real latest published version — confirmed via `cargo info parley`, not assumed) at both a single
paragraph and a 10,000-line document, 5 reps each, in `--release` mode on an otherwise idle
machine, with parley shaping one `Layout` per paragraph (matching cosmic-text's own internal
per-paragraph shaping and matching how real parley consumers -- Xilem/Masonry, Blitz -- use it).

## What the spike found

**parley wins on all six corpora at document scale**: 2.4x-8.2x faster than cosmic-text on timing,
and 1.3x-4.9x less peak memory once measured with a retention policy that matches cosmic-text's
own (see "Evidentiary basis" below for why that qualifier matters), with line counts matching
exactly on all six corpora and glyph counts matching on five of six (an implicit correctness
check; `emoji_zwj` differs by 1.59%, a real ZWJ-clustering difference worth a closer look before
shipping). Migration cost is low: cosmic-text usage outside `flui-painting/src/text_layout/`
(5 files) is 5 test files and one 5-occurrence test file in `flui-engine`; `flui-widgets` never
touches cosmic-text directly; only one cosmic-text type (`Family`) crosses `flui-painting`'s own
public API today.

## Evidentiary basis (what to check before trusting this ADR's numbers)

This spike went through two rounds of methodology correction before its numbers stabilized --
recorded here because an ADR's "what the spike found" section is a claim later work builds on, and
a reader should be able to check the basis rather than take the headline number on faith:

1. **Timing, round 1**: the first timing comparison handed parley an entire 10,000-line document
   as one `Layout` while cosmic-text's `Buffer` shaped each paragraph independently by design --
   not a fair comparison, and it produced a spurious ~27x "Devanagari is catastrophically slower"
   result. It was real (profiled with `samply`, symbolicated with `atos`: 95.1% of self-time inside
   `core::str::count::do_count_chars`, called from `parley::shape::shape_item`, `mod.rs:474`; and
   confirmed via source reading -- `shape_text`'s `Item` boundaries are driven only by
   script/bidi-level/style changes, never by `\n`, `src/shape/mod.rs:140-161`, so a single-script
   document becomes one `Item` regardless of length, and `shape_item`'s per-segment offset
   computation is O(n²) over that `Item`), but it was an artifact of the single-`Layout` setup, not
   of Devanagari shaping. Fixed by shaping one `Layout` per paragraph for both backends' comparison
   (splitting on `\n`, confirmed linear via a 1,250/2,500/5,000/10,000-line scaling study);
   Devanagari flipped from ~90s (quadratic) to ~1.1s (linear), and the backend-vs-backend result
   flipped from "27x slower than cosmic-text" to "3.0x faster."
2. **Memory, round 2**: the *corrected* per-paragraph timing pass's memory numbers were unfair in
   the opposite direction -- parley's per-paragraph loop dropped each `Layout` immediately after
   reading its counts, while cosmic-text's single `Buffer` call retains every paragraph's shaped
   state simultaneously by construction. That produced a spurious 30-38x memory-advantage number
   (~22 MB vs 620-770 MB). Fixed by retaining every built `Layout` in a `Vec` held alive until peak
   RSS is sampled (`ParleyBackend::shape_per_paragraph_retained`), the same retention cosmic-text
   has by default. The real memory advantage, once both backends retain the same amount of state,
   is 1.3x-4.9x, not 30-38x -- smallest on the two most-complex-shaping corpora (Arabic,
   Devanagari), largest on Latin and emoji.
3. Both rounds are kept in full in the spike's own report (not just this ADR's summary), including
   the numbers that got walked back, since a large finding that later gets corrected is exactly the
   kind of thing a reader re-deriving this decision needs to see, not just the final conclusion.

The underlying `Item`-boundary behavior from round 1 is real and worth filing upstream as a
low-priority "worth knowing" report (draft in the spike's own report) since a consumer that doesn't
paragraph-bound its input could still hit it -- but FLUI's own architecture (one `Layout` per
`RenderParagraph`) was never going to hit it, so it is not a reason to avoid parley.

Full measurement tables, both scaling studies, the profiling/symbolication method, the
init-vs-shaping memory breakdown, the work-parity table, and the upstream issue draft are in
`docs/research/text-stack-2026.md`; the migration-cost numbers above are real `grep` counts against
`crates/flui-{engine,painting,widgets}` cited there, not re-derived here.

## Decision

**Migrate `flui-painting` from cosmic-text to parley**, shaping one `Layout` per paragraph /
`RenderParagraph` (never a whole multi-paragraph document as a single `Layout` -- see Context).

Before the migration itself starts (this ADR authorizes the direction; the migration is separate,
scoped follow-up work, not part of this spike):

1. Look closer at the `emoji_zwj` glyph-count difference (251 vs 247) to understand whether it's a
   clustering choice FLUI needs to account for.
2. Re-derive `flui-painting`'s selection-rect and offset↔cursor conversion logic against parley's
   `Line`/`GlyphRun` API -- neither backend exposes editor selection semantics natively, so this
   isn't a drop-in swap regardless of which stack was chosen.
3. Independently verify BiDi visual-order correctness, UAX #14 line breaking, variable-font
   support, and wasm32 buildability -- none were measured in the spike.
4. File the upstream `Item`-boundary issue (draft in the spike's report) so it's tracked
   independently of FLUI's own migration timeline.
5. Verify whether flui-painting's actual, filtered cosmic-text initialization (`font_resolve.rs`'s
   custom `Database` + `EmojiForbiddenFallback`, vs the spike's plain, unfiltered
   `FontSystem::new()`) changes the memory comparison's init-cost side -- the spike's data suggests
   init cost is small for either strategy, but that wasn't independently confirmed against flui's
   actual initialization path.

## Consequences

- `crates/flui-painting/src/text_layout/` (5 files) and its 5 test files get rewritten against
  parley's `Layout<B>`/`Line`/`GlyphRun` API in place of cosmic-text's `Buffer`/`LayoutRun`.
  flui-painting currently builds selection-rect and offset↔cursor conversion itself on top of
  cosmic-text (`measure.rs`); that logic needs re-deriving against parley's API, not a drop-in
  swap, since neither backend exposes editor selection/cursor semantics natively.
- The one public re-export, `pub use cosmic_text::fontdb::Family` in `flui-painting/src/lib.rs`,
  is replaced by parley's equivalent font-family selection type — the only cosmic-text-shaped
  break in `flui-painting`'s own public surface.
- `crates/flui-painting/Cargo.toml`'s single `cosmic-text = { version = "0.19" }` line is replaced
  by a `parley` dependency; `crates/flui-engine`'s 5 test-only occurrences
  (`paragraph_readback_tests.rs`) get updated to match.
- B1 (FontSystem-per-realm) is unaffected either way: both backends require only `&mut` access to
  their own font context, with no internal locking, so per-realm ownership is equally viable under
  either stack -- this decision doesn't gate B1.
- Every call site in `flui-painting/src/text_layout/` must shape at the paragraph granularity
  (one `Layout` per `RenderParagraph`), never hand parley a whole multi-paragraph buffer as one
  `Layout` -- the Context section's `Item`-boundary behavior makes that the one usage pattern to
  actively avoid. This should be a natural fit for FLUI's render-object-per-paragraph model, not a
  constraint fought against it, but is worth stating as an explicit implementation rule so a future
  contributor doesn't accidentally reintroduce the single-`Layout` pattern for, say, a
  virtualized-list-of-paragraphs optimization.

## If later Rejected

cosmic-text 0.19.0 stays FLUI's text stack, and the performance/memory gap this spike found across
all six corpora stands as a known, unclaimed opportunity for whoever revisits this ADR.
