# ADR-0077: Migrate from cosmic-text to parley

- **Status:** Proposed — precondition 1 met 2026-09-26 (recorded in
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md), Context); absorbed by ADR-0092, which
  supersedes it on acceptance
- **Date:** 2026-09-22
- **Supersedes (on acceptance):** ADR-0016, ADR-0059

Backed by a research spike: [`docs/research/text-stack-2026.md`](../research/text-stack-2026.md),
code in `tools/text-spike/` (standalone crate, not a workspace member). Nothing in `crates/flui-*`
has changed as part of this ADR; the migration is separate work. Until ADR-0092 is accepted,
ADR-0016 and ADR-0059 stay in force.

## Context

`flui-painting` depends on cosmic-text 0.19.0 for all text shaping and layout today
(`crates/flui-painting/Cargo.toml`; the module's own doc comment: "Text shaping and layout
over cosmic-text"). Per-realm font ownership, BiDi, and further editor text work all build on
whichever shaping stack FLUI settles on.

The spike ran the same six original corpora (Latin, pure-RTL Arabic, bidirectional
Arabic+Latin, CJK, ZWJ emoji, Devanagari) through both cosmic-text 0.19.0 and parley 0.11.1 (the
real latest published version — confirmed via `cargo info parley`, not assumed) at both a single
paragraph and a 10,000-line document, 5 reps each, in `--release` mode on an otherwise idle
machine, with parley shaping one `Layout` per paragraph (matching cosmic-text's own internal
per-paragraph shaping and matching how real parley consumers -- Xilem/Masonry, Blitz -- use it).
**The spike measured shaping and layout only.** It did not touch glyph rasterization or the engine
atlas -- see "Reconciling with ADR-0059" below for why that specifically matters here.

## Reconciling with ADR-0059

[ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md) (Accepted, 2026-09-06) already asked "migrate to
parley now?" and rejected it, on cost, for one dominant reason: no wgpu-side equivalent of glyphon
existed for parley, so a migration was "a text swap plus a hand-rolled renderer." Its own explicit
re-open trigger: *"A parley-compatible wgpu glyph atlas reaches maturity."*

**Half of that objection is already gone**, independent of anything this spike measured:
[ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (Accepted, landed 2026-09-18 -- twelve days after
ADR-0059, before this spike started) removed glyphon entirely. The engine now owns its own glyph
atlas and consumes shaper-agnostic `GlyphImage` bitmaps through one door,
`SharedFontSystem::rasterize(GlyphKey) -> Option<GlyphImage>` -- it no longer takes a
cosmic-text-specific buffer at all. A parley-shaped run reaching the engine as glyph instances is
no longer blocked on "does parley have its own glyphon."

**The other half is still open, and this spike did not close it.** `rasterize()`'s current
implementation (`crates/flui-painting/src/text_layout/layout.rs:346-366`) is itself deeply
cosmic-text-specific: `GlyphKey` is `pub(super) cosmic_text::CacheKey` (`glyphs.rs:23`), and
rasterization goes through a `cosmic_text::SwashCache`'s `get_image_uncached`. Checking
`tools/text-spike/`'s own dependency tree against this: **parley 0.11.1 does not rasterize glyphs
at all** -- `cargo tree -p parley` has `skrifa` (font parsing/outline extraction) but no `swash` or
`zeno` (the crates that actually scan-convert outlines to bitmaps); cosmic-text pulls both. Parley
deliberately leaves rasterization to the consumer (this is normal for shaping-only crates -- Xilem/
Masonry pair parley with a separate renderer). So a migration needs a rasterizer brought in
alongside parley -- plausibly `swash` directly, unbundled from cosmic-text, since flui-painting
already knows how to drive it -- plus a new `GlyphKey` that doesn't assume a
`cosmic_text::CacheKey`, and confirmation that whatever glyph/cluster identity parley exposes can
build a stable one. **None of this was prototyped or measured by this spike.** It is the reason
this ADR challenges ADR-0059 rather than declaring it superseded, and it is the first, required
precondition below.

## What the spike found (shaping and layout only)

**parley wins on all six corpora at document scale**: 2.4x-9.0x faster than cosmic-text on timing,
and 1.2x-4.2x less peak memory once measured with a retention policy and font-generic request that
match cosmic-text's own (see "Evidentiary basis" below for why those qualifiers matter), with line
counts matching exactly on all six corpora and glyph counts matching on four of six (an implicit
correctness check; `arabic_mixed` differs by 0.25% and `emoji_zwj` by 3.19%, both real
clustering/selection differences worth a closer look before shipping). This addresses ADR-0059's
cost argument on the shaping side specifically -- it says nothing about the rasterization gap
above.

Migration surface for the shaping/layout side is low: cosmic-text usage is confined to three files
under `crates/flui-painting/src/text_layout/` (`layout.rs`, `font_resolve.rs`, `glyphs.rs` --
`mod.rs` itself doesn't mention it) plus two files at the crate root (`fonts.rs`, which directly
accepts and mutates a `cosmic_text::fontdb::Database` to register loaded fonts, and `lib.rs`,
which re-exports `cosmic_text::fontdb::Family`) -- five files total, matching the spike report's
own occurrence count. Five more files are test-only (`crates/flui-painting/tests/`), plus one
test-only file in `flui-engine` (`paragraph_readback_tests.rs`). `flui-widgets` never touches
cosmic-text directly.

## Evidentiary basis (what to check before trusting this ADR's numbers)

This spike went through rounds of methodology correction before its numbers stabilized --
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
   flipped from "27x slower than cosmic-text" to "faster."
2. **Memory, round 2**: the *corrected* per-paragraph timing pass's memory numbers were unfair in
   the opposite direction -- parley's per-paragraph loop dropped each `Layout` immediately after
   reading its counts, while cosmic-text's single `Buffer` call retains every paragraph's shaped
   state simultaneously by construction. That produced a spurious 30-38x memory-advantage number
   (~22 MB vs 620-770 MB). Fixed by retaining every built `Layout` in a `Vec` held alive until peak
   RSS is sampled (`ParleyBackend::shape_per_paragraph_retained`), the same retention cosmic-text
   has by default. The real memory advantage, once both backends retain the same amount of state,
   narrowed substantially from 30-38x -- see round 3 below for the figure that stood after both
   memory-fairness rounds landed together.
3. **Font-selection and timing-boundary parity, round 3**: review caught two more
   asymmetries. Parley was requesting the CSS generic `"system-ui"` while cosmic-text requested
   `Family::SansSerif` -- different generics that can select different fonts, confounding font
   choice with the backend comparison. And cosmic-text's `shape()` dropped its `Buffer` (destroying
   hundreds of MB) *inside* the timed window, while parley's retained `Layout`s were dropped after
   both the timing and RSS samples -- an inconsistent destruction boundary. Both fixed: parley now
   requests `GenericFamily::SansSerif` to match, and both backends expose a `shape_retained`
   variant that returns their shaped state so the caller controls exactly when it's dropped,
   relative to both samples, on both sides. This round also changed which corpora's glyph counts
   agree between backends (`arabic_mixed` newly disagrees by 0.25%; `emoji_zwj`'s gap changed from
   1.59% to 3.19%) -- font selection turned out to affect work-parity, not just timing/memory. The
   timing (2.4x-9.0x) and memory (1.2x-4.2x) ranges in "What the spike found" above are from this
   third, fully-corrected pass.
4. Every round is kept in full in the spike's own report (not just this ADR's summary), including
   the numbers that got walked back, since a large finding that later gets corrected is exactly the
   kind of thing a reader re-deriving this decision needs to see, not just the final conclusion.

