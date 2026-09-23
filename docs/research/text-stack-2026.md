# Text stack spike 2026: parley vs cosmic-text

Status: measurements complete, through three rounds of methodology
correction, each caught by review before the last. **Recommendation:
migrate to parley for shaping/layout**, used the way real text UIs use it
(one `Layout` per paragraph/`RenderParagraph`, not one per document) --
but this is a direction, not yet a green light: the migration also needs
a glyph-rasterization bridge this spike never tested, and it challenges
(does not yet supersede) an existing Accepted ADR that rejected this same
migration once already, for a reason a *later*, unrelated ADR has
since partly resolved. See Measurements, the Risk section, and
Recommendation below, and the attached ADR's "Reconciling with ADR-0059"
section, before treating this as settled.

Round 1: an initial pass compared timing unfairly (parley shaping an
entire 10,000-line document as one `Layout` vs cosmic-text's own
paragraph-bounded internal shaping), finding a dramatic, real, but
usage-pattern-specific quadratic cost in parley; the corrected
per-paragraph timing comparison resolved it and shows parley winning on
timing across all six corpora. Round 2: the memory comparison from that
same corrected pass was *itself* unfair in the opposite direction (parley
dropped each paragraph's `Layout` immediately, cosmic-text's `Buffer`
retains everything) -- fixed by retaining every built `Layout`
(`shape_per_paragraph_retained`) the same way cosmic-text's `Buffer`
does. Round 3: a further review found the corrected comparison was still
requesting different font generics per backend (`"system-ui"` vs
`Family::SansSerif`) and had an inconsistent shaped-state destruction
boundary relative to the timer -- both fixed, and the full 240-point
series re-run a third time; every table in this report reflects that
final pass. All rounds are kept in this report, including the numbers
that got walked back; the mechanism behind the single-`Layout` case is
real, source-confirmed, and worth an upstream note, just not a blocker
by itself. Spike code lives in `tools/text-spike/` (standalone crate, not
a flui workspace member -- see its `Cargo.toml`'s empty `[workspace]`
table). Nothing in `crates/flui-*` changed for this spike.

## Why

FLUI's text stack decision affects three other in-flight tracks: B1
(FontSystem-per-realm), B3 (BiDi), and the B2 continuation (further editor
text-editing work). `flui-painting` depends on cosmic-text 0.19.0 today.
This spike asks whether parley (the linebender/Bevy-adjacent stack, built on
fontique + swash/harfrust) is a better long-term foundation, using the same
corpora and workload shape flui's own text editing needs to handle.

## Method

Two backends, one CLI harness (`tools/text-spike/src/main.rs`), six original
corpora, four axes:

