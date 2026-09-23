# Plan — B7: spike Parley vs cosmic-text

Status: setup phase. Worktree `/Users/vanyastafford/Develop/flui-wt-b7-text-stack-spike/`, branch
`b7/text-stack-spike`, based on `origin/main` at `f85c615f`. Assigned by peer session "Master"
(Track B, size S, no dependencies). Not a framework change — decides FLUI's long-term text stack
(affects B1 FontSystem-per-realm, B3 BiDi, B2-continuation) without touching the framework itself.

## 1. Constraint

**"В фреймворке ничего не меняем"** — nothing in `crates/flui-*` changes for this task. All work
lives in a new standalone crate `tools/text-spike/` (NOT a flui workspace member — root
`Cargo.toml`'s `[workspace] members` is an explicit path array, so a new `tools/` crate needs an
empty `[workspace]` table of its own to detach from workspace resolution) plus
`docs/research/text-stack-2026.md`. If the report recommends a stack switch, attach a draft ADR
with status "Proposed" — that ADR is a proposal document, not an implementation.

## 2. Backends under test

- **parley** (latest; includes fontique) — version currently unverified. Placeholder
  `parley = "0.7"` in `tools/text-spike/Cargo.toml` is a guess; must be corrected via `cargo add
  parley` (or a registry lookup) once a compile slot is available. cratesio MCP is not connected
  this session (`ENOENT: cratesio-mcp` not on PATH) — cannot resolve the version that way; use
  `cargo add` under a slot, or WebFetch/WebSearch against crates.io/docs.rs if available before
  that.