The underlying `Item`-boundary behavior from round 1 is real and worth filing upstream as a
low-priority "worth knowing" report (draft in the spike's own report) since a consumer that doesn't
paragraph-bound its input could still hit it -- but FLUI's own architecture (one `Layout` per
`RenderParagraph`) was never going to hit it, so it is not a reason to avoid parley.

Full measurement tables, both scaling studies, the profiling/symbolication method, the
init-vs-shaping memory breakdown, the work-parity table, and the upstream issue draft are in
`docs/research/text-stack-2026.md`, along with the checked-in runner script and raw per-rep dataset
behind every table (not just the aggregated numbers) so the medians and ratios can be independently
recomputed.

## Decision

**Direction: migrate `flui-painting` from cosmic-text to parley** for shaping and layout, shaping
one `Layout` per paragraph / `RenderParagraph` (never a whole multi-paragraph document as a single
`Layout` -- see Context). **This is not yet an authorization to start the migration.** The
following preconditions are gates, not activities -- each names what must be true, and how that
gets checked, before the corresponding piece of work is considered satisfied:

1. **Rasterization prototype (blocking; see "Reconciling with ADR-0059").** Build a minimal
   parley-shaped-glyph → bitmap path (parley shaping + a chosen rasterizer, plausibly `swash`
   directly) and a `GlyphKey` that doesn't assume `cosmic_text::CacheKey`. Pass condition: the
   prototype produces `GlyphImage`s the existing `flui-engine` atlas (`ADR-0067`) accepts
   unmodified, for at least one Latin and one complex-script (Devanagari or Arabic) test case, with
   a stable `GlyphKey` across repeated rasterization of the same glyph (a correctness property the
   engine's atlas cache depends on). Until this exists, this ADR supersedes neither ADR-0016 nor
   ADR-0059.
   **Met 2026-09-26:** swash driven directly, key on blob identity; see ADR-0092 §5 and Context.
2. **`arabic_mixed` (0.25%) and `emoji_zwj` (3.19%) glyph-count differences.** Pass condition:
   either (a) a test asserting parley's cluster/selection boundaries match FLUI's expected
   grapheme-cluster boundaries for fixed ZWJ and bidi-mixed corpora (family, couple+heart,
   profession, flag, skin-tone for emoji; an Arabic+Latin mixed string for the bidi case), or (b) a
   written, reviewed decision that each difference is immaterial, with the specific clusters that
   differ enumerated.
3. **Selection-rect and offset↔cursor conversion.** `flui-painting` builds this itself on top of
   cosmic-text today (`measure.rs`); neither backend exposes editor selection semantics natively,
   so this is a rewrite regardless of stack. Pass condition: the existing `flui-painting` test
   suite's selection/cursor tests (the ones exercising `measure.rs`'s current logic) pass unchanged
   in behavior against the parley-backed reimplementation, for at least LTR, RTL, and mixed-BiDi
   cases.