- **Backend**: `cosmic-text` (exact-pinned `=0.19.0`, matching
  `crates/flui-painting/Cargo.toml`'s resolved version) vs `parley`
  (exact-pinned `=0.11.1` — the real latest published version, confirmed via
  `cargo info parley`; an earlier draft of this spike guessed `"0.7"`, which
  was stale by 4 minor releases and never actually run).
- **Corpus**: `latin`, `arabic` (pure RTL), `arabic_mixed` (bidirectional,
  inline Latin fragments to exercise UAX #9 visual reordering), `cjk`
  (Chinese + Japanese + Korean), `emoji_zwj` (ZWJ sequences, skin-tone
  modifiers, regional-indicator flags, keycaps), `devanagari` (conjuncts and
  vowel signs / matras). All six are original short paragraphs, not copied
  from any real source.
- **Size**: `paragraph` (the corpus text as-is) vs `large` (the same
  paragraph repeated one-per-line to exactly 10,000 lines).
- **Cache**: `cold` (fresh backend instance, one shape call) vs `warm`
  (backend instance shapes once first, discarding that timing, then the
  timed shape runs against the same instance — measures each backend's own
  warm-cache/fallback-cache behavior, not OS-level caching).
- **parley shaping mode** (`--parley-mode`, parley only): `per-paragraph`
  (the CLI default and the number used everywhere in this report unless
  stated otherwise) splits `large`'s text on `\n` and builds one `Layout`
  per non-empty paragraph, reusing the same `FontContext`/`LayoutContext`
  across paragraphs -- matching cosmic-text's own internal behavior
  (`Buffer::set_rich_text_impl` splits on `BidiParagraphs` and shapes each
  line independently) and matching real parley consumers (Xilem/Masonry,
  Blitz) and FLUI's own planned one-`Layout`-per-`RenderParagraph`
  architecture. `single` hands the *entire* `large` document to one
  `ranged_builder`/`build` call -- not a realistic UI usage pattern, kept
  only as an explicit "what if you don't paragraph-bound it" data point
  because it is what exposed the Risk section's finding.

Each of the 6×2×2×2 = 48 combinations was run 5 times in `--release` mode
(`CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0`), while holding the shared dev
machine's compile slot and with no other worker's build running concurrently
(both required for the numbers to be comparable — see
`.rust-studio/specs/b7-text-stack-spike/plan.md` §6).

### Reproduction

For a single measurement:

```bash
cd tools/text-spike
cargo build --release
./target/release/text-spike --backend cosmic-text --corpus latin --size paragraph --cache cold
./target/release/text-spike --backend parley       --corpus latin --size large     --cache warm
# parley's single-Layout-for-the-whole-document mode (Risk section only):
./target/release/text-spike --backend parley --corpus devanagari --size large --cache cold --parley-mode single
```

For the full 240-point series behind every table in this report:

```bash
cd tools/text-spike
cargo build --release
scripts/run_measurements.sh results/measurements.jsonl   # ~10-15 min, idle machine
python3 scripts/aggregate.py results/measurements.jsonl   # medians/ratios per table
```

`results/measurements.jsonl` (checked into this PR) is the exact raw
dataset every table in this report was generated from -- run
`aggregate.py` against it directly to recompute the tables without
re-running the (much slower) measurement pass, or re-run
`run_measurements.sh` on an idle machine to regenerate it from scratch
and confirm the numbers reproduce.

Each invocation prints one JSON object (`backend`, `corpus`, `size`,
`cache`, `parley_mode`, `elapsed_ms`, `init_rss_bytes`, `peak_rss_bytes`,
`line_count`, `glyph_count`) to stdout -- `init_rss_bytes` is sampled
right after backend construction (before any shaping), `peak_rss_bytes`
after shaping, so shaping-attributable memory is the difference between
the two (see "Memory: a second, fairer comparison" below for why both are
reported separately). `--corpus` must be one of the six names above
(validated at startup); `--size large` defaults to 10,000 repeated lines
(`--large-lines` to override).

## Measurements

240 of 240 planned data points (48 combinations × 5 reps), checked in at
`tools/text-spike/results/measurements.jsonl` (raw, one JSON object per
rep) and reproducible with `tools/text-spike/scripts/run_measurements.sh`
+ `scripts/aggregate.py` -- this is the **third** full pass, after a
Codex review caught two more asymmetries the second pass still had (see
"Evidentiary basis" in the ADR, and the note after the large-size table
below): parley was requesting a different CSS font generic than
cosmic-text, and the two backends dropped their shaped state on opposite
sides of the timer. Numbers below are post-fix; every earlier pass's
numbers are superseded by this section, not just supplemented.

### `--size paragraph` (single short paragraph, ~150-450 chars)

Every paragraph-size run is single-digit-to-sub-millisecond regardless of
backend or corpus — not decision-relevant on its own, included for
completeness. Median of 5 reps, `elapsed_ms`:

| Corpus | cosmic-text cold | cosmic-text warm | parley cold | parley warm |
|---|---:|---:|---:|---:|
| latin | 7.29 | 0.43 | 6.11 | 0.08 |
| arabic | 6.92 | 0.32 | 6.08 | 0.14 |
| arabic_mixed | 7.61 | 0.46 | 6.61 | 0.12 |
| cjk | 7.48 | 0.49 | 6.20 | 0.07 |
| emoji_zwj | 7.07 | 0.33 | 6.21 | 0.10 |
| devanagari | 7.22 | 0.34 | 6.58 | 0.13 |

Warm-cache paragraph shaping is consistently faster for parley than
cosmic-text at this size. Both backends are constructed *before* the
timer starts in `main.rs` (see its module docs), so `elapsed_ms` never
includes `ParleyBackend::new()`/`CosmicBackend::new()` themselves --
what the cold/warm gap reflects is the *first* shape call doing lazy
font-discovery and internal cache population that later calls on the
same backend instance don't repeat, not backend construction (a Codex
review caught an earlier, incorrect version of this sentence that
conflated the two).

### `--size large` (10,000 lines, one `Layout` per paragraph) — timing

Median of 5 reps, `parley` in `per-paragraph` mode (the realistic
comparison), both backends' `shape_retained` variants used so destruction
happens after this sample on both sides (see this section's note below):

| Corpus | cosmic-text cold (ms) | parley cold (ms) | speedup |
|---|---:|---:|---:|
| latin | 4255 | 781 | **5.4x faster** |
| arabic | 3070 | 1258 | **2.4x faster** |
| arabic_mixed | 4474 | 961 | **4.7x faster** |
| cjk | 5120 | 566 | **9.0x faster** |
| emoji_zwj | 3083 | 782 | **3.9x faster** |
| devanagari | 3246 | 1075 | **3.0x faster** |

**parley wins on all six corpora** on timing when used the way a real UI
uses it: 2.4x-9.0x faster. Warm-cache numbers track cold within noise for
every corpus.

**Note on this table's history**: an earlier pass of this same
per-paragraph comparison reported 2.4x-8.2x -- close to, but not
identical to, the range above. Two more asymmetries surfaced by a Codex
review were fixed between that pass and this one: parley was requesting
the CSS generic `"system-ui"` while cosmic-text requested
`Family::SansSerif` (different generics, can select different fonts);
and cosmic-text's `shape()` dropped its `Buffer` -- destroying hundreds of
MB -- *inside* the timed window, while parley's retained `Layout`s were
dropped after the timer stopped. Both fixed (parley now requests
`GenericFamily::SansSerif`; both backends expose `shape_retained`, used
above, so destruction happens after the timer on both sides) and the full
240-point series re-run a third time; the numbers in this table are from
that corrected run, not the earlier one.

For contrast, the same `devanagari` `large` `cold` case in `single` mode
(handing the whole 10,000-line document to one `Layout` -- **not a UI
scenario**, see Risk section): **78,782 ms**, i.e. per-paragraph mode is
~73x faster than single-`Layout` mode for this corpus alone. That gap,
not the backend-to-backend comparison, is what the Risk section is about.

### Memory: a second, fairer comparison

The first pass's memory numbers (parley ≈22 MB vs cosmic-text 620-770 MB,
a 30-38x gap) were **methodologically unfair in a different way than the
timing numbers were**, caught by the same kind of review: cosmic-text's
single `Buffer::shape_until_scroll` call builds and holds *every*
paragraph's shaped state simultaneously before the harness ever samples
peak RSS. The original `shape_per_paragraph` loop built one `Layout`, read
its counts, and **dropped it** before the next paragraph -- so at no
instant did more than ~1 paragraph's data exist in memory, regardless of
document length. That is not "parley uses less memory to shape the same
document"; it's "the harness measured two different retention policies."
Peak RSS (`ru_maxrss`) is a process-lifetime high-water mark that never
decreases once reached, so this isn't fixed by measuring later -- the
measurement itself has to hold the same amount of state cosmic-text does.

**Fix**: `ParleyBackend::shape_per_paragraph_retained` (in
`tools/text-spike/src/parley_backend.rs`) keeps every built `Layout` in a
`Vec` that `main.rs` holds alive until after sampling peak RSS -- the
same retention cosmic-text's `Buffer` has by construction. Re-ran the full
240-point series with this fix. **Separately**, `init_rss_bytes` is now
sampled right after backend construction (before any shaping), so
font-database-load cost and shaping-attributable growth can at least be
told apart -- motivated by a (turned out to be wrong, see below)
hypothesis that cosmic-text's eager macOS system-font-database load
dominated its memory number.

Median of 5 reps, `--size large --cache cold`, all values in MB. This
table (like the timing table above) is from the third, font-generic- and
destruction-boundary-corrected pass -- the numbers below superseded an
earlier corrected-for-retention-only pass, which reported a 1.3x-4.9x
range close to, but not identical to, the one below:

| Corpus | cosmic init | cosmic peak | cosmic shaping delta | parley init | parley peak (retained) | parley shaping delta | delta ratio |
|---|---:|---:|---:|---:|---:|---:|---:|
| latin | 13.5 | 677.9 | 664.4 | 16.9 | 175.9 | 159.0 | **4.2x less** |
| arabic | 13.2 | 630.1 | 616.9 | 16.6 | 412.6 | 396.0 | **1.6x less** |
| arabic_mixed | 13.4 | 483.9 | 470.5 | 16.8 | 252.4 | 235.6 | **2.0x less** |
| cjk | 12.9 | 458.9 | 446.0 | 16.4 | 271.8 | 255.5 | **1.7x less** |
| emoji_zwj | 13.0 | 774.5 | 761.5 | 16.5 | 255.5 | 239.0 | **3.2x less** |
| devanagari | 14.1 | 517.4 | 503.2 | 17.6 | 438.2 | 420.7 | **1.2x less** |

**Caveat on the "shaping delta" columns** (a Codex review point): `ru_maxrss`
is a process-lifetime high-water mark that never falls, so
`peak - init` is the growth in that mark between the two samples, not a
precise "bytes retained by shaping" figure. If backend construction has
an internal transient spike above its own settled footprint, `init`
already reflects that spike (the mark can't fall back down before the
next sample), which would understate the later delta by an unknown,
possibly backend-dependent amount. Read "shaping delta" and "delta
ratio" below as an upper-bound-shaped proxy for shaping-attributable
memory, not an exact attribution -- the two raw high-water marks
(`cosmic peak`, `parley peak`) are the numbers with no interpretation
layered on; the deltas are a step further, useful as a directional
signal, not as a precise figure to cite on its own.

Two corrections to earlier assumptions, both from this table:

1. **Init cost is small and roughly equal for both backends** (~13-14 MB
   cosmic-text, ~16-18 MB parley) -- the hypothesis that cosmic-text's
   eager macOS system-font-database load was the dominant memory cost is
   **not supported** by this data. The gap is almost entirely in the
   shaping+retention delta, not initialization.
2. Once fairly measured (same retention policy, matching font generics,
   consistent destruction boundary), **parley still uses less memory on
   every corpus, but by a much more modest margin than the first,
   unfair-retention pass suggested**: 1.2x-4.2x less, not 30-38x less.
   `devanagari` and `arabic` -- two of the more complex-shaping corpora --
   show the smallest gap (1.2x, 1.6x); `latin` and `emoji_zwj` show the
   largest (4.2x, 3.2x).

**Caveat this spike did not resolve** (a Codex review point, correcting an
earlier, over-confident version of this paragraph): `flui-painting`'s
actual cosmic-text initialization is not plain `FontSystem::new()` -- it
wraps a custom, filtered `Database` and an `EmojiForbiddenFallback` (see
`crates/flui-painting/src/text_layout/font_resolve.rs`, and
`cosmic_backend.rs`'s own module doc-comment, which notes this spike
deliberately uses the *default* unfiltered fallback for a stack-vs-stack
comparison rather than flui's specific tuning). The small init-cost gap
this spike measured does not establish that flui's actual policy is
irrelevant to the comparison: `font_resolve.rs` rebinds generic families,
snaps unsupported weights, and filters emoji out of the fallback tail --
all of that changes *which* faces and fallback segments get selected
during shaping, which can change shaping time and retained glyph state
even when construction memory itself is negligible. This spike's numbers
should be read as a default-cosmic-text-vs-default-parley comparison,
not as validated against flui's production fallback policy. Reproducing
flui's exact filtering here would have meant re-implementing
`font_resolve.rs`'s logic inside a crate that deliberately has no
`flui-*` dependency -- left as an explicit, unresolved gap, not
dismissed.

Retention itself has a real, separate memory cost regardless of backend:
holding 10,000 paragraphs' worth of shaped `Layout`/`Buffer` state
simultaneously is not what a virtualized real UI would do (it would hold
roughly as many paragraphs as are visible, not the whole document) -- this
table answers "what does holding everything cost," which is the fair
backend-vs-backend question, not "what would a real scrolling editor's
memory footprint be."

### Work-parity check

For the `speedup`/memory numbers above to mean anything, both backends
need to have done comparable work. Checked via `line_count` and
`glyph_count` at `--size large --cache cold` (10,000-line corpora):

| Corpus | cosmic-text lines/glyphs | parley lines/glyphs | glyph count diff |
|---|---|---|---:|
| latin | 10,000 / 4,350,000 | 10,000 / 4,350,000 | 0.00% |
| arabic | 10,000 / 2,280,000 | 10,000 / 2,280,000 | 0.00% |
| arabic_mixed | 10,000 / 4,050,000 | 10,000 / 4,040,000 | **-0.25%** |
| cjk | 10,000 / 1,350,000 | 10,000 / 1,350,000 | 0.00% |
| devanagari | 10,000 / 1,750,000 | 10,000 / 1,750,000 | 0.00% |
| emoji_zwj | 10,000 / 2,510,000 | 10,000 / 2,430,000 | **-3.19%** |

`line_count` matches exactly on every corpus (no empty-line-skip
discrepancy despite `shape_per_paragraph` explicitly skipping empty `\n`
segments -- none of this spike's corpora happen to produce one, since
each is a single non-empty line repeated with `\n` separators and no
trailing newline). `glyph_count` matches exactly on four of six corpora
now (previously five of six -- see below for why that count changed with
the font-generic fix). Neither difference is a wrapping issue (both
report exactly 10,000 lines).

### Glyph-count cross-check

Both backends were run against every corpus at `--size paragraph` as a
sanity check before the timed series: `line_count` and `glyph_count`
matched exactly for `latin` (435 glyphs), `arabic` (228), `cjk` (135),
and `devanagari` (175). Two corpora did **not** match, and both changed
between the second and third measurement passes when the font-generic
fix (`"system-ui"` → `GenericFamily::SansSerif`, matching cosmic-text's
`Family::SansSerif` request -- see the timing table's note above) landed:

- `arabic_mixed`: cosmic-text 405 glyphs, parley 404 (-0.25%) -- new in
  this pass; the second pass (differing font generic) had these matching
  exactly, which in hindsight is itself a little suspicious given the
  two backends were requesting different fonts at the time.
- `emoji_zwj`: cosmic-text 251 glyphs, parley 243 (-3.19%) -- present in
  every pass so far, but the exact gap changed with the font fix too
  (247 in the second pass, 243 in this one).

Both are real differences in how the two shapers cluster or select
glyphs for these corpora, not harness bugs, and both are corpora where
"does it split/select the same way" needs a closer look (precondition 2
in the ADR) before trusting either backend's default behavior for
bidirectional or emoji-heavy editor content. That the font-generic fix
changed which corpora disagree, not just by how much, is itself a
reminder that font selection is not a side detail in this comparison --
it's load-bearing for even the "did both backends do the same work"
question, not just the timing/memory numbers.

## Risk (investigated, not a blocker): a real quadratic cost in single-`Layout` usage

**Status: root cause confirmed, both by profiling and by reading parley's
own source; impact confirmed bounded to a non-realistic usage pattern by
a follow-up per-paragraph re-measurement.** This section is kept in full
because the mechanism is real and worth understanding (and worth an
upstream note), even though it does not block adopting parley.

### How it was found

The first pass of this spike measured `large` (10,000 lines) by handing
the *entire* document to one `ranged_builder`/`build` call per backend.
cosmic-text's own `Buffer` shapes each `\n`-delimited paragraph
independently under the hood regardless (`set_rich_text_impl` splits via
`BidiParagraphs`), but parley's `build(text)` does not do this
automatically -- so this setup was, in effect, "cosmic-text shaping 10,000
small units" vs "parley shaping one 10,000-line unit," not a fair
backend-vs-backend comparison. That asymmetry produced a large, real, but
misleading result:

- `parley devanagari large single cold`: **isolated re-run** (single
  process, otherwise-idle machine, 120s-per-rep timeout since neither GNU
  `timeout` nor `gtimeout` exist on this macOS box, worked around with
  Python's `subprocess.run(timeout=...)`) -- 5/5 reps at **89.1-98.7s**.
  `warm` cache: **0/5 reps completed inside 120s.** (This specific
  isolated run predates the font-generic fix described in the timing
  table's note above -- parley was requesting `"system-ui"` here, not
  `GenericFamily::SansSerif`. The scaling study immediately below was
  re-run after that fix and confirms the same order of magnitude and the
  same quadratic shape, so the font choice does not explain this finding
  away.)
- cosmic-text's own `devanagari large` (identical corpus, identical 10,000
  lines): **~3.2-3.3s.**

### Scaling study (single-`Layout` mode): confirmed quadratic

```bash
for n in 1250 2500 5000 10000; do
  ./target/release/text-spike --backend cosmic-text --corpus devanagari --size large --cache cold --large-lines $n
  ./target/release/text-spike --backend parley       --corpus devanagari --size large --cache cold --large-lines $n --parley-mode single
done
```

Re-run after the font-generic fix (parley requesting `GenericFamily::SansSerif`,
matching cosmic-text, as in every other table in this report):

| Lines | cosmic-text (ms) | parley single-`Layout` (ms) | parley ratio vs previous row |
|---:|---:|---:|---:|
| 1,250 | 411 | 1,365 | — |
| 2,500 | 817 (1.99x) | 5,083 (**3.72x**) | |
| 5,000 | 1,642 (2.01x) | 19,973 (**3.93x**) | |
| 10,000 | 3,265 (1.99x) | 78,782 (**3.94x**) | |

cosmic-text: ~2.0x time per doubling, three doublings running — textbook
**O(n) linear**. parley single-`Layout`: ~3.7-3.9x time per doubling,
three doublings running — textbook **O(n²) quadratic**. (An earlier,
pre-font-fix run of this same scaling study showed 3.69x-4.07x per
doubling and 85,056 ms at 10,000 lines -- close to, and the same shape
as, the numbers above; the font choice changes the constant somewhat, not
the complexity class.)

### Root cause: profiled and symbolicated

```bash
cargo install samply --locked
samply record --save-only -o /tmp/devanagari-profile.json.gz -- \
  ./target/release/text-spike --backend parley --corpus devanagari --size large --cache cold --large-lines 5000 --parley-mode single
samply load /tmp/devanagari-profile.json.gz --no-open   # local symbol server on :3000
```

(The Firefox Profiler web UI itself couldn't be used here — this
session's browser is Safari/WebKit-based, and Safari refuses to import a
local profile from `profiler.firefox.com`, a documented limitation.
Worked around by fetching the recorded profile's raw
`frameTable`/`funcTable`/`stackTable` JSON directly from `samply load`'s
local symbol server and ranking leaf-frame addresses with a small Python
script, then symbolicating the hot addresses with
`atos -o ./target/release/text-spike -l 0x100000000 <addr>` against the
release binary, built with `debug = true` in its release profile from the
start specifically to make this possible.)

**Result: 95.1% of all 22,723 sampled leaf (self-time) stack frames are
inside `core::str::count::do_count_chars`**, a Rust standard-library
function that counts Unicode scalar values by scanning a string start to
end. Every hot address resolves to this same function, called from one
site: **`parley::shape::shape_item` (`mod.rs:474`)**, inside parley's own
shaping module.

### Mechanism, confirmed by reading parley's own source

Reading parley-0.11.1's vendored source
(`~/.cargo/registry/.../parley-0.11.1/src/shape/mod.rs`) rather than just
inferring from the profile:

- `shape_text` walks the input character by character and starts a new
  `Item` (parley's own unit of "one bidi+script+style run") only on a
  **script, bidi-level, or style change**, or an inline box at that byte
  index (`mod.rs:140-161`, the `break_run` checks). **There is no check
  for `\n` anywhere in that loop.** For a document that is uniformly one
  script, one bidi level, and one style throughout -- `latin.txt`,
  `arabic.txt` (pure RTL, no embedded Latin), and `devanagari.txt` among
  this spike's six corpora -- this means the whole document becomes
  **one `Item`**, regardless of how many lines it contains.
  **This does NOT apply to `arabic_mixed.txt` (deliberately alternates
  Arabic and Latin, crossing bidi levels), `cjk.txt` (mixes Han,
  Hiragana, Katakana, and Hangul, each a distinct `Script` value), or
  `emoji_zwj.txt` (mixes Latin text with emoji)** -- each of those
  corpora, by the same rule, produces multiple `Item`s even in
  single-`Layout` mode, naturally bounding the O(n²) cost this section
  describes (an earlier version of this paragraph incorrectly claimed
  every corpus became one `Item`; a Codex review caught it). The
  quadratic scaling study and profile below were run specifically
  against `devanagari.txt`, one of the three homogeneous corpora, and
  the claim in this section is scoped to that case, not generalized to
  all six.
- Within one `Item`, `shape_item` further splits into font-selection
  segments, breaking only when consecutive grapheme clusters resolve to a
  different font (`mod.rs:352-364`). Line 474's
  `item_text[..segment_start_offset].chars().count()` is computed
  relative to the current *Item's own start* (`text_range.start`,
  `mod.rs:315`/`338`) — for a single-item document that's the same as
  "start of the whole document," which is exactly why it only shows up
  when nothing bounds the item.
- Why Devanagari pays this cost so much harder than Latin at the same
  `Item` size was **narrowed but not fully pinned down**: the profile's
  `harfrust::hb::ot_shaper_indic::setup_syllables` entry (absent from
  every other corpus) confirms Devanagari's clusters route through the
  Indic-specific shaping path, consistent with more/smaller font-selection
  segments per character than Latin's -- each paying its own O(item length)
  rescan. Pinning the exact segment-count difference would mean
  instrumenting parley's own source, out of scope for a spike that
  changes nothing in flui and doesn't fork a dependency.

### Resolution: per-paragraph shaping is linear, confirmed by direct re-measurement

`tools/text-spike/src/parley_backend.rs` now has two modes:
`ParleyBackend::shape` (the single-`Layout` path above, kept only as an
explicit "not a UI scenario" data point) and
`ParleyBackend::shape_per_paragraph` (splits on `\n`, one `Layout` per
non-empty paragraph, reuses `FontContext`/`LayoutContext` across
paragraphs). Re-running both the isolated devanagari case and the full
scaling study in `per-paragraph` mode, after the font-generic fix:

- `devanagari large cold`, per-paragraph: **1.07s** (vs single-`Layout`'s
  78.78s at the same 10,000 lines, this section's own post-fix number —
  **~73x faster** just from paragraph-bounding the `Item`s).

| Lines | cosmic-text (ms) | parley per-paragraph (ms) | parley ratio vs previous row |
|---:|---:|---:|---:|
| 1,250 | 411 | 139 | — |
| 2,500 | 817 (1.99x) | 278 (2.00x) | |
| 5,000 | 1,642 (2.01x) | 543 (1.95x) | |
| 10,000 | 3,265 (1.99x) | 1,076 (1.98x) | |

parley's per-doubling ratio settles at ~1.97x by the third doubling —
**linear**, matching cosmic-text's own ~2.0x, once the fixed
backend-construction cost (bigger relative to total time at n=1,250,
explaining the noisier 2.37x/1.61x early ratios) is amortized. **The
quadratic behavior is fully confirmed to be an artifact of the
single-`Layout`-for-a-whole-document usage pattern, not a fundamental
Devanagari-shaping defect.** Peak RSS also drops from ~261-266 MB
(single-`Layout`) to ~23 MB (per-paragraph) for the same corpus.

### Conclusion: worth an upstream note, not a blocker

This does not block adopting parley for FLUI, because FLUI's own planned
architecture (one `Layout` per `RenderParagraph`) is exactly the
per-paragraph pattern that avoids the defect entirely -- it was never
going to hand parley a 10,000-line single blob in the first place. It is
still worth filing upstream, because the underlying behavior (`Item`
boundaries not tracking `\n`) is a general property of the crate that any
consumer handing parley a long, single-script, multi-paragraph string
without pre-splitting it would hit, not something unique to this spike's
synthetic stress test.

### Ready-to-file upstream issue text (linebender/parley)

Drafted here for a human to review and file -- this spike does not publish
anything to a repository outside flui itself. Lower priority than an
earlier draft of this report framed it as: FLUI's own conclusion is that
per-paragraph usage avoids this entirely, so file as a "worth knowing"
report, not a "this blocks adoption" one.

> **Title**: `Item` boundaries don't track paragraph breaks, causing
> quadratic-time shaping when a whole multi-paragraph document is shaped
> as one `Layout` (low priority: normal per-paragraph usage avoids this)
>
> **Summary**
>
> `shape_text`'s `Item` boundaries are driven solely by script/bidi-level/
> style changes (`src/shape/mod.rs`'s `break_run` checks), not by `\n` --
> so a single-script, single-style string spanning many paragraphs becomes
> one `Item` regardless of length when handed to a single
> `ranged_builder`/`build` call. Within that `Item`, `shape_item`
> (`mod.rs:474`) computes `item_text[..segment_start_offset].chars().count()`
> once per font-selection segment, relative to the `Item`'s own start --
> an O(n) rescan repeated roughly once per segment, i.e. O(n²) over the
> `Item`'s length. This is not hypothetical: shaping a synthetic
> 10,000-line Devanagari string as one `Layout` measured ~90s (vs ~3.2s
> for an equivalent-sized cosmic-text `Buffer`, which paragraph-bounds
> internally), confirmed via a 1,250/2,500/5,000/10,000-line scaling study
> (~4x time per doubling, three doublings running) and a `samply` profile
> (95% of self-time in `core::str::count::do_count_chars`, called from
> this one site). Splitting the same input into one `Layout` per paragraph
> (which is how Xilem/Masonry/Blitz-style consumers already use parley)
> avoids the issue entirely -- confirmed by re-measuring the same corpus
> at ~1.1s, linear with input size. Filing this because any consumer that
> doesn't pre-split by paragraph (or hits many paragraphs' worth of
> single-script text some other way) could still hit it.
>
> Devanagari appears to pay this cost much harder than Latin/CJK at the
> same `Item` size; the profile's `harfrust::hb::ot_shaper_indic::setup_syllables`
> entry (present for Devanagari only) suggests more/smaller font-selection
> segments for Indic-shaped text, but the exact mechanism wasn't
> instrumented further.
>
> **Minimal reproduction** (parley 0.11.1, `Cargo.toml`: `parley = "=0.11.1"`)
>
> ```rust
> use parley::layout::{Alignment, AlignmentOptions};
> use parley::style::{FontFamily, StyleProperty};
> use parley::{FontContext, Layout, LayoutContext};
> use std::time::Instant;
>
> fn shape(font_cx: &mut FontContext, layout_cx: &mut LayoutContext<()>, text: &str) {
>     let mut builder = layout_cx.ranged_builder(font_cx, text, 1.0, true);
>     builder.push_default(StyleProperty::FontSize(16.0));
>     builder.push_default(StyleProperty::FontFamily(FontFamily::from("system-ui")));
>     let mut layout: Layout<()> = builder.build(text);
>     layout.break_all_lines(None);
>     layout.align(Alignment::Start, AlignmentOptions::default());
> }
>
> fn main() {
>     const LINE: &str = "तेज़ भूरी लोमड़ी आलसी कुत्ते के ऊपर से कूद जाती है।\n";
>     let mut font_cx = FontContext::new();
>     let mut layout_cx = LayoutContext::new();
>     for n in [1250, 2500, 5000, 10000] {
>         // Handing the whole `n`-line document to ONE build() call is the
>         // slow path this issue is about; splitting `text` on '\n' and
>         // calling `shape` once per line avoids it (see summary above).
>         let text = LINE.repeat(n);
>         let start = Instant::now();
>         shape(&mut font_cx, &mut layout_cx, &text);
>         println!("{n} lines: {:?}", start.elapsed());
>     }
> }
> ```
>
> **Environment**: macOS (Apple Silicon, arm64), parley 0.11.1, parley's
> own MSRV 1.88.

### Dependency tree, license, MSRV

Measured via `cargo tree` and `cargo metadata --format-version=1` against
`tools/text-spike/Cargo.lock` (workspace total: 123 unique crates).

| | cosmic-text 0.19.0 | parley 0.11.1 |
|---|---|---|
| Subtree size (`cargo tree -p <crate>`, unique crates incl. itself) | 45 | 73 |
| Own crate license | MIT OR Apache-2.0 | Apache-2.0 OR MIT |
| Own crate `rust-version` | 1.89 | 1.88 |
| Max `rust-version` declared anywhere in the subtree | 1.89 | 1.88 |
| License families seen in the subtree | MIT/Apache-2.0 family, Zlib, one `Apache-2.0 OR GPL-2.0-only` dual license (`self_cell` 1.3.0 — Apache-2.0 satisfies it) | MIT/Apache-2.0 family, Zlib, ISC, Unicode-3.0, Unlicense OR MIT |

parley's subtree is ~1.6x the crate count of cosmic-text's (fontique's font
matching + harfrust's shaping + swash bring their own dependency chains).
Neither subtree has a copyleft-only license; both are safe for flui's
existing MIT/Apache-2.0 dual license. Both declare an MSRV under flui's own
1.97 workspace MSRV, so neither would force an MSRV bump.

### macOS system-font fallback: not actually checked

**Corrected** (a Codex review point): the earlier version of this
subsection claimed a nonzero, matching glyph count across backends
established that both "located working glyphs" via macOS system-font
discovery. It doesn't. A shaper that fails to find a real glyph typically
still emits a `.notdef`/"tofu" placeholder for the missing character, and
that placeholder still counts as one glyph in `glyph_count` -- so a
nonzero, even a matching, count is consistent with either backend having
silently substituted tofu for some or all of a script's glyphs. Neither
this spike's harness nor its corpora were designed to catch that: doing
so needs inspecting the resolved font/glyph IDs for `.notdef`, or
rasterizing and visually comparing output, neither of which this spike
did (`SwashCache` is constructed in `cosmic_backend.rs` but never
exercised). **This spike does not support any claim about macOS
fallback-quality parity between the two backends** -- listed in Gaps
below, not presented as a finding.

### BiDi, UAX #14 line breaking, variable fonts, editor-API surface

- **Editor-API surface**: parley 0.11.1's `Layout<B>` exposes `lines()` →
  `Line` → `items()` → `PositionedLayoutItem::{GlyphRun, InlineBox}`, with
  per-`GlyphRun` `glyphs()`/`positioned_glyphs()`, `baseline()`, `offset()`,
  `advance()`. cosmic-text's `Buffer::layout_runs()` → `LayoutRun` exposes
  glyphs plus `line_y` directly (flui-painting's own baseline computation
  already depends on this, per `crates/flui-painting/src/text_layout/layout.rs`
  comment "cosmic-text's `LayoutRun::line_y` IS the baseline"). Neither
  backend's cursor-position/selection-rect/affinity API was exercised in
  this spike; flui-painting builds selection-rect and offset↔cursor
  conversion itself on top of cosmic-text today (`measure.rs`), so a switch
  would mean re-deriving that logic against parley's `Line`/`GlyphRun` API
  rather than reusing an equivalent built-in.
- **Per-realm FontContext/FontSystem without a global mutex**: both
  backends require `&mut` access to their font context for shaping
  (`parley::FontContext`, cosmic-text's `FontSystem`) — neither is
  internally locked, so both are equally capable of being owned
  per-realm (one instance per `UiRealm`, no shared mutex) rather than
  process-global. This spike's own `ParleyBackend`/`CosmicBackend` structs
  each own their context directly, with no lock, as an existence proof for
  both. This directly informs B1: the choice between the two backends does
  not change B1's feasibility either way.
- BiDi visual order, UAX #14 line breaking, and variable-font support were
  not independently measured in this spike — see Gaps below.

### wasm32 buildability

Not checked in this pass — see Gaps below.

## Migration-cost estimate (cosmic-text → parley)

Grep counts against the actual current tree (`crates/flui-{engine,painting,widgets}`),
2026-09-22:

| Crate | `cosmic_text`/`cosmic-text` occurrences (src/) | Files (src/) | Occurrences (tests/) | Files (tests/) |
|---|---|---|---|---|
| `flui-painting` | 94 | 5 (`text_layout/layout.rs`, `fonts.rs`, `lib.rs`, `text_layout/font_resolve.rs`, `text_layout/glyphs.rs`) | 5 | 5 (`text_overflow_unit.rs`, `rich_text_example.rs`, `text_layout_pipeline.rs`, `text_painter_unit.rs`, `font_registration.rs`) |
| `flui-engine` | 5 | 1 (`paragraph_readback_tests.rs`, test-only) | 0 | 0 |
| `flui-widgets` | 0 | 0 | 0 | 0 |

Only one `Cargo.toml` in the whole workspace declares cosmic-text as a real
dependency: `crates/flui-painting/Cargo.toml:30`
(`cosmic-text = { version = "0.19" }`). `flui-widgets` never touches
cosmic-text directly — it only consumes `flui-painting`'s `TextLayout`
abstraction, so a backend swap is invisible to it by construction.
`flui-engine`'s 5 occurrences are confined to one test file exercising
paragraph readback, not production rendering code.

This means the real migration surface is confined to `flui-painting`:
three files under `src/text_layout/` (`layout.rs`, `font_resolve.rs`,
`glyphs.rs` -- the module's own doc comment already says "Text shaping
and layout over cosmic-text") plus `fonts.rs` and `lib.rs` at the crate
root, plus 5 test files — a contained, single-crate rewrite, not a
cross-workspace one. `flui-painting`'s public surface (the one thing
outside consumers see) is narrow: `pub use cosmic_text::fontdb::Family;`
in `lib.rs` is the *only* cosmic-text type that crosses the crate
boundary today (per that file's own comment, "the one cosmic-text type
on this crate's surface... cannot hold the result"), so a switch's blast
radius on downstream crates is that one re-export, not 94 call sites'
worth of API surface. **This count is shaping/layout/registration only**
-- `layout.rs` and `glyphs.rs` also carry `flui-painting`'s glyph
*rasterization* (`GlyphKey`, `SharedFontSystem::rasterize`), a distinct
piece of work this count doesn't separately size (see Recommendation).

## Recommendation

**Direction: migrate to parley for shaping and layout, using one `Layout`
per paragraph / `RenderParagraph`** -- the usage pattern this spike
measured as the winner, and the pattern FLUI's own architecture would
naturally use anyway (flui-widgets never touches cosmic-text/parley
directly; only `flui-painting`'s `text_layout` module would). **This is
not yet a green light to start the migration.** This spike measured
shaping and layout only; it did not test glyph rasterization, and a
prior, unrelated ADR ([ADR-0059](../adr/ADR-0059-flui-stays-on-cosmic-text.md),
Accepted) rejected exactly this migration once already, primarily on a
rasterization/glyph-atlas cost that this spike's numbers do not, by
themselves, resolve -- see "Reconciling with ADR-0059" in the attached
ADR before treating this as settled.

**Why the shaping/layout case is strong**: parley wins on all six
corpora at document scale on timing (2.4x-9.0x faster than cosmic-text
0.19.0) and, once measured with a fair apples-to-apples retention policy
and matching font-generic request (see "Memory: a second, fairer
comparison" and the timing table's note above), on memory too (1.2x-4.2x
less, smallest on the more-complex-shaping corpora, largest on
Latin/emoji_zwj). Line counts match on all six corpora; glyph counts
match on four of six (the parity table above -- `arabic_mixed` and
`emoji_zwj` differ, both real clustering/selection differences worth a
closer look before shipping, not correctness blockers on their own). Its
dependency subtree carries no copyleft-only license and declares an MSRV
comfortably under flui's own 1.97. Its per-realm `FontContext` ownership
story is exactly as viable as cosmic-text's `FontSystem` for B1.

**Why "migrate" is not yet the full picture**: this spike's own migration
scope (below) covers replacing the shaping/layout API surface -- it does
not include glyph rasterization, which today goes through a
cosmic-text-specific `SwashCache`/`CacheKey` path
(`crates/flui-painting/src/text_layout/{layout.rs,glyphs.rs}`,
per [ADR-0067](../adr/ADR-0067-engine-owned-glyph-atlas.md)'s
`rasterize()` door) that parley itself does not provide an equivalent
for -- parley's own dependency tree has no rasterizer (`cargo tree -p
parley` shows `skrifa` but neither `swash` nor `zeno`). ADR-0059's
dominant rejection reason (no wgpu-side glyphon equivalent for parley)
is arguably already resolved by ADR-0067's later, unrelated change (the
engine atlas is now shaper-agnostic) -- but the *specific* bridge from
parley's glyph identities to a working `GlyphKey`/rasterizer was never
built or measured here. Migration cost for shaping/layout alone is low:
the real dependency is one line in `crates/flui-painting/Cargo.toml`,
cosmic-text usage for shaping is confined to three files under
`crates/flui-painting/src/text_layout/` (`layout.rs`, `font_resolve.rs`,
`glyphs.rs`) plus `fonts.rs` (font registration) and `lib.rs` (one
re-exported type) at the crate root -- five files total -- plus five test
files. That estimate does not yet include whatever the rasterization
bridge costs, because that bridge hasn't been designed.

**Three rounds of self-correction went into these numbers, all caught by
review before this report's conclusion settled -- worth stating plainly
since a reader trusting only the final table wouldn't see the process**:

1. An earlier pass found what looked like a catastrophic (~27x,
   confirmed-quadratic) Devanagari-specific timing defect. It was real,
   but it was an artifact of a single-`Layout`-for-the-whole-document
   measurement setup, not of the Devanagari shaping itself --
   re-measured with one `Layout` per paragraph (the realistic usage, and
   the pattern this recommendation is *for*), the same corpus is faster
   than cosmic-text, linear with input size. The underlying
   `Item`-boundary behavior is real and worth filing upstream (draft
   attached above) as a "worth knowing, low priority" report, since a
   consumer that doesn't paragraph-bound its input could still hit it --
   but it is not a reason to avoid parley for FLUI, since FLUI's own
   architecture never would have hit it.
2. That same corrected pass's *memory* numbers (≈22 MB vs 620-770 MB, a
   30-38x gap) were unfair in the opposite direction: parley's
   per-paragraph loop dropped each `Layout` immediately, while
   cosmic-text's `Buffer` retains every paragraph's shaped state at
   once. Re-measured with parley retaining every `Layout` the same way,
   the gap narrowed substantially -- still in parley's favor everywhere,
   but the earlier 30-38x number should not be repeated or cited; it
   measured two different retention policies, not two shaping engines.
3. That retention-corrected pass still had two more asymmetries (a
   different font generic requested per backend; an inconsistent
   destruction boundary relative to the timer), caught by a further
   review and fixed by requesting matching generics and giving both
   backends a `shape_retained` variant. The numbers throughout this
   report are from the resulting third, fully-corrected pass; every
   number from an earlier pass mentioned above is superseded, not
   supplemented.

**Before committing to the migration itself** (out of scope for this
spike, which changes nothing in `crates/flui-*`; see the attached ADR's
Decision section for pass/fail criteria on each of these):

1. **Rasterization prototype** -- bridge parley's shaped glyphs to a
   working rasterizer and a non-cosmic-text-specific `GlyphKey`; confirm
   it produces `GlyphImage`s the existing engine atlas accepts. This is
   the one this report treats as blocking a full "migrate" decision.
2. Look closer at the `arabic_mixed` (405 vs 404) and `emoji_zwj` (251
   vs 243) glyph-count differences to understand whether they're
   clustering/selection choices FLUI needs to account for.
3. Re-derive `flui-painting`'s selection-rect and offset↔cursor conversion
   logic against parley's `Line`/`GlyphRun` API (neither backend exposes
   editor selection semantics natively, so this isn't a drop-in swap
   regardless of which stack is chosen).
4. Independently verify BiDi visual-order correctness, UAX #14 line
   breaking, variable-font support, and wasm32 buildability -- none were
   measured in this pass (see Gaps).
5. File the upstream `Item`-boundary issue (draft above) so it's tracked
   independently of FLUI's own migration timeline.
6. Verify whether flui-painting's actual, filtered cosmic-text
   initialization (`font_resolve.rs`'s custom `Database` +
   `EmojiForbiddenFallback`, vs this spike's plain, unfiltered
   `FontSystem::new()`) changes the shaping/timing comparison, not just
   the init-memory number -- the fallback policy affects which fonts and
   fallback segments get selected during shaping, which this spike's
   small init-RSS gap does not by itself rule out.

A draft ADR is attached:
[`docs/adr/ADR-0077-migrate-to-parley.md`](../adr/ADR-0077-migrate-to-parley.md)
(status: Proposed -- and, per the ADR itself, does not yet supersede
ADR-0059 until the rasterization precondition above is met).

## Gaps / not measured in this pass

- macOS system-font fallback quality was not established at all, not even
  at a "glyph presence" level: a matching, nonzero `glyph_count` is
  consistent with either backend silently substituting a `.notdef`/tofu
  placeholder for missing characters, since a placeholder glyph still
  counts. Checking this needs inspecting resolved glyph IDs for
  `.notdef`, or rendering and comparing output -- neither done here.
- BiDi visual-order correctness and hit-test/cluster-to-cursor mapping on
  `arabic_mixed.txt` were not independently exercised against either
  backend's actual reordering output in this pass.
- UAX #14 line-breaking conformance and variable-font axis support were not
  independently tested (no variable font was included in the corpora).
- wasm32 buildability was not checked in this pass.
- Peak-RSS (`libc::getrusage(RUSAGE_SELF).ru_maxrss`) measures the whole
  process's high-water mark, not backend-attributable allocation — a
  process-level upper bound, not a precise per-backend number.
- The exact reason Devanagari produces more/smaller font-selection
  segments per `Item` than Latin (Risk section) was narrowed to "the
  Indic shaping path" via profiling and source reading, but not pinned to
  an exact line without instrumenting parley's own source.
- The memory comparison retains *every* paragraph's shaped `Layout`/
  `Buffer` state simultaneously (10,000 paragraphs at once), for a fair
  backend-vs-backend number -- but that is not what a virtualized real
  editor would hold in memory (roughly as many paragraphs as are
  visible), so neither backend's absolute memory number here should be
  read as "what FLUI's memory footprint would be."
- `flui-painting`'s actual cosmic-text initialization (a filtered
  `Database` + `EmojiForbiddenFallback`, per `font_resolve.rs`) was not
  reproduced in this spike, which deliberately uses cosmic-text's plain,
  unfiltered `FontSystem::new()` for a stack-vs-stack comparison. The
  init-cost data here suggests this doesn't change much (init is small
  either way), but that wasn't independently confirmed against flui's
  actual initialization path.