- **cosmic-text** — pinned to the EXACT version flui-engine currently depends on: confirmed
  `0.19.0` (read from both `crates/flui-painting/Cargo.toml`'s `cosmic-text = { version = "0.19"
  }` and the root `Cargo.lock`'s resolved `version = "0.19.0"`). Pinned as `= "0.19.0"` in
  `tools/text-spike/Cargo.toml`.

## 3. Corpora (all in `tools/text-spike/corpora/`, all done)

Original, self-composed text (not copied from any real/copyrighted source), themed around "text
shaping is the foundation of a good UI":

- [x] `latin.txt` — English paragraph.
- [x] `arabic.txt` — pure RTL Arabic paragraph.
- [x] `arabic_mixed.txt` — bidirectional Arabic+English (inline Latin fragments like
  "الاسم: John Smith" to exercise UAX #9 visual reordering).
- [x] `cjk.txt` — mixed Simplified Chinese + Japanese + Korean sentences (covers the "CJK" corpus
  requirement; Korean included alongside since font-fallback quality is a required measurement).
- [x] `emoji_zwj.txt` — ZWJ sequences (family, couple+heart, kiss, rainbow flag, professions),
  skin-tone modifiers, regional-indicator flag pairs, keycap, plain emoji, mixed with plain text.
- [x] `devanagari.txt` — conjuncts and vowel signs (matras) to exercise complex shaping.

## 4. Required measurements

1. Shaping+layout time: one paragraph AND ~10k lines, cold cache AND warm cache, per corpus, per
   backend.
2. Peak memory (RSS) — planned via `libc::getrusage(RUSAGE_SELF).ru_maxrss` (units differ: bytes
   on macOS, KB on Linux; this spike only needs to run on macOS but note the unit in the report).
3. macOS system-font fallback quality (does each backend find working glyphs for Arabic/CJK/
   Devanagari/emoji without an explicitly bundled font).
4. BiDi: visual order correctness AND hit-test/cluster-to-cursor mapping on `arabic_mixed.txt`.
5. UAX #14 line breaking.
6. Variable-font support.
7. Editor-API surface: selection rects, point→cursor position, affinity (upstream/downstream) —
   compare what each backend exposes natively vs what flui would need to build on top.
8. Per-realm `FontContext`/`FontSystem` feasibility without a global mutex (directly informs B1).
9. License + dependency-tree analysis: `cargo tree` crate count, MSRV, license per crate.
10. wasm32 buildability (`cargo build --target wasm32-unknown-unknown` at minimum; note if either
    backend needs `getrandom`/thread/fs shims for wasm).

## 5. Deliverables

1. `tools/text-spike/` — standalone crate, CLI harness, both backends behind a common measurement
   interface, run against all 6 corpora.
2. `docs/research/text-stack-2026.md` — measurement table with exact reproduction commands
   (`cargo run -p text-spike -- --backend ... --corpus ... --size ... --cache ...`), a
   recommendation, and a migration-cost estimate for cosmic-text → parley (if recommended) derived
   from real `grep -rn cosmic_text` / `grep -rn cosmic-text` counts across `crates/flui-engine`,
   `crates/flui-painting`, `crates/flui-widgets` (not a guess — actual counts, cited).
3. Draft ADR (status "Proposed") only if recommending a switch.
4. PR containing only `docs/` + `tools/text-spike/` — qualifies for the lighter CI lane. Master
   triggers `@codex review` personally; I do not.

## 6. Slot discipline (binding, from Master + established session norms)

- Compilation is heavy (parley pulls in swash, fontique, etc.) — request a slot from Master before
  any `cargo add` / `cargo build` / `cargo run` / benchmark run.
- `CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0` when compiling.
- Own worktree, own `target/` — never set `CARGO_TARGET_DIR` (cross-worktree sharing is banned:
  cargo can silently link a wrong-worktree's rlib via matching relative-path fingerprints).
- Take TIMING measurements only while holding a slot AND the machine is otherwise idle (no other
  worker's build running concurrently) — a concurrent build would skew the benchmark numbers.
- `cargo metadata --offline` (lockfile-only resolution) does NOT require a slot — Master-confirmed
  exception ("резолюция, не компиляция").
- Local non-compiling gates (`rustfmt --check`, `just fmt-check text-check` if applicable to a
  non-member crate, reading/writing files) never need a slot.

## 7. Standing rule carried over from PR #1261 (still active, unrelated crate but same session)

If Codex or any reviewer surfaces a NEW P1 finding on #1261 while B7 is in progress, drop B7 and
fix the P1 in-PR first. Any NEW P2 finding on #1261 becomes a separate GitHub issue — **not** the
`area: gestures` label (that label does not exist in this repo and I do not create it; corrected
by Master 2026-09-22). Use existing labels instead: `bug` if it is a defect, `tech-debt`
otherwise, plus `testing` if the issue is about test coverage/quality. Link the originating Codex
comment in the issue body, then resolve the review thread with a reply linking the issue — not
another commit. Master merges #1261 personally once CI is green on `2bcc96f8`; I only report
green, I do not merge. (Background CI poll task `bpv1al8vf` for that commit is still running in
the other worktree — check it before reporting.)

## 8. Progress log

- 2026-09-22: worktree created, `tools/text-spike/Cargo.toml` written (cosmic-text pinned
  `0.19.0` confirmed; parley version placeholder, unverified), all 6 corpora files written.
- 2026-09-22: wrote `src/{main,corpora,metrics,cosmic_backend,parley_backend}.rs`.
  `cosmic_backend.rs` is grounded directly in `flui-painting`'s actual 0.19.0 usage (grepped from
  `crates/flui-painting/src/text_layout/layout.rs`), so it's a reasonably solid draft.
  `parley_backend.rs` is an UNVERIFIED best-effort draft — no flui crate uses parley today, so
  there's no in-repo call site to ground it against, and no compile slot was open to check it
  against the real API; its own doc comment flags this. rustfmt-clean
  (`rustfmt --edition 2021 --check tools/text-spike/src/*.rs` passes). Not yet done: no
  `cargo add`/build/run performed (no compile slot requested yet), `docs/research/text-stack-2026.md`
  not started, no measurements taken.
- 2026-09-22: slot granted (rustc 0). Confirmed `tools/text-spike` is NOT picked up by the root
  workspace (`members` is an explicit path list, no glob, no `tools/text-spike` entry). Real
  parley version found via `cargo info parley`: latest published is **0.11.1**, not the "0.7"
  guess (stale by 4 minor releases) -- Cargo.toml corrected to `parley = "=0.11.1"`. Verified
  `parley_backend.rs` against docs.rs/parley/0.11.1 (WebFetch, not memory) before touching code:
  found real API drift from the 0.7-era draft (`ranged_builder` gained a `quantize: bool` param;
  `StyleProperty::FontStack` no longer exists, replaced by `FontFamily`; `Layout::align` takes no
  `max_width`). Rewrote the file against the verified signatures, removed the UNVERIFIED marker.
  `cargo check` then found a second, unrelated bug in `cosmic_backend.rs` (copied flui-painting's
  `Buffer::new_empty`/`set_rich_text` calling convention, but `Buffer::new`/`set_text` -- what
  this spike actually uses -- do NOT take `&mut FontSystem`; only `shape_until_scroll` does).
  Fixed by reading the vendored cosmic-text-0.19.0 source directly. `cargo check`, `cargo check
  --all-targets`, and `cargo test` (3/3 corpora tests pass) all clean. `cargo build --release`
  clean (99 crates). Smoke-tested every corpus × both backends at `--size paragraph`: glyph
  counts match exactly for 5/6 corpora (latin/arabic/arabic_mixed/cjk/devanagari); `emoji_zwj`
  diverges (cosmic-text 251 vs parley 247 glyphs) -- a real shaper difference in ZWJ clustering,
  recorded in the report, not a harness bug. `--size large` (10k lines) smoke test on `latin`:
  cosmic-text 5.0s/781MB peak RSS vs parley 1.6s/398MB -- parley ~3.2x faster, ~half the memory,
  on this workload (single unverified sample, real series pending). Gathered (non-CPU-heavy)
  `cargo tree`/`cargo metadata` data: cosmic-text subtree 45 crates (MSRV 1.89), parley subtree 73
  crates (MSRV 1.88); no copyleft-only license in either subtree. Grepped real cosmic-text usage
  counts for the migration-cost section: flui-painting 94 occurrences/5 files (+5 test files),
  flui-engine 5 occurrences/1 file (test-only), flui-widgets 0 -- only
  `crates/flui-painting/Cargo.toml:30` declares the dependency. Wrote
  `docs/research/text-stack-2026.md` skeleton with all of the above filled in; the timing table
  itself is a placeholder pending the 5-rep series.
- 2026-09-22, mid-series incident: the first full 5-rep measurement run (background task
  `buggxl1ko`) hit the Bash tool's 600s foreground timeout and was auto-moved to background.
  Checking its output file afterward showed no new progress past "parley emoji_zwj large cold"
  and no matching process in `ps`, which I wrongly read as "the job died" (actually: its own
  progress logging had just stopped reaching that tracked output file — the process was still
  running fine). I wrote and launched a "resume" script for what looked like the missing tail
  combos. Both scripts ended up running **concurrently** for several minutes before I noticed via
  `ps` (two live `text-spike` release processes shaping in parallel) — this violates the
  machine-idle requirement and invalidated every timing number both processes touched during the
  overlap. Fix: killed both processes (`kill -9`; task notifications confirmed exit 137 for both),
  deleted the entire partial results file rather than trying to salvage a "clean prefix" (not
  worth the risk of silently keeping contaminated numbers), and restarted the *original*,
  complete, non-resumed script as a single `run_in_background: true` task (`bki8abs9k`) with
  nothing else running concurrently. **Lesson for next time**: don't infer a background job died
  from a stale/non-updating output file alone — check `ps` for the actual process before writing
  a "resume" script, since a quiet output file and a dead process are not the same thing, and two
  scripts racing on the same append-only results file is exactly the kind of contamination the
  slot-discipline rule (§6) exists to prevent.
- 2026-09-22, clean restart (task `bki8abs9k`, single process, nothing else running) reached
  237/240 points cleanly before Master asked to pause for Fable's #1260 slot (~10-15 min). Let the
  in-flight combo run rather than killing mid-rep; it turned out to be by far the slowest thing in
  the whole series: `parley devanagari large warm` ran 40s+ per rep (vs ~1-1.5s for every other
  `parley ... large` combo) and was still going when I killed it after Master's pause request, so
  that cell only has 2/5 reps (79.7s, 86.6s). The immediately preceding cell, `parley devanagari
  large cold`, DID finish cleanly (5/5 reps, 79.1-90.9s, tight band) *before* the pause request —
  a ~25x slowdown vs cosmic-text's own `devanagari large` (~3.2s) on the identical corpus.
  **Master's correction, important**: the `large warm` hang's timing coincided with when Fable's
  build slot request came in, so until there's a clean dedicated re-run, this is a HYPOTHESIS, not
  a confirmed finding — do not report it as fact. Plan once "можно продолжать": (1) re-run
  `parley devanagari large {cold,warm}` alone, 5 reps, 120s per-rep timeout; (2) if it reproduces,
  a scaling study on `devanagari` at ×1/×2/×4/×8 lines for both backends to distinguish linear vs
  quadratic growth; (3) if non-linear, profile with `samply` to find the hot function
  (harfrust? line breaking? fontique fallback?); (4) write it up as its own "Risk" section in the
  report with a reproduction command, not folded into the main measurement table. The 237 points
  already collected for every OTHER combo were gathered before this contamination window and
  stand as-is; only the two `devanagari large` cells (and the un-run scaling/profiling work) are
  blocked on the re-run.
- 2026-09-22, slot reopened, machine idle, Opus (A1 PR0) next in queue. Ran the full
  re-verification plan:
  1. Isolated re-run (task `bftblxzum`, single process, 120s-per-rep timeout via Python
     `subprocess.run(timeout=120)` since neither GNU `timeout` nor `gtimeout` exist on this
     macOS box): `cold` 5/5 reps at 89.1-98.7s (tight band) -- REPRODUCED, confirmed, not
     contamination. `warm` 0/5 reps completed -- every single rep exceeded 120s.
  2. Scaling study (task `bzptte7ss`, single process, `--large-lines` 1250/2500/5000/10000):
     cosmic-text held ~2.0x time per doubling (linear, textbook O(n)) the whole way; parley held
     ~3.7-4.1x time per doubling three doublings running (textbook O(n^2)). 8x the input cost
     cosmic-text 8.0x the time and parley 60.7x the time.
  3. Profiling: `cargo install samply --locked` (252 deps, ~1.3GB added to `tools/text-spike`'s
     own `target/`), `samply record --save-only` on `devanagari large cold --large-lines 5000`
     (a smaller, faster repro than the full 10k case). The Firefox Profiler web UI couldn't load
     the local recording (Safari/WebKit refuses local-profile imports from profiler.firefox.com,
     confirmed by trying it in this session's own browser pane) -- worked around by fetching the
     raw profile JSON directly from `samply load`'s local symbol server, parsing
     `frameTable`/`funcTable`/`stackTable` with a small Python script to rank leaf (self-time)
     addresses, then symbolicating the hot addresses with `atos -o ./target/release/text-spike -l
     0x100000000 <addr>` against the release binary (built with `debug = true` in its release
     profile from the start, which is what made this possible without a separate debug build).
     Result: 95.1% of all 22,723 sampled leaf frames are inside `core::str::count::do_count_chars`,
     all called from one site, `parley::shape::shape_item` (`mod.rs:474`) -- confirmed root cause,
     not just a confirmed symptom.
  Wrote the full "Risk" section in `docs/research/text-stack-2026.md` with all three pieces of
  evidence, updated the main measurement table's devanagari row, wrote the Recommendation section
  (conditional: do not switch now, revisit once the defect is resolved upstream or via the
  untested `complex-scripts` feature / a newer parley release), and drafted
  `docs/adr/ADR-0077-migrate-to-parley.md` (status Proposed, blocked on that
  condition, consequences written for both an eventual Accept and Reject). Stopped the `samply
  load` server. Machine confirmed idle again before reporting "слот закрыт" to Master; `du -sh`
  on `tools/text-spike/target/` was 1.3G (samply's own install + the profiled build inflate this
  well past what `cargo build --release` alone used).
- 2026-09-22, methodology correction from Master (caught before PR was opened -- good catch,
  I had not questioned my own harness's problem setup): the "large" comparison wasn't
  backend-vs-backend, it was "cosmic-text shaping many line-sized units" (its `Buffer` splits on
  `BidiParagraphs` internally, per `set_rich_text_impl`) vs "parley shaping one document-sized
  unit" (`ranged_builder(text).build(text)` on the WHOLE 10k-line string). Verified this
  precisely by reading parley-0.11.1's own `src/shape/mod.rs`: `shape_text`'s `break_run` check
  (`mod.rs:140-161`) only starts a new `Item` on a script/bidi-level/style change or an inline
  box -- there is no `\n` check anywhere in that loop, confirmed by reading the loop body
  directly, not inferred. A single-script/style/direction document -- every corpus in this spike,
  Latin included -- is therefore ONE `Item` regardless of `\n` count when shaped through a single
  `build()` call. `mod.rs:474`'s `item_text[..segment_start_offset].chars().count()` is relative
  to `text_range.start` (the current ITEM's start, `mod.rs:315`/`338`), not literally "start of
  the whole document" as I'd loosely said before reading the source -- same thing for a
  single-item document, which is exactly why the earlier framing didn't distinguish the two.
  Fixed by adding `ParleyBackend::shape_per_paragraph` (splits on `\n`, one `Layout` per
  non-empty paragraph, reuses `font_cx`/`layout_cx` across paragraphs) alongside the original
  `shape` (renamed in doc comments to "not a UI scenario", kept as an explicit data point) in
  `tools/text-spike/src/parley_backend.rs`; added `--parley-mode per-paragraph|single` to
  `main.rs` (`per-paragraph` is now the CLI default). rustfmt-clean. Rewrote the report's Risk
  section with the fully-sourced item-boundary mechanism (not just profiling-inferred), added a
  "Methodology correction" subsection, marked the Recommendation section explicitly provisional
  pending the per-paragraph re-run, and added a caveat paragraph to the ready-to-file upstream
  issue text plus retitled/resummarized it to describe the single-`Layout` stress case precisely
  rather than implying it's how parley is normally used. Did NOT compile anything for this
  (slot was with Opus/A1 PR0) -- only source edits + rustfmt-check, per Master's "отчёт можешь
  переписывать без слота, слот у Opus" instruction. Still open, needs the next slot: task 4
  (why Devanagari specifically has more font-selection segments per Item than Latin -- narrowed
  to "something in the font-selection/Indic-shaping path" via source + the `setup_syllables`
  profile hit, but not pinned to an exact line without instrumenting parley's own source, which
  is out of scope) and the full per-paragraph re-run (6 corpora x 2 sizes x 2 caches x 5 reps,
  plus a devanagari x1/x2/x4/x8 scaling study in per-paragraph mode) -- PR is NOT opened until
  that lands and the report's provisional sections are finalized.
- 2026-09-22, slot reopened (Opus closed A1 PR0), machine idle. `cargo build --release` clean
  (4.24s incremental from the source edits, no dep changes). Smoke test confirmed the fix
  dramatically: `devanagari large cold` per-paragraph = **1.14s** vs the earlier single-`Layout`
  measurement's ~90s at the same 10k lines (~78-80x faster). Ran the full clean series
  (`run_measurements_v2.sh`, single `run_in_background: true` process, 240/240 points, no
  contamination this time -- learned from the earlier double-script incident, checked `ps` before
  assuming anything, didn't launch a second script) followed by the devanagari scaling study
  (1250/2500/5000/10000 lines) in per-paragraph mode: cosmic-text unchanged (~2.0x per doubling,
  linear); parley per-paragraph settles at ~1.97x per doubling by the third doubling (noisier at
  n=1250->2500, 2.37x, since fixed backend-construction cost dominates more at small n) --
  **confirmed linear, matching cosmic-text**. The quadratic behavior is now proven to be 100% an
  artifact of the single-`Layout`-for-a-whole-document pattern, not a Devanagari-shaping defect.
  Aggregate result across all 48 combos: **parley wins on all six corpora** (not just four),
  2.5x-8.4x faster, ~22MB peak RSS vs cosmic-text's 620-770MB (30-38x less memory) -- emoji_zwj
  and devanagari both flipped from "slower" to "faster" once measured correctly (emoji_zwj:
  7.3x slower -> 3.4x faster; devanagari: ~27x slower -> 2.9x faster). Rewrote
  `docs/research/text-stack-2026.md` in full (kept the Risk section's mechanism/profiling/source
  evidence as historical record + upstream-issue material, reframed as "investigated, not a
  blocker" rather than "confirmed blocker"; new clean Measurements tables; Recommendation now
  unconditional "adopt parley, per-paragraph"). Rewrote `docs/adr/ADR-0077...md`'s Decision from
  "do not migrate, blocked" to "migrate" and renamed the file itself from
  `ADR-0077-text-stack-parley-conditional.md` to `ADR-0077-migrate-to-parley.md` (no longer
  conditional; file wasn't committed yet so a rename cost nothing) -- fixed the one cross-reference
  in the report to match. rustfmt-clean throughout. Did not open the PR yet -- that's next, per
  Master's separate instructions (workspace-isolation check, script gates before push, delete
  `tools/text-spike/target/` after).
- 2026-09-22, PR-opening checklist done, then a SECOND methodology correction arrived before I
  could open it -- Master read the source and caught that the memory comparison had the same class
  of bug the timing comparison had, just in the opposite direction: `CosmicBackend::shape` shapes
  10,000 lines in ONE `Buffer::shape_until_scroll` call that holds every line's data simultaneously
  before returning, while `ParleyBackend::shape_per_paragraph`'s loop builds-and-drops one `Layout`
  per paragraph, so at no instant does it hold more than ~1 paragraph's worth of state -- the
  "30-38x less memory" number was measuring two different retention policies, not two shaping
  engines. `ru_maxrss` is a process-lifetime high-water mark (never decreases), so this isn't
  fixable by sampling later -- the measurement has to retain the same amount of state cosmic-text
  does. Fixed by adding `ParleyBackend::shape_per_paragraph_retained` (returns `(ShapeResult,
  Vec<Layout<()>>)`; `main.rs` holds the `Vec` alive until after sampling peak RSS) and by sampling
  RSS at two points (`init_rss_bytes` right after backend construction, `peak_rss_bytes` after
  shaping) per Master's other ask -- motivated by a hypothesis that cosmic's eager macOS
  system-font-database load dominated its memory number, which the data then DISPROVED (init is
  ~13-14MB cosmic vs ~16-18MB parley, small and roughly equal for both -- the real gap is entirely
  in the shaping+retention delta). `cargo build --release` clean (3.52s). Smoke test confirmed the
  direction (latin large cold: cosmic 966.8MB peak vs parley retained 175.5MB) before committing to
  a full re-run. Ran the full 240-point series again (`run_measurements_v2.sh` reused unmodified,
  single `run_in_background: true` process, no contamination). Result: parley's memory advantage
  narrows from the suspicious 30-38x to a defensible **1.3x-4.9x** (smallest on arabic/devanagari --
  the two most syntactically-complex corpora -- largest on latin/emoji_zwj), still favoring parley
  on every corpus; timing conclusions (2.4x-8.2x faster, all six corpora) are unchanged by this fix
  since it only touches the memory sampling, not the shape calls themselves. Also built the
  work-parity table Master asked for (task 3): `line_count` matches exactly on all 6 corpora,
  `glyph_count` matches on 5/6 (the already-documented emoji_zwj ZWJ-clustering difference, -1.59%,
  confirmed as the SAME known discrepancy scaled 10,000x, not a new one). Rewrote the report's
  Measurements section (separate timing table + new "Memory: a second, fairer comparison" +
  "Work-parity check" subsections, both walking through why the two rounds of correction happened,
  not just presenting corrected numbers silently) and ADR-0077 (new "Evidentiary basis" section
  covering both correction rounds explicitly, per Master's "условия доказательства должны быть в
  ADR" instruction; status left as **Proposed**, unchanged, per Master's explicit instruction #4 --
  this is a strategic/user decision, not something I unilaterally accept). Ran all three local
  gates clean: `just text-check` (confirmed `docs/research/**/*.md` is typos-excluded by existing
  repo policy -- my earlier direct `typos <path>` invocations that flagged `ot_shaper_indic` as a
  false positive were bypassing that exclude by naming the file explicitly; the real gate is
  clean), `just fmt-check`, `just port-check`, `just inventory-check`. Deleted
  `tools/text-spike/target/` (reclaimed 1.3G) since CI doesn't build this non-member crate today.
  Did NOT open the PR -- Master paused PR-opening twice already for methodology issues found by
  reading the code; waiting for explicit go-ahead on this round's corrected numbers before opening,
  rather than assuming pp.1-3 being done also authorizes pp.4 (open PR) without being told.