4. **BiDi visual-order, UAX #14 line breaking, variable-font support, wasm32 buildability.** None
   were measured in the spike. Pass condition: a conformance case per area -- a fixed bidi_mixed
   test string with an expected visual glyph order, a fixed line-breaking test string with expected
   break points compared against cosmic-text's current output, one variable font exercising at
   least the weight axis, and `cargo build --target wasm32-unknown-unknown` succeeding for
   whichever crate ends up depending on parley.
5. **flui's actual (filtered) cosmic-text initialization vs the spike's plain `FontSystem::new()`.**
   `font_resolve.rs`'s custom `Database` + `EmojiForbiddenFallback` were not reproduced in the
   spike, which used cosmic-text's default, unfiltered fallback for a stack-vs-stack comparison.
   Pass condition: re-run the spike's init-RSS measurement (or an equivalent) against flui's actual
   initialization path; the spike's data suggests init cost is small for either strategy, but that
   is a prediction to verify, not a result already in hand.
6. **File the upstream `Item`-boundary issue** (draft in the spike's report) so it's tracked
   independently of FLUI's own migration timeline.

Preconditions 2-6 can proceed in parallel with each other. Precondition 1 is the one this ADR
treats as blocking: without it, "migrate" is not a fully-costed decision, since ADR-0059's
dominant cost concern was never really about shaping speed.

**The font system becomes per-realm, not a process global.** ADR-0016's one shared font source
for measuring and painting stays; what changes is its scope. Each `UiRealm` owns its font context
(parley's `FontContext`/`LayoutContext`, plus the rasterizer's cache) as owner-local state,
reached through the realm rather than through the `FONT_SYSTEM` static, so layout on one realm
never observes another realm's in-flight `register_font`. Both stacks need only `&mut` access to
their own context with no internal locking, so this is not blocked on the shaper choice, but it
lands with the migration rather than as a second rewrite of the same surface. How fonts every
realm should see (the bundled baseline, application-registered faces) reach each realm's context
is settled with the implementation; sharing one mutable database across realms is not an option.

## Consequences

- `crates/flui-painting/src/text_layout/layout.rs`, `font_resolve.rs`, and `glyphs.rs` get
  rewritten against parley's `Layout<B>`/`Line`/`GlyphRun` API in place of cosmic-text's
  `Buffer`/`LayoutRun`; `crates/flui-painting/src/fonts.rs` (font registration, currently mutates a
  `cosmic_text::fontdb::Database` directly) needs an equivalent registration path against whatever
  parley/fontique or a standalone rasterizer needs; `crates/flui-painting/src/lib.rs`'s
  `pub use cosmic_text::fontdb::Family` — the only cosmic-text type on `flui-painting`'s own public
  surface — is replaced by an equivalent font-family selection type. `measure.rs`'s selection-rect
  and offset↔cursor conversion logic needs re-deriving against parley's API (precondition 3 above),
  not a drop-in swap.
- `crates/flui-painting/src/text_layout/glyphs.rs`'s `GlyphKey` and
  `crates/flui-painting/src/text_layout/layout.rs`'s `rasterize()` need a parley-compatible
  rasterization path (precondition 1) -- this is new work, not a rename of existing cosmic-text
  calls, since parley does not rasterize glyphs itself.
- `crates/flui-painting/Cargo.toml`'s single `cosmic-text = { version = "0.19" }` line is replaced
  by `parley` plus whichever rasterizer precondition 1 settles on; `crates/flui-engine`'s 5
  test-only occurrences (`paragraph_readback_tests.rs`) get updated to match.
- The `FONT_SYSTEM` static and `flui_painting::shared_font_system()` give way to a realm-owned
  font context; `SharedEngineServices` stops constructing a process-wide instance, and the
  cross-realm `register_font` staleness window ADR-0016 accepts disappears.
- Every call site in `flui-painting/src/text_layout/` must shape at the paragraph granularity
  (one `Layout` per `RenderParagraph`), never hand parley a whole multi-paragraph buffer as one
  `Layout` -- the Context section's `Item`-boundary behavior makes that the one usage pattern to
  actively avoid. This should be a natural fit for FLUI's render-object-per-paragraph model, not a
  constraint fought against it, but is worth stating as an explicit implementation rule so a future
  contributor doesn't accidentally reintroduce the single-`Layout` pattern for, say, a
  virtualized-list-of-paragraphs optimization.
- Precondition 1 is satisfied. This record is not accepted on its own: ADR-0092 carries it and
  adds `Superseded-by` here on acceptance.

## If later Rejected

cosmic-text 0.19.0 stays FLUI's text stack, and the shaping/layout performance and memory gap this
spike found across all six corpora stands as a known, unclaimed opportunity for whoever revisits
this ADR -- most likely worth reopening if the rasterization prototype (precondition 1) turns out
cheaper than expected, or if cosmic-text's shaping performance becomes a measured bottleneck in a
real FLUI app.
