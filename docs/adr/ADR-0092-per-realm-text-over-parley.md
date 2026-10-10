# ADR-0092: Text shapes per UI runtime over Parley and crosses the display list as neutral shaped runs

- **Superseded-by:** [ADR-0182](ADR-0182-explicit-numeric-text-sizing.md) for
  raw query capability boundaries and fallible text-loan acquisition (§10, 3a).
  Runtime ownership of reusable shaping resources remains in force.
- **Status:** Accepted. Every gate (§8) is met: gate 1 by a prototype on 2026-09-26, gates 2, 6
  and 7 on 2026-09-30 (see Context). Every step of §10 landed. Two things this record leaves to
  others: the bidi base direction (Parley 0.11.1 takes it from the first strong character;
  flui-painting mapping decision 12), and how a hot-reload plugin's font bytes cross its FFI
  boundary ([ADR-0094](ADR-0094-hot-reload-through-subsecond.md)). Each UI runtime owns a
  `TextContext` over the app's one `FontCollection` (one per owner thread, which is one per
  process while [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) fixes one
  owner thread), lent to every pipeline; Parley measures, paint draws the runs of the layout
  that measured (`DrawOp::Paragraph` carries a `ShapedParagraph`, the engine's atlas rasterizes
  through `SwashRasterizer`), and carets, selection boxes, hit-testing, line metrics and word
  boundaries read that same layout. The performance overlay's labels are shaped through the
  UI runtime's `TextContext`, so the engine shapes no text. A face registered on the collection
  reaches measurement, paint and carets together at each pipeline's next frame (§2). The app's
  shared engine services build the collection with the bundled faces and hand the host's to a
  feed on a thread of its own, which scans the host once with fontdb (`HostFonts::scan`) and
  adds its faces with fallback tables FLUI owns; the first frame does not wait for it, and its
  faces arrive as a registration's do. cosmic-text, `unicode-script` and `FONT_SYSTEM` are
  gone, and the editor steps through the same ICU4X boundaries the painter clusters by, with no
  `unicode-segmentation` in any FLUI crate. The supersessions below have taken effect and their
  back-links are written.
- **Date:** 2026-09-25
- **Superseded-by:** ADR-0114 for §10 step 4b's direction-only line alignment; other decisions remain in force.
- **Superseded-by:** [ADR-0122](ADR-0122-validated-glyph-image.md) for §5's
  `GlyphImage` representation and admission only; the raster trait, font identity,
  registry ownership and shaping decisions remain in force.
- **Revised:** 2026-09-26 (rasterization prototype; see Context); 2026-09-29 (§10 step 3
  split into 3a and 3b; the UI runtime lends its context through a shared handle; Parley
  measurement behind `parley-layout`; a pipeline is built with its context, and the hot-reload
  plugin image is a UI runtime of its own for text); 2026-09-30 (§7: the host's faces come from the
  process font system's discovery, with one family rule and one fallback order for both
  shapers; §10 step 3c; §10 step 4 split into 4a and 4b, and 4a's face-agreement gate
  decided: host faces first; then 4a and 4b land together, §4's blob table is per paragraph,
  and variation interning and the subpixel bin move to the raster side); 2026-09-30 (§2 and
  §10 step 3b: the collection-changed event is the collection's generation, read by each
  pipeline at its next frame); 2026-09-30 (§10 step 5 rewritten to what ships: no caret
  rollback flag, the base direction and the off-thread host scan moved to later steps, and
  Parley's `complex-scripts` feature stays off); 2026-09-30 (§10 step 6 split into 6a, 6b and
  6c; §7: the app's shared engine services own one fontdb scan and FLUI owns the fallback
  lists; gates 2, 6 and 7 closed with the findings in Context); 2026-09-30 (§10 step 6b: the
  host feed runs off the owner thread and registers one file at a time; §3, §7, gate 6 and
  Context rewritten for it); 2026-09-30 (§10 step 6c: the editor's boundaries are ICU4X's, §6
  accepted, and the record Accepted)
- **Supersedes:** [ADR-0077](ADR-0077-migrate-to-parley.md) (absorbed: its direction, its
  preconditions and its "If later Rejected" branch are carried here)
- **Supersedes:** [ADR-0016](ADR-0016-unified-font-system-registration.md),
  [ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md)
- **Amends:** [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  (Part 1: the process-wide font doors become a per-UI runtime context; Part 2: the `Paragraph`
  payload), [ADR-0066](ADR-0066-display-list-command-representation.md) (`DrawOp::Paragraph`),
  [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (`GlyphKey`, the rasterization door, where the
  atlas lives; the atlas's key type becomes a `GlyphRasterizer` parameter)
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the text store answers geometry from this
  layout), [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) (the raster
  thread and the `GpuContext` atlas), [ADR-0097](ADR-0097-no-process-global-state-gate.md)
  (`FONT_SYSTEM` left the allowlist at §10 step 6a)
- **Refs:** decision D12 in the [decision index](../../design/decisions.md); roadmap exit B1
  (per-UI runtime fonts)

## Context

Text was one process-wide, locked object when this record was written (the file and line
numbers below are of that tree; §10 step 6a removed `text_layout/layout.rs`).

- `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`
  (flui-painting's `text_layout/layout.rs:124`, a `parking_lot::Mutex`, `:18`) holds
  the cosmic-text `FontSystem`, its scaler and a database generation. Every measurement on every
  UI runtime and every glyph rasterization takes that lock: `SharedFontSystem::rasterize` locks it and
  rasterizes with the scaler kept beside the database (`layout.rs:347-366`).
- Its initializer scans the host's fonts synchronously (`FontSystem::new()`, `layout.rs:147`),
  then rebuilds the system around FLUI's emoji-forbidding fallback (`layout.rs:161-164`,
  `font_resolve.rs:277`). The runtime forces that initialization while constructing its shared
  services (`crates/flui-app/src/app/runtime.rs:148`), so the full scan sits before the first
  frame.
- The glyph key is cosmic-text's own: `pub struct GlyphKey(pub(super) cosmic_text::CacheKey)`
  (flui-painting's `text_layout/glyphs.rs:23`, removed at §10 step 4), stable only because the process-wide
  database is append-only.
- The display list carries the shaper's layout: `DrawOp::Paragraph { layout: Arc<TextLayout>, … }`
  (`crates/flui-painting/src/display_list/command.rs:167-174`), where `TextLayout` wraps
  cosmic-text's buffer. The crate still re-exports `cosmic_text::fontdb::Family`
  (`crates/flui-painting/src/lib.rs:87`).
- Every painter builds its own glyph atlas over that shared system
  (`crates/flui-engine/src/painter/mod.rs:163-167`), one per window.
- `add_face` appends a face and bumps the generation (`layout.rs:484-495`); the family
  resolver rebuilds on the new generation (`font_resolve.rs:641-662`), but nothing marks text
  render objects for layout. [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  records this as a named gap and defers "a `FontContext` handle threaded through layout" until a
  second font source or closing the ambient-reach ratchet becomes a requirement. Per-UI runtime fonts
  (roadmap B1) and the no-process-global-state gate
  ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) are both of those triggers.
- Unicode data comes from three places: cosmic-text's own dependencies, `unicode-segmentation`
  (`crates/flui-painting/Cargo.toml:45`, `crates/flui-widgets/Cargo.toml:50`) and
  `unicode-script` (`crates/flui-painting/Cargo.toml:40`). Editor boundaries and shaper
  boundaries can disagree.

[ADR-0077](ADR-0077-migrate-to-parley.md) proposed moving to Parley on the strength of a spike
(`tools/text-spike`, `parley = "=0.11.1"` at
[`tools/text-spike/Cargo.toml:23`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/tools/text-spike/Cargo.toml#L23))
that measured
shaping and layout only, and made a rasterization prototype its blocking precondition. Since then
the market survey reports that Parley shapes with HarfRust and analyses with ICU4X, that fontique
offers a shared collection mode (`CollectionOptions { shared: true }`) whose changes are visible
to every clone, that glyph rendering was split out of Parley into `glifo` (unstable, in the Vello
repository), and that Bevy's move to Parley lost glyph `byte_index`/`byte_length`. These are
survey findings, not re-checked here.

### What the spike showed (2026-09-26)

A rasterization prototype ran on branch `spike/parley_atlas` at `38af54e25`, behind a `parley`
feature in `flui-painting` and `flui-engine`. It is not merged.

- **Versions.** Parley 0.11.1, swash 0.2.10, skrifa 0.44, vello_cpu 0.2.0. swash and glifo share
  skrifa 0.44; the skrifa 0.40 duplicate in today's tree leaves with cosmic-text.
- **Shape.** A per-UI runtime `ParleyText` over fontique's shared `Collection` and
  `SourceCache::new_shared`; `fork()` gives a second UI runtime a context over the same fonts. The
  atlas became `GlyphAtlas<R: GlyphRasterizer = SharedFontSystem>`. Pages, packing, eviction,
  growth, upload and the `GlyphImage` format are unmodified; only the key type became a
  parameter. Gate 1 said "accepts unmodified"; the key change is the one §5 already names.
- **Oracle.** The reference is cosmic-text's `SwashCache::get_image_uncached` on a test-local
  `FontSystem`, fed the same bytes, glyph, size and subpixel bin, at 13, 18 and 32 px. swash
  driven directly is bit-identical on 416 distinct glyphs (Latin 151, Arabic 94, CJK 66, emoji 33
  of which 24 colour, Devanagari 72). That comparison is near-tautological, since both sides are
  swash with the same parameters: it shows the key carries enough information, not that an
  independent scaler agrees. The independent check is glifo 0.3.0 through vello_cpu: mean
  similarity 0.90–0.99, ink ratio 0.969–1.048. Its low minimums come from punctuation (glifo
  hints vertically only) and emoji (it draws COLRv1 where swash draws COLRv0).
- **Gate substitution.** Gate 1 asked for glifo against hand-written skrifa-outline-to-atlas
  glue. The prototype used swash (skrifa outlines plus zeno, as a library) as that stand-in. The
  gate is recorded as met with that substitution.
- **Key stability.** Keys are equal across re-shaping and across forked UI runtimes. They survive
  source-cache prunes only while the raster-side registry holds the blob: fontique's shared
  `SourceCache` holds `WeakBlob`s, so an unheld blob reloads under a new id (0 became 1 in the
  prototype) and every key for that font changes.
- **Locks.** FLUI adds no lock. fontique's shared mode takes internal mutexes on local cache
  misses: `Collection::family` locks `system.fonts` (fontique `collection/mod.rs:326`) and
  `SourceCache::get` locks the shared cache (fontique `source_cache.rs:104`); those locks are
  shared across forked UI runtimes. Gate 1 only requires the rasterizer to run outside shaping, which
  holds: both rasterizers are `Send`, own their state behind `&mut`, and share no lock with
  `ParleyText`. The thread test shows `Send` and execution, not that the two threads overlapped.
- **Globals.** `FONT_SYSTEM` is never built on the Parley path. This is asserted per nextest
  process; the assertion fails under plain `cargo test`, where tests share a process.
  `cargo xtask globals` stays at 54 listed. The new dependencies' statics are atomic id counters
  only.
- **Timing.** Windows, 16 px, medians, both backends pinned to Segoe UI, on a shared host with up
  to 1.6× run-to-run variance. Timing is not part of the gate. The ratios are a reviewer's
  recomputation from the prototype's own tables; the benchmark was not re-run.
  - Cold raster of every distinct key: cosmic plus swash 0.50–1.04 ms, Parley plus swash
    0.47–0.95 ms, so equal cost per glyph.
  - glifo is 1.26–1.44× slower than swash on outlines and 3.7× on emoji.
  - 1,000 lines, shape plus raster: Parley plus swash is 1.14–2.04× faster than cosmic
    (`emoji_zwj` 1.14, `cjk` 2.04).
  - Unpinned, Parley resolves `sans-serif` to Arial and cosmic to Segoe UI; the range is then
    1.05–1.93×.
  - Caveats: the corpus is one paragraph repeated; cosmic shapes one `Buffer` while Parley shapes
    a `Layout` per line; Parley's lazy font scan is counted in shaping while `FontSystem::new` is
    excluded; at 1,000 lines glifo beats swash on Latin and Arabic, so those totals are dominated
    by shaping.
- **Not verified.**
  - wasm32, macOS and Linux.
  - GPU readback of atlas pixels (atlas pages lack `COPY_SRC`).
  - Same-key bitmap determinism: the grow test asserts `slot.texel` and `size` from the entries
    map, which `grow` never changes, and nothing rasterizes a key twice and compares bitmaps.
  - Baseline agreement to 1 px: the prototype truncates Parley's absolute y, while today's path
    rounds the baseline (`(run.line_y * scale).round()`) and truncates only the glyph offset.
  - Variable fonts and synthetic bold and italic.
  - CJK segmentation: ICU4X logs "no segmentation model" because Parley's `complex-scripts`
    feature is off.
  - Font registration after the atlas exists: every atlas test registers its runs before
    `GlyphAtlas::new`.
  - Neutral runs: `ShapedRun.font` is `parley::FontData`, so §4 is not demonstrated.

### The `arabic_mixed` difference (gate 2, 2026-09-30)

The spike measured its `arabic_mixed` corpus as 407 glyphs on cosmic-text and 406 on Parley.
Reproduced on the Windows development host with the removed spike's corpus (its
`corpora/arabic_mixed.txt` at `ba3af13d2^`), cosmic-text 0.19.0 against
Parley 0.11.1, in a scratch crate:

| Family requested | cosmic-text glyphs (face) | Parley glyphs (face) |
|---|---|---|
| sans-serif (the spike's request) | 407 (Segoe UI) | 406 (Arial) |
| "Segoe UI" | 407 | 407 |
| "Arial" | 406 | 406 |
| "Tahoma" | 406 | 406 |
| "Times New Roman" | 406 | 406 |

The count is face choice: cosmic-text's sans-serif named Open Sans, which the host lacks, and
fell through to Segoe UI, while fontique's names Arial. Segoe UI draws lam-alef as two glyphs;
Arial, Tahoma and Times New Roman draw one ligature. On one face the counts are equal. The one
difference left is attribution: on a ligating face cosmic-text tagged the ligature glyph with the
lam's byte and Parley tags it with the alef's cluster, the lam a component cluster with no glyph.
Neither is a shaping defect; flui-painting `ARCHITECTURE.md`, mapping decision 19, records both
and the rule FLUI ships.

### Startup cost and retained memory (gate 6, 2026-09-30)

Measured by `cargo bench -p flui-painting --bench text_startup` on the Windows development host
(release build, 168 faces in 145 files), criterion medians of 10 samples, and a counting global
allocator for the heap figures. The heap figures count allocations only: font files the
collection maps are not in them, so they are not resident memory. The "before" column ran the same bench
on the tree before step 6a (`main` at `b76144dd3`), where the scan is
`cosmic_text::FontSystem::new()` and the feed `FontCollection::with_host_faces` over the
`shared_font_system()` handle; every other line of the bench is the same:

| What | Before §10 step 6a | After |
|---|---|---|
| `FontCollection::new()` (bundled faces) | 9.8 µs | 10.1 µs |
| Host scan (`cosmic_text::FontSystem::new`, then `HostFonts::scan`) | 6.3 ms | 5.8 ms |
| Feed (`FontCollection::with_host_faces`, then `with_host_fonts`) | 36.5 ms | 36.8 ms |
| Heap a host-fed collection keeps | 2.42 MiB | 2.42 MiB |

Heap one laid-out `TextPainter` keeps (1,000 laid out at 400 px and held, the live-bytes delta
divided by 1,000), the same before and after: a 12-char label 3.0 KB, a 57-char sentence
4.9 KB, a 570-char paragraph 37.8 KB (about 66 B per char), the 407-char `arabic_mixed` corpus
29.1 KB, and 135 chars of CJK 27.7 KB.

- Memory is acceptable: 2.4 MiB for the collection, and a painter's cost linear in its text.
  A document of 10,000 laid-out 570-char paragraphs would keep about 380 MB, so long text is
  laid out by a virtualized list, not held whole.
- The startup cost was not acceptable on the owner thread for good: about 43 ms of scan and
  feed ran before the first frame, and the feed, which parses every file the scan found, was
  86% of it. §10 step 6b took both off the owner thread. Dropping fontdb for a hand-rolled scan
  would save at most the scan's few milliseconds, so fontdb stays.

After §10 step 6b (`HostFontFeed`, the same bench and host, same day; the host was under other
load, so the scan read slower than above):

| What | Time |
|---|---|
| `FontCollection::new()` (bundled faces) | 19.3 µs |
| Host scan (`HostFonts::scan`) | 14.3 ms |
| Synchronous feed, one file per registration (`FontCollection::with_host_fonts`) | 16.3 ms |
| Owner thread at start (`FontCollection::with_host_feed`) | 16.9 µs |
| Off-thread feed on the shared collection (`HostFontFeed::run`, scan excluded) | 45.4 ms |
| Heap a host-fed collection keeps | 0.15 MiB |

- The owner thread now pays about 17 µs before the first frame: the bundled collection and a
  feed handle.
- Registering one file per call is what made the synchronous feed and its heap smaller.
  fontique 0.11.1's `load_fonts_from_paths` (`collection/mod.rs:682-697`) calls
  `register_font_impl` once per face found under the paths, with one accumulator shared by the
  whole call, and each call merges the whole accumulator into the families again, so a call
  over many files adds each earlier face once more per later face: quadratic in the call's
  faces, and the 2.42 MiB above was mostly those duplicate entries. One file per call bounds
  that to the faces of one font collection file. Measured on this tree with only that line
  changed back: 57.0 ms and 2.42 MiB for the batch call, 16.3 ms and 0.15 MiB per file.
  Upstream report: not filed yet; re-check on the next fontique release.
- The off-thread feed costs more than the synchronous one: each source is first read on a
  scratch collection (§7), and each registration and fallback write on the shared collection
  takes fontique's lock, bumps its version and deep-copies its data into the feed's clone at
  the next write. That runs on the feed's own thread.

### The upstream `Item`-boundary report (gate 7, 2026-09-30)

ADR-0077's quadratic finding is in Parley 0.11.1's `shape_item`
(`parley/src/shape/mod.rs:474-476`), which counts `item_text[..segment_start_offset].chars()`
once per segment, O(n²) in an `Item`'s length. On Parley's `main` branch (unreleased) shaping
has moved to `parley_engine/src/shape/shaper.rs`, which walks a segment once, so the cost is
gone upstream and there is nothing to file. `itemize` still does not split an `Item` at `\n`,
which is harmless without the rescan. FLUI shapes one `Layout` per paragraph (§1), so it never
reached the quadratic case. Re-check after the next Parley release: bump Parley and rerun the
devanagari 10,000-line case.

## Decision

### 1. Parley shapes and lays out, one `Layout` per paragraph

`flui-painting` moves from cosmic-text to Parley. Each `RenderParagraph` shapes its own `Layout`;
a multi-paragraph buffer is never one `Layout` (ADR-0077's quadratic `Item`-boundary finding).
FLUI's own family resolution ([ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md)) carries over:
a style's family is resolved by FLUI's policy before shaping, whatever the shaper's fallback does.

### 2. One shared font collection, handed down explicitly

The application owns one fontique `Collection` with `shared: true`, wrapped as
`flui_painting::FontCollection`. Faces are only ever added, never removed or changed: the handle
offers `register_font` and no removal, so the rule is the API's, not a convention. It lives in
the host's (`flui-app`'s) shared engine services (`SharedEngineServices`, one per owner thread,
which is one per app while ADR-0091 fixes a single owner thread), held in the host's granted
ADR-0097 trampoline cell with no global of its own, and reaches each UI runtime only through
`UiRuntime::new`. Registering a font adds it to the collection and raises the collection's
generation: that generation is the font-collection-changed event. The app notifies every UI runtime it owns (`UiRuntime::fonts_changed`,
on each UI runtime's owner turn), and at the next frame each pipeline marks for layout and paint every
render object that measured through the UI runtime's context since the last change — closing
ADR-0065's named gap. The pipeline finds those objects by their loans of the context, so a render
object that measures text needs no code of its own to be re-laid out.

### 3. Per-UI runtime contexts with no FLUI lock

Each UI runtime owns a `flui_painting::TextContext`: a Parley `FontContext` and `LayoutContext` over a
clone of the collection, as owner-thread state, used through `&mut` and reached through the
layout context explicitly — the handle ADR-0065 deferred. Layout on one UI runtime never waits on
another UI runtime's shaping. FLUI's frame path takes no lock of its own; clippy `disallowed_types`
rejects a `Mutex` or `RwLock` in `flui-painting`, with no `#[expect]`ed site since §10 step 6a.

fontique 0.11.1, read with `system` off and every face registered from memory, which is how FLUI
builds the collection:

- Registration takes `&mut self` (`collection/mod.rs:202,213,226`); in shared mode it writes the
  shared data under fontique's mutex and bumps a shared version. `FontCollection::register_font`
  takes `&self`, so it registers through a clone of fontique's collection, which copies the
  local family data once per registration; that is the price of adding no FLUI lock.
- Every clone runs `sync_shared` at its next query (`collection/mod.rs:567-581`): one atomic
  load, and only when the version moved does it take the shared mutex and deep-copy the data.
- The system-fallback lock (`collection/mod.rs:429`) is compiled out without `system`.
- `SourceCache::get` returns a memory source without locking (`source_cache.rs:95-97`). That
  covers the bundled and registered faces. A host face fed by path (§7) is a `Path` source,
  which takes the shared source cache's lock on a cache miss (`source_cache.rs`).

So the one lock left is one shared-mutex acquisition per UI runtime on its first query after a
registration; a frame with no registration behind it takes none. While the host feed runs off
the owner thread (§7), each file it adds is one registration: it moves fontique's version, so a
UI runtime's next query re-reads the data, and a query that comes while a file is being added waits
for that one file's registration, never for the whole feed. That is a reading of fontique's
code, not a measurement.

### 4. A neutral shaped-run contract on the display list

`DrawOp::Paragraph` carries shaped runs defined by `flui-painting`, not a shaper type. A run
names its font by blob identity (blob id plus face index), its size, its normalized variation
coordinates and synthesis, and a list of glyphs (glyph id, position, subpixel bin), with span
colour and decoration as today. Nothing in the run names Parley, fontique, skrifa or cosmic-text,
so the engine and any second raster backend
([ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)) consume runs without a shaper
dependency. "Painted as measured" (ADR-0065) still holds by identity: the runs are produced from
the same `Layout` that measured the paragraph.

Settled by §10 step 4:

- The run does not carry `parley::FontData`. Each `ShapedParagraph` holds its own table of
  FLUI-owned `FontBlob`s (the font bytes behind a shared handle, and the blob id keys name
  them by), and a run names its face by an index into it. The table is per paragraph, not per
  frame: a retained layer replays a picture recorded frames earlier, so a table built from the
  blobs this frame's recorders named would miss a replayed paragraph's faces. A paragraph that
  carries its faces is complete by construction, for one shared handle per distinct face per
  paragraph, and the handle keeps fontique's weakly cached blob, and so its id, alive.
- A run carries its normalized variation coordinates raw; the raster side's registry interns
  them (`FontRegistry::prepare_run`), because a `VariationId` means nothing to another
  registry.
- A glyph carries its id and logical position; the subpixel bin, device row and raster size
  are computed when it is placed (`ShapedRun::placed_glyphs`), because they depend on the
  device transform, which only the replay knows.
- `the_engine_does_not_shape` pins that the engine names no shaper, in its source and its
  manifest.

### 5. Glyph keys carry font identity; rasterization is a raster-side trait

**Image representation superseded by ADR-0122.** The rasterizer now returns an
immutable `GlyphImage` admitted by checked construction; the atlas trusts its
byte-layout invariant. The key, registry and `Option` rasterization contract
below remain binding, including replay checks for a different valid bitmap.

`GlyphKey` becomes font blob id, face index, glyph id, exact size, variation identity, a
quarter-pixel x bin (cosmic-text's rule) and the hinting and synthesis flags. The prototype hashed
the variation coordinates to 64 bits; the migration interns them instead, because a hash can
collide.

Keys are stable while the raster-side font registry holds every blob a key names. The registry
only grows, so each blob stays resident for the rasterizer's life; that residency cost is
accepted and measured under gate 6. (fontique's shared source cache holds blobs weakly, so
without the registry a pruned blob reloads under a new id and its keys change; see Context.)

A `GlyphRasterizer` trait turns a key, resolved against its own font registry, into the
`GlyphImage` ADR-0067's atlas already accepts; the atlas takes the rasterizer as a type
parameter. `rasterize` returning `None` means "not placed", which is how the atlas already
treats `slot() == None`, not "draws nothing". The rasterizer is **swash, driven directly**, with
swash's per-font cache key made once per face: `FontRef::from_index` mints a new one per call
and would rebuild hinting state per glyph. The atlas feeds each run's face to the rasterizer
it owns before placing the run's glyphs (`GlyphAtlas::rasterizer_mut`,
`FontRegistry::prepare_run`), from §4's blob handle.

The rasterizer is owned by the raster side and runs outside shaping; the atlas belongs to the
`GpuContext` and is single-owned on the raster thread
([ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)). One shaper everywhere;
only the rasterizer and hinting may differ per platform.

glifo is not adopted for now: it is slower (Context), marked experimental, hints vertically only,
brings its own atlas that overlaps ADR-0067, and its fake bold was never wired. Revisit only if
the CPU raster backend ([ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)) settles on
vello_cpu.

### 6. ICU4X is the single Unicode source

Grapheme and word segmentation for editing, bidi and line breaking use the ICU4X data Parley
already brings. `unicode-segmentation` and `unicode-script` leave FLUI's crates, and no
`unicode-bidi` is added. `unicode-script` left at §10 step 6a; flui-painting's boundaries are
ICU4X's since step 5, and since step 6c the editor in `flui-widgets` steps through them too
(`flui_painting::text_boundaries`). `unicode-segmentation` stays in the lockfile, through
`winit` and `convert_case`; what this section binds is that no FLUI crate depends on it, which
`deny.toml` enforces: it bans the crate with those two as its only allowed dependents.

### 7. System fonts come from one discovery, resolve by one rule and fall back in one order

Bundled fonts are in the collection before the first frame. The host's faces come from one
discovery, a fontdb scan the app's shared engine services own (`HostFonts::scan`, a value; no
process-global keeps it): the platform's font directories, and fontconfig's configuration
through fontdb's pure-Rust parser where there is one. `FontCollection::with_host_fonts` reads
the scan's sources (file paths, in-memory fonts, family names), its generic families and its
fallback lists, and reads the files into the collection. fontique's `system` feature is not
used: it reaches `windows`, which tier S forbids. The collection is fed once per app, in the
host's shared engine services; UI runtimes only clone it.

Measurement and paint read one layout, so one set of rules picks a face:

- **one family rule**: `resolve_family_name` (ADR-0059's rule) resolves a style against the
  families the collection holds, and Parley is handed that one family, with nothing after it. A
  family is held only when spelled exactly as the fonts name it, as fontdb matches names, so the
  rule narrows fontique's case-insensitive lookup to that question;
- **one fallback order**: past that family, each script's fallback families are FLUI's list for
  the script on the host platform, then the platform's common list, then the family the
  sans-serif generic names (`FallbackChain`, over `fallback_tables`: per-platform lists FLUI
  owns, ported from cosmic-text 0.19 and keyed by ISO 15924 code; the host locale's language,
  script and region subtags pick the Han list, where cosmic-text matched the tag whole and gave
  `ja-JP` the Simplified Chinese faces). The common list is the emoji generic. fontique has no walk over every other face, so a
  character no listed family covers is `.notdef`;
- **one set of generics**: with `bundled-fonts` the collection binds every generic to Roboto and
  a host feed rebinds none; without it an unbound generic takes the scan's pick, a carried family
  that sets Latin text, preferring the platform's common list; system-ui names sans-serif's.

A face whose family the collection holds when the feed starts (the bundled faces, and a face
the app registered before the start) is not fed again. The feed runs off the owner thread: the
app's shared engine services build the collection with the bundled faces
(`FontCollection::with_host_feed`), the first frame renders with them, and a thread of its own
(`flui-host-fonts`) scans the host and runs the feed (`HostFontFeed::run`). The feed adds one
file per registration, so fontique's lock is held for one file at a time, and each source is
first read on a scratch collection inside `catch_unwind`: a source that panics or holds no
family is skipped, since a panic under fontique's lock would poison it and every UI runtime's next
query would panic. The registration reads a path source again, so the guard covers a file that
panics on every read, not one replaced between the trial and the registration. When the feed
ends, the collection's generation rises once if the feed added a source, bound a generic or
reordered a fallback list, and also if the feed unwound; a feed that changed nothing leaves it
alone, so no UI runtime lays its text out again for nothing. The feed then wakes the owner, and the
owner's next turn tells every UI runtime, as after a registration (§2): text measured before lays
out again in the host's faces. The thread is the app's own, not a job on the runtime's compute
lane or the host's executors: a lane may refuse a job and drop it, and the feed must run
exactly once, so a refusal would need a way to hand the feed back. The thread is detached; one
still feeding when the loop exits only adds faces to a collection no UI runtime reads any more and
sets a redraw flag no loop polls. An app family
registered while the feed runs may end up with the host's faces of the same name too,
depending on which comes first; that is accepted. Without `bundled-fonts` the collection would
have no face for the first frame, so it is fed before it is handed out, as
`FontCollection::with_host_fonts` does, and the feed has nothing left to do. If no thread can
be started, the feed runs on the owner thread. On wasm32 fontdb finds no host fonts and the
platform has no common list, so the collection holds the bundled and registered faces and every
script falls back to the sans-serif family alone. A bundled-only collection
(`FontCollection::new()`) falls back to Roboto for every script.

### 8. Acceptance gates

These are ADR-0077's preconditions, carried over and extended. Gate 1 is met (2026-09-26,
Context). Gates 2–8 are conditions on the migration changes; every one is met as of §10 step 6a.

1. **Rasterization prototype (blocking).** Parley-shaped glyphs rasterize into `GlyphImage`s the
   existing atlas accepts unmodified, for one Latin and one complex-script case, with a
   `GlyphKey` stable across repeated rasterization of the same glyph; no process-global font
   state; the rasterizer runs outside the shaping lock. The prototype evaluates `glifo` against
   hand-written skrifa-outline-to-atlas glue and records the choice.
   Met, with swash standing in for the hand-written glue.
2. **Cluster differences.** The `arabic_mixed` and `emoji_zwj` glyph-count differences from the
   spike are either pinned by a boundary test or enumerated and accepted in writing (ADR-0077
   precondition 2). Closed: `emoji_zwj` by the grapheme rows of `caret_contract` (§10 step 5),
   `arabic_mixed` enumerated in Context and pinned by
   `a_lam_alef_ligature_is_one_glyph_and_two_caret_stops` (flui-painting mapping decision 19).
3. **Caret and selection.** The existing selection and offset↔cursor tests pass unchanged in
   behaviour for LTR, RTL and mixed bidi, deriving positions from clusters, since Parley's glyph
   runs do not carry byte indices.
4. **Conformance per area.** Bidi visual order, UAX #14 break points, one variable font on the
   weight axis, and `cargo build --target wasm32-unknown-unknown` for the crate that depends on
   Parley (ADR-0077 precondition 4). The decision on Parley's `complex-scripts` feature (without
   it ICU4X has no CJK segmentation model) is made and recorded here.
5. **Family resolution.** ADR-0059's regression (a missing family never falls through to an emoji
   face, issue #927) is pinned by a test on the new stack. FLUI resolves families itself;
   Parley's `sans-serif` is Arial on Windows.
6. **Initialization cost.** Init time and memory measured against FLUI's real initialization,
   not a default one (ADR-0077 precondition 5). Closed: measured by the `text_startup` bench,
   before and after §10 steps 6a and 6b (Context). Memory is accepted; since step 6b the owner
   thread pays about 17 µs before the first frame, and the scan and the feed run on a thread of
   their own.
7. **Upstream report.** The `Item`-boundary issue is filed upstream (ADR-0077 precondition 6).
   Closed as recorded (Context): Parley's unreleased `main` no longer has the quadratic count, so
   there is nothing to file; re-check after the next Parley release by bumping Parley and
   rerunning the devanagari 10,000-line case.
8. **Determinism and baseline.** Rasterizing the same key twice yields equal bitmaps, and glyph
   baselines round as today's path does (`(run.line_y * scale).round()`), pinned against today's
   output. Met: `rasterizing_a_key_twice_draws_the_same_bitmap` and
   `parley_metrics_round_to_todays_baseline`.

### 9. No `flui-text` crate yet

Shaping stays in `flui-painting`. A separate text crate is considered only after the migration,
and only if `cargo build --timings` shows shaping separates cleanly from recording.

### 10. Migration series

Each step is one PR (or a short series), leaves production shippable, and names the follow-up
that wires what it adds.

1. **Raster seam.** `flui_painting::GlyphRasterizer` (key type plus `rasterize`); the engine's
   `GlyphAtlas<R: GlyphRasterizer = SharedFontSystem>`, whose default keeps today's path;
   `ParleyGlyphKey`, `FontRegistry` and `SwashRasterizer` behind flui-painting's `parley`
   feature, off by default and with no production caller. *Acceptance:* the existing atlas and
   readback suites pass unmodified (`cargo xtask gpu-test`). swash matches cosmic-text's scaler
   bit for bit on every distinct key Parley places for Latin (bundled Roboto) and at least one
   host complex script, at 13/18/32 px across all four x bins. Rasterizing one key twice gives
   equal images (gate 8, first half). `cargo xtask globals` is unchanged; `cargo xtask deps` and
   `cargo xtask reach` are green.
2. **Per-UI runtime text context**, in two halves that land separately: 2a in `flui-painting`
   (the types, the lint and their tests), 2b in the runtime (the bullet on shared engine
   services and `UiRuntime::new`). 2a and 2b have landed.
   - `flui_painting::FontCollection` wraps the shared fontique `Collection` (no host scan; with
     `bundled-fonts`, the bundled faces registered and the generic families bound to Roboto) and
     offers `register_font` and no removal. `TextContext`, built from it, owns Parley's
     `FontContext` and `LayoutContext` and shapes a paragraph through `&mut`. Both types exist in
     every build, so a UI runtime's constructor has one signature with or without `parley`; shaping
     and registration sat behind the `parley` feature until step 4a folded it.
   - (2b) The runtime's shared engine services hold the collection, and each UI runtime owns a
     `TextContext` built from the collection passed to `UiRuntime::new`.
   - The cosmic-text path is unchanged, and `FONT_SYSTEM` and `shared_font_system()` stay until
     step 6: this step adds the Parley path beside it rather than routing cosmic-text through a
     new handle, so the handle's shape is Parley's from the start.
   - clippy `disallowed_types` rejects `Mutex` and `RwLock` in `flui-painting`; the cosmic-text
     path's `FONT_SYSTEM` is the one `#[expect]`ed site.
   - *Acceptance (2a):* two contexts over one collection shape on two threads whose intervals
     overlap, with equal metrics; a face registered after both contexts exist shapes in each;
     the Parley path never builds `FONT_SYSTEM`; `cargo xtask globals` is unchanged.
   - *Acceptance (2b):* two UI runtimes built over one collection each hold one context over it
     (`FontCollection::ptr_eq`, and a holder count of one per UI runtime), a UI runtime releases its
     context when it drops, a second presentation adds none, and the UI runtimes a runner builds get
     the runtime's collection (every runner site builds its UI runtime through flui-app's one
     `build_ui_runtime`). That a face registered on the collection shapes in every context
     built from it is 2a's `a_face_registered_after_the_fork_shapes_in_every_ui_runtime`; with the
     UI runtimes' contexts proven to be built from that same collection, it is not repeated at the
     runtime level, which would need `parley` on the runtime's test build.
3. **Layout reaches the UI runtime's text context; registration re-lays out text.** Two halves that
   land separately. 3a and 3b have landed. A third part, 3c, feeds the host's faces into the
   collection (§7); it has landed.
   - (3a) The UI runtime lends its `TextContext` to each presentation's layout, and the box layout,
     intrinsics, dry-layout and dry-baseline contexts expose it (`ctx.text()`, a scoped
     `&mut TextContext`). Every `TextPainter` measuring method takes `&mut TextContext`, and
     `RenderParagraph` and `RenderEditable` pass the lent one.
   - (3a) How the UI runtime lends it: a shared handle, `TextContextHandle`
     (`Rc<RefCell<TextContext>>`), held by the UI runtime and cloned into every presentation's
     `PipelineOwner` when the presentation is assembled (`RuntimeCapabilities::text`, a required
     field). Threading `&mut TextContext` down would change `run_frame`, `run_layout` and every
     binding and harness that drives them, while `pump` and `render_frame` take `&self`. The
     UI runtime and the pipeline owners are already `!Send`; a typed render object sees only the
     scoped borrow, taken from `&mut` context, so it cannot hold two loans or lay out a child
     while that loan remains in use through the same typed context.
   - **Raw capability and acquisition supersession:**
     [ADR-0182](ADR-0182-explicit-numeric-text-sizing.md) replaces the independent
     source/child-query channels with complete mutable intrinsic, dry-layout and
     dry-baseline contexts. Erased layout lends through `text(&mut self)` without
     source extraction. Both raw and typed hooks therefore borrow one capability
     for text and recursive child queries. Independently captured handles or
     driver sources can still alias the runtime resource: `ctx.text()?` refuses
     an overlap with `RenderError::TextContextBusy`, without poisoning, native
     preparation debt or automatic retries. The earlier claim that source
     opacity alone prevented raw overlapping loans was incorrect. Runtime-shared
     resource ownership is preserved. Public query refusal/recovery is pinned by
     `shared_text_alias_refuses_raw_queries_and_recovers`; trybuild rejects live
     loans across same-context query and erased-layout child operations.
   - (3a) Parley measures behind `parley-layout`, not `parley`: the workspace test scope turns
     `parley` on for CI's `test` job, and if `parley` switched measurement, CI
     would measure every text-size test with Parley while the build that ships measures with
     cosmic-text. `parley` compiles the Parley measurement and its tests pin a painter to it.
     Under `parley-layout` size, baselines and intrinsics came from Parley while glyphs and
     carets still came from cosmic-text, until step 4a (flui-painting `ARCHITECTURE.md`,
     mapping decisions 14 and 15).
   - (3b) Registering raises a font-collection-changed event on every UI runtime, which marks
     text render objects for layout (ADR-0065's named gap). The app's door is
     `flui::register_font` (`flui_app::register_font`): it registers on the app's collection,
     whose `register_font` also loads the face into the process font system the collection was
     fed from, and dispatches `UiRuntime::fonts_changed` to every UI runtime, which requests a frame
     for each presentation. Each `PipelineOwner` records, per loan of the context, the node the
     loan was made for, and its drain before each frame compares the collection's generation
     with the last one it applied: on a change it marks every recorded node for layout and
     paint. `SharedFontSystem::register_font` is no longer a public door (a `testing` one
     remains, and reaches carets alone). This meets the precondition step 4's move of
     measurement waits on: a face registered after start reaches measurement, paint and carets
     alike. The app's fonts belong to the
     thread that runs it: a registration made before that thread builds its first UI runtime is
     checked (`FontCollection::check_font`) and held, and lands on both sides when the
     collection is built, so a call on a thread that never runs the app changes neither side.
     The collection judges bytes before the caret side (the process font system) loads them, so
     no refusal leaves a face on one side only.
   - (3b) Every pipeline is built with a text context: `PipelineOwner::new` and
     `new_with_capacity` take a `TextContextHandle`, `PipelineOwner` has no `Default`, and a
     layout, intrinsic or dry-query context takes a `TextSource`, so no path builds a context
     of its own. A presentation's pipeline is built inside `PresentationState::new` from
     `RuntimeCapabilities::text`. A frame driver that moves the owner out of its slot for a
     typestate transition uses `PipelineOwner::take_idle`, whose placeholder shares the
     context. A pipeline with no UI runtime behind it passes `TextContextHandle::standalone`, a
     context over a collection of its own holding the bundled faces.
   - (3b) The hot-reload plugin pipeline (`flui-hot-reload`'s `pipeline.rs`) is one such
     pipeline: `app_plugin!` mounts it with a standalone context, not the host UI runtime's. The
     plugin is a `dlopen`ed image the host reaches only through `flui_app_build(width,
     height)`, which has no parameter that could carry the host's handle, and `abi_token` covers
     only the `Scene` and `LayerTree` layouts, so nothing would check that the two images agree
     on `TextContext`'s layout if one were passed. The plugin image is therefore a UI runtime of its
     own for text, and faces the host app registers do
     not reach it; carrying font bytes across the FFI into the plugin's collection is a
     follow-up, or goes with ADR-0094's replacement of the `dlopen` path.
   - *Acceptance (3a):* two UI runtimes over two collections measure through their own contexts, and
     a frame on one lends nothing of the other's; every presentation's pipeline holds the
     UI runtime's handle; a painter measures through the context it is given, and a registration on
     that collection invalidates its cache; Parley's metrics on the bundled Roboto round to
     today's baselines; a layout that panics while holding the context releases it.
   - *Acceptance (3b):* a two-UI runtime test: a font registered on the collection both UI runtimes were
     built over re-lays out text in both at their next frame, measured and painted in the new
     face (`font_registration_matrix`, flui-runtime). Test bootstraps construct the collection.
   - *Acceptance (3b, pipelines):* a pipeline constructor without a context does not compile; an
     owner taken out of its slot leaves one that measures through the same context; a plugin
     pipeline measures through the context it is mounted with and lays its root out at each
     frame's surface size. The default build still measured
     with cosmic-text through `FONT_SYSTEM`, and `parley-layout` still shaped for paint there,
     until step 4a.
   - (3c) Host faces in the collection, ahead of step 4, so that text the bundled faces do not
     cover (CJK, emoji, a family chain such as Cupertino's `-apple-system`, `system-ui`,
     `Segoe UI`) measures in the face it paints with once Parley measures by default. flui-app's
     shared engine services build the collection with `FontCollection::with_host_faces` over the
     process font system (§7): its faces, generics and fallback order; the family rule is shared
     with `shape.rs`. Standalone contexts, test bootstraps and the hot-reload plugin keep
     `FontCollection::new()`, the bundled faces alone, so they stay deterministic. This is the
     measurement/paint face agreement step 4a's gate asks for.
   - *Acceptance (3c):* on the host's faces, Parley's measured width and height equal the painted
     cosmic-text layout's within 0.05 px for Latin at 400 and 700, monospace, Cupertino's chain
     at 400 and 600, CJK, emoji and mixed text, at 16 and 32 px, and fail on a bundled-only
     collection; every family the process font system carries is in the collection; each
     script's fallback families and the emoji generic follow the caret side's lists in order,
     with the sans-serif family last; a family spelled in another case than the fonts name it
     resolves alike on both sides; on a bundled-only collection a glyph the named family lacks
     measures in Roboto;
     the runtime feeds once per app, not per UI runtime; a font file that cannot be read is skipped
     and the feed completes. `cargo xtask globals` is unchanged.
4. **Parley measures by default (4a); neutral shaped runs on the display list (4b).** Two
   halves, planned to land separately as step 3's did; the owner decided on 2026-09-30 that
   they land together, so one Parley layout measures and paints and the hard-break divergence
   between the two shapers never ships. Both halves have landed.
   - (4a) `TextPainter` measures size, baselines and intrinsics on Parley through the lent
     `TextContext` in the default build. A cosmic-text `TextLayout` is still built, on the
     first caret query, for carets and selection until step 5; flui-painting
     `ARCHITECTURE.md`, mapping decision 15, records what differs meanwhile.
   - (4a) The `parley` and `parley-layout` features are removed: Parley, swash and the raster
     side are in the default build, and nothing selects cosmic-text measurement. The host-face
     feed (step 3c) is therefore unconditional too.
   - (4a) With `bundled-fonts`, the process font system installs the bundled Roboto, Material
     Icons and CupertinoIcons in place of any host face of those names (the collection measures
     those families in the bundled faces alone), binds its generic families to Roboto, as the
     collection does, and
     snaps a weight the family lacks to one it has, so the caret layout, on cosmic-text until
     step 5, lays default-family and generic text out in the face it was measured in (mapping
     decision 16); paint draws the measured Parley runs (4b), a bold Roboto lacks synthesized
     rather than snapped (mapping decision 18). The fed collection
     binds its generics to the families the process font system binds them to, so both sides
     name Roboto.
   - (4a) Merge gate. Face agreement, decided by the owner on 2026-09-30: host faces go into
     the collection first (step 3c), rather than accepting that non-Latin and named host
     families measure in Roboto or with no face while they paint in host faces, or landing 4a
     with 4b. With 3c merged, the cases that diverged measure what they paint on the Windows
     development host at 16 px: Cupertino's chain 161.16 px (was 163.29 measured against
     161.16 painted), a style naming `Segoe UI` 161.16 px (was the same pair),
     `你好世界 emoji 😀` 133.27 px (was 82.77 against 133.27), `你好世界` 64.00 px, `😀`
     21.97 px, and Latin in the default family, bold and monospace 163.29 px, each measured and
     painted alike, within the 0.05 px of `host_faces_oracle`, whose rows run on CI's Linux host
     too. That half of the gate is closed. The other half, the ordering, is met: the registration
     half of step 3b (the font-collection-changed event, and `register_font` moving to the
     collection) merged first, so a face registered at run time through the one door reaches
     measurement, paint and carets on the next frame.
   - (4a) Merge gate, line breaks: closed by the owner on 2026-09-30, by landing 4a with 4b,
     and `"A\n"` is two lines, as Parley lays it out. CR LF breaks once, as the owner decided
     on 2026-09-30: `"A\r\nB"` is two lines and `"A\r\n"` two, like `"A\n"`. Parley alone
     breaks at the CR and again at the LF, so the painter hands it each CR that directly
     precedes an LF as a space, the CR's one byte, which keeps every byte offset into the
     layout valid for the text it was given. What was measured before the decision: soft wrapping agrees: Parley shapes
     with `OverflowWrap::BreakWord`, so an overlong word breaks between glyphs as cosmic-text's
     `Wrap::WordOrGlyph` breaks it. Over ten paragraphs (Latin, Cyrillic, Arabic, CJK, emoji,
     URLs) at 12, 14 and 17 px and every width from 2 to 398 px in 3 px steps, 668 of 3990
     measured a height other than the one painted without it and 30 with it, all 30 at 2 px,
     narrower than a space. Hard breaks do not agree: the two shapers break at different
     characters and read a trailing break differently (mapping decision 15 lists them), and
     no setting of either shaper aligns them. With paint on Parley that difference moves
     to the caret layout until step 5 (mapping decision 15).
   - *Acceptance (4a):* measurement is Parley's in the default build, at the painter
     (`text_context_contract`, and a face registered on the collection reaches measurement,
     paint and carets at the next layout,
     `a_face_registered_on_the_collection_reaches_measurement_paint_and_carets`) and at the UI runtime (a face registered on one UI runtime's collection sizes
     that UI runtime's paragraph); measured and painted metrics agree on the bundled Roboto, named,
     as the default family and as the monospace generic, regular and bold, and a host face named
     "Roboto", "Material Icons" or "CupertinoIcons" does not replace the bundled one; on the host's faces they agree for every row of
     `measured_width_equals_painted_width_on_host_faces`, a family the host names exactly among
     them; the `wasm32` lane, `cargo xtask deps` and `cargo xtask reach` are green.
   - (4b) `DrawOp::Paragraph` carries flui-painting's `Arc<ShapedParagraph>`: a face table of
     FLUI-owned font blobs (§4), and runs naming a face in it, a size, raw variation
     coordinates and synthesis, with glyph ids, logical positions and span colour. It replaces
     `Arc<TextLayout>`. The paragraph is built from the shaped layout that measured, so
     "painted as measured" holds by identity; the ellipsis is shaped into it on Parley. Within
     its box each line is aligned by the paragraph's direction (flui-painting `ARCHITECTURE.md`,
     mapping decision 18, which lists how paint now differs from the cosmic-text paint it
     replaces: synthetic bold, hard breaks, per-cluster fallback, right alignment).
   - (4b) The engine's atlas is `GlyphAtlas<SwashRasterizer>`, and the atlas's default
     parameter is gone. Recording a paragraph registers each run's face in the rasterizer's
     registry and places the run's glyphs; the engine no longer takes `FONT_SYSTEM`'s lock to
     rasterize. `WgpuPainter::draw_text` is gone. The performance overlay's labels are shaped
     through the UI runtime's `TextContext` at scene assembly: `PerformanceOverlayLayer::record`
     composes them into the display list the layer carries, and the engine clips it to the
     overlay's bounds and replays it, rasterizing the labels like any other paragraph.
   - (4b) The cosmic-text paint path is removed: `TextLayout::placed_glyphs` and its ink
     bounds, `SharedFontSystem::rasterize` and its `GlyphRasterizer` impl, and the cosmic
     `GlyphKey`, whose name `ParleyGlyphKey` takes.
   - (4b) No rollback for measurement or paint: with the features and the cosmic-text paint
     path removed, no flag brings cosmic-text measurement or paint back, and a paint regression
     found after this step is fixed forward. Step 5 moved carets and selection with none
     either.
   - *Acceptance:* the `DrawOp` payload is one `Arc`; the text readback suite passes
     unmodified; `the_engine_does_not_shape` is extended so the engine's manifest names no
     parley, fontique, skrifa, swash or cosmic-text. Glyph baselines round as today
     (`(run.line_y * scale).round()`), pinned against today's output (gate 8, second half). A
     registry test: a source-cache prune while the registry holds the blob keeps keys equal, and
     fails without the registry.
5. **Carets, selection, hit-testing and boundaries read the Parley layout.** Landed.
   - `TextPainter` keeps the `ParagraphLayout` that measured and painted, and every cursor
     query reads it (`parley_text/caret.rs`), in the painted box's coordinates: each cluster
     edge takes the per-line shift paint gives the line's glyphs. The cosmic-text `TextLayout`,
     `Shaper`, `ResolvedFont` and `SharedFontSystem`'s shaping door are gone, and so is the
     weight snap that only served them. Flutter differs in places; flui-painting
     `ARCHITECTURE.md`, mapping decision 15, records each:
     - a caret is per scalar: inside a cluster of several scalars (a combining mark, a ZWJ
       sequence, a ligature) it is a proportional slice, so an input method's scalar-addressed
       rect query ([ADR-0090](ADR-0090-ime-pull-text-store-contract.md)) gets one;
     - a hit snaps to the nearest ICU4X grapheme boundary on the hit line, so it never lands
       inside a grapheme or between CR and LF;
     - a selection box covers a stretch of one bidi direction on one line and carries that
       run's direction, and is as tall as the line box;
     - at a soft wrap `Downstream` puts the caret at the next line's start and `Upstream` at
       the previous line's end; after a hard break, a trailing one included, the caret starts
       the next line;
     - a caret or hit never reaches past the kept text: dropped lines and an appended ellipsis
       answer the kept text's end.
   - Word boundaries come from ICU4X's word segmenter for non-complex scripts, over the text,
     with FLUI's tie-break; the layout's cluster flags come from the same segmenter but hold a
     space where the text has a CR. Parley's `complex-scripts` feature stays off (gate 4): no
     dictionary or LSTM data, so CJK and Thai word selection stays per character.
     `unicode-segmentation` leaves flui-painting; the editor's keyboard word and grapheme steps
     in `flui-widgets` keep it until step 6c.
   - A registration loads the collection alone: nothing lays text out on the process font
     system, so the collection keeps no caret side, and `SharedFontSystem` loses `add_face`
     and `generation`.
   - No rollback flag: a cosmic-text caret layout over Parley paint is itself the divergence
     this step removes, and measurement and paint have had no rollback since step 4.
   - `pub use cosmic_text::fontdb::Family` goes: once `ResolvedFont` is gone, no public
     signature names it, so FLUI needs no family type of its own to replace it.
   - Not in this step: the bidi base direction (Parley 0.11.1 still takes it from the first
     strong character, mapping decision 12; setting it needs a leading mark and an offset map
     through every query, its own step), and the host scan off the owner thread (step 6,
     which rewrites discovery). The `system` feature question is settled: `fontique/system`
     reaches `windows`, which tier S forbids, and needs fontconfig headers on Linux, so it is
     not used.
   - *Acceptance:* gate 2 in part: the `emoji_zwj` difference by the grapheme rows
     (`a_combining_mark_is_one_hit_target`, `a_zwj_family_is_one_hit_target`, which also pass
     on the cosmic-text path, so they pin the behaviour rather than prove the change); the
     `arabic_mixed` difference is neither pinned nor enumerated yet. Gate 3 by `caret_contract`
     and by the existing selection, tap, drag and double-tap tests and `text_store_kit`,
     passing unchanged; gate 4's `complex-scripts` decision above; gate 5 by the row
     `a_missing_family_never_takes_its_space_from_an_emoji_face` of `family_resolution`
     (flui-painting `src/text_layout/context.rs`), which shapes on Parley and turns red when
     the family reaches Parley unresolved. Gates 2 (`arabic_mixed`), 6 and 7 stayed open until
     step 6a.
6. **cosmic-text removed; `FONT_SYSTEM` leaves.** Split in three on 2026-09-30: removing the
   crate, moving discovery off the owner thread and moving the editor to ICU4X touch different
   crates and fail in different ways, so each is its own change.
   - (6a) Landed. cosmic-text and `unicode-script` leave the workspace, and `parking_lot` leaves
     flui-painting. Host discovery is fontdb's, a direct dependency: `HostFonts::scan` scans once
     and picks the generic families and the fallback lists for the host locale (`sys-locale`),
     and the app's shared engine services feed the collection from it
     (`FontCollection::with_host_fonts`), still synchronously. FLUI owns the per-platform
     fallback tables (`fallback_tables`, ported from cosmic-text 0.19 under its MIT OR
     Apache-2.0 licence), keyed by ISO 15924 code, with no `static`. `FONT_SYSTEM`,
     `shared_font_system()`, `SharedFontSystem`, the emoji-forbidden cosmic fallback, the test
     doors that pinned the process font system (`init_font_system_with_faces`,
     `flui_testing::pin_font_faces`) and the process-side Roboto binding go; the feed keeps
     decision 16 by binding only a generic the collection leaves unbound. swash takes its
     `render` feature directly, which cosmic-text used to switch on for it. The raster oracle
     and the platform lists were recorded from cosmic-text before it left, as literals.
   - *Acceptance (6a):* `cargo tree -i cosmic-text` and `cargo tree -i unicode-script` are
     empty; the globals allowlist is shorter by `text_layout::layout::FONT_SYSTEM`; the
     `disallowed_types` `#[expect]` is gone; swash on Parley's keys draws every recorded
     cosmic-text bitmap (`swash_matches_the_recorded_reference`); every platform's fallback
     table equals the recorded lists on any host (`platform_tables_match_the_recorded_lists`),
     but for a regional locale's Han list, picked by its subtags
     (`a_platform_chain_picks_han_by_locale`);
     a host copy of a bundled family is never fed and a bound generic never rebound
     (`a_host_copy_of_a_bundled_family_is_not_fed`,
     `a_missing_path_is_skipped_and_the_feed_completes`); the app's collection is the host-fed
     one (`the_runtime_launches_one_host_feed_for_every_ui_runtime`); the demo snapshots and the perf
     counts are unchanged without the pin; `cargo xtask deps`, `reach`, `globals` and the wasm32
     lane are green; §§1–5 are accepted and the back-links in Consequences are written.
   - (6b) Landed. The feed moves off the owner thread: the first frame renders with the bundled
     faces, and the host's arrive as a registration's do, through the collection's generation.
     `FontCollection::with_host_feed` returns the bundled collection and a `HostFontFeed`; the
     app's shared engine services launch it on a `flui-host-fonts` thread once the fonts the app
     registered before the start are in, and every top-level owner turn
     (`dispatch_platform_ui_runtime`) tells every UI runtime when the generation moved since the last
     notice, which is also how a registration is announced. The feed registers one file at a
     time, reads each source on a scratch collection first and raises the generation once if it
     changed the collection, and on unwind (§7).
     *Acceptance (6b):* a first-frame test renders bundled text before the scan completes, and
     text styled with a host-only family re-lays out when the feed lands. Met by the rows
     `the_first_frame_renders_bundled_text_before_the_host_feed_lands` and
     `text_in_a_host_only_family_re_lays_out_when_the_feed_lands` of flui-runtime's
     `font_registration_matrix`, with flui-painting's `host_feed_contract`
     (`a_host_feed_raises_the_generation_once`,
     `a_family_held_before_the_feed_is_not_fed_again`,
     `a_feed_that_adds_nothing_leaves_the_generation_alone`) and
     `a_source_that_panics_is_skipped_and_the_collection_stays_usable`, flui-app's
     `font_collection_contract` (`the_runtime_launches_one_host_feed_for_every_ui_runtime`,
     `the_host_feed_runs_off_the_owner_thread_and_wakes_once`) and the row
     `a_landed_host_feed_wakes_every_ui_runtime_window` of `owner_dispatch_matrix`.
   - (6c) Landed. The editor's grapheme and word steps in `flui-widgets` (`controller.rs`,
     `editable_text.rs`, `text_store.rs`) move to the ICU4X boundaries in flui-painting, and
     `flui-widgets` drops its direct `unicode-segmentation` dependency, which completes §6.
     The boundaries are public as `flui_painting::text_boundaries` (grapheme steps, cluster
     ranges and word segments, each query segmenting from the start of its line), and
     `deny.toml` bans `unicode-segmentation` for every crate but `winit` and `convert_case`.
     *Acceptance (6c):* no FLUI crate depends on `unicode-segmentation`; `text_store_kit` and the
     editor's tap, drag, double-tap and keyboard tests pass unchanged. Met: `cargo tree -i
     unicode-segmentation` names only `winit` and `convert_case`, and `cargo deny check bans`
     fails if a FLUI crate names it again; the `text_editing` table, the controller's unit
     tests and `text_store_kit` pass unchanged. The new row
     `the_editor_steps_the_graphemes_the_painter_snaps_to` compares the editor's stops with
     the painter's clusters over a ZWJ family, flags, stacked combining marks, CR LF and the
     conjunct "क्षि"; `unicode-segmentation` 1.13.3 clusters each of those the same way, so the
     row pins the shared contract rather than a difference.

## Alternatives considered

- **Stay on cosmic-text with per-UI runtime `FontSystem`s.** Rejected as the target: each UI runtime would
  scan and hold its own database, and the shaping and memory gap ADR-0077 measured stands. Gate 1
  is met, so this is no longer the fallback path; it returns only if a migration gate fails
  (below).
- **glifo as the rasterizer.** Not adopted now (§5).
- **A per-UI runtime mutable collection.** Rejected: every UI runtime would load the same faces, and a
  shared glyph atlas could not key on them.
- **OS text stacks in production (DirectWrite, Core Text).** Rejected: layout would differ by
  platform. Only rasterization and hinting may vary (§5).
- **Keep `Arc<TextLayout>` on the display list.** Rejected: it ties the engine and any second
  backend to the shaper, and a Parley `Layout` is not a stable wire type.
- **A `flui-text` crate now.** Rejected until measured (§9).

## Consequences

- `FONT_SYSTEM` and `flui_painting::shared_font_system()` were deleted at §10 step 6a, with
  cosmic-text, and with them the last process-global font state: the lock on every measurement
  and glyph rasterization had already gone with steps 4 and 5. `SharedEngineServices` constructs
  the collection (since §10 step 2b) and owns the one host scan (since step 6a), a value dropped
  once the collection is fed.
- Every capability context that measures text gains an explicit font-context handle; test
  bootstraps construct one.
- The first frame has the bundled faces only: since §10 step 6b the scan and the feed run off
  the owner thread, which pays about 17 µs for them before the first frame (Context, gate 6).
  A text run styled with a host-only family, or in a script only a host face covers, is laid
  out in a fallback face first and again when the feed lands; that visible swap is the price of
  taking the scan off the startup path. Every text node is laid out once more when it lands,
  including text whose face did not change, and while the feed runs some host faces may
  already shape before the generation rises.
- A laid-out paragraph keeps about 66 bytes per char (Context, gate 6), so a long document is
  laid out by a virtualized list, not held whole.
- `pub use cosmic_text::fontdb::Family` left at §10 step 5, when no public signature named it
  any more: one fewer upstream type on a public path
  ([ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)).
- The engine names no shaper crate; its glyph atlas moves from each painter to the `GpuContext`.
- The editor's segmentation changed data source at §10 step 6c: its grapheme steps and word
  jumps walk ICU4X's boundaries, the ones hit-testing snaps to. No editor test moved; where
  `unicode-segmentation` and ICU4X would disagree, the editor now follows the painter.
- **Every gate is met (§8), so the fallback this record kept no longer applies:** cosmic-text
  staying the shaper, with ADR-0016 and ADR-0059 in force and §§2–5 re-scoped over it. cosmic-text
  is gone (§10 step 6a).
- **Back-links, written at §10 step 6a.** ADR-0077, ADR-0016 and ADR-0059 name this record as
  superseding them, in their Status lines and a `Superseded by` line; ADR-0065, ADR-0066 and
  ADR-0067 carry an `Amended by` line naming it.

## Verification

The gate 1 prototype exists on `spike/parley_atlas` (not merged). Everything below exists.

- The raster seam, `GlyphKey`, `FontRegistry` and `SwashRasterizer` (`flui_painting::glyphs`),
  with the oracle (`crates/flui-painting/tests/parley_oracle.rs`): since §10 step 6a,
  `swash_matches_the_recorded_reference` compares every distinct key of its Latin, Cyrillic,
  Greek and icon samples at 13, 18 and 32 px and four bins with what cosmic-text's scaler drew,
  recorded (`crates/flui-painting/tests/support/raster_recorded.rs`, 1,188 bitmaps).
- The gate 1 prototype and its oracle glyph tests, with the glifo/skrifa choice recorded.
- `FontCollection` and `TextContext` exist, with `crates/flui-painting/tests/text_context.rs`:
  `two_ui_runtimes_shape_in_parallel` (two contexts over one collection shape on two threads whose
  intervals overlap, with equal metrics) and `a_face_registered_after_the_fork_shapes_in_every_ui_runtime`
  (fails when the collection is not shared). No process font system exists to build since
  §10 step 6a, which `cargo xtask globals` makes structural.
- FLUI's text path names no `Mutex` or `RwLock`: clippy `disallowed_types` in
  `crates/flui-painting/clippy.toml`, with no `#[expect]`ed site since §10 step 6a. fontique's
  own locks are outside that check (§3).
- Runtime tests (§10 step 2b), in `crates/flui-runtime/src/ui_runtime/tests/text_context.rs`:
  `two_ui_runtimes_hold_contexts_over_the_one_collection_they_were_given`,
  `dropping_a_ui_runtime_releases_its_text_context` and `a_second_presentation_adds_no_text_context`;
  in flui-app, `isolated_windows_shape_over_the_runtimes_font_collection` (through
  `build_ui_runtime`, the one call every runner site builds its UI runtime with) and the rows of
  `font_collection_contract`: `the_runtime_launches_one_host_feed_for_every_ui_runtime` (repeated
  calls on one runtime return the collection the services own, one feed is launched for it,
  and it is host-fed once that feed runs, `testing::host_fed`) and
  `two_runtimes_hold_different_collections`.
- Layout measures through the UI runtime's context (§10 step 3a): in
  `crates/flui-runtime/src/ui_runtime/tests/text_context.rs`,
  `two_ui_runtimes_measure_text_through_their_own_contexts` and
  `every_presentation_pipeline_holds_the_ui_runtimes_text_context`; in
  `crates/flui-painting/tests/text_painter_unit.rs`,
  `measurement_follows_the_context_it_is_given` and
  `a_registration_on_the_collection_invalidates_the_painter_cache`; in
  `crates/flui-rendering/tests/text_context.rs`,
  `a_layout_that_panics_while_holding_the_text_context_releases_it` and
  `intrinsic_and_dry_queries_measure_through_the_pipelines_context`.
- Every pipeline is built with a text context (§10 step 3b): the `compile_fail` doctests on
  `PipelineOwner::new`; in `crates/flui-rendering/tests/text_context.rs`,
  `a_taken_pipeline_leaves_an_owner_that_measures_through_the_same_context`; in
  `crates/flui-hot-reload/tests/plugin_pipeline_text.rs` (under `app-plugin`),
  `a_plugin_pipeline_measures_through_the_context_it_is_given`, and in
  `plugin_pipeline_layout.rs`, `a_plugin_pipeline_lays_out_at_the_size_of_each_frame`.
- The host's faces in the collection (§10 step 3c): in
  `crates/flui-painting/tests/host_faces_oracle.rs`,
  `measured_width_equals_painted_width_on_host_faces` and
  `every_family_the_host_scan_finds_resolves_in_the_collection`; in
  `crates/flui-painting/src/text_layout/fallback_chain.rs`,
  `fontique_fallbacks_follow_the_chain_in_order`,
  `the_sans_serif_family_ends_every_script_fallback` and
  `the_emoji_generic_is_the_common_list`; in `crates/flui-painting/src/parley_text/shape.rs`,
  `a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection`; the row
  `a_style_resolves_by_the_family_rule` of `family_resolution` (`context.rs`);
  `a_missing_path_is_skipped_and_the_feed_completes` (`context.rs`); in
  flui-app, `the_runtime_launches_one_host_feed_for_every_ui_runtime` (`runtime.rs`).
- Registration re-lays out text (§10 step 3b): in flui-runtime's `font_registration_matrix`,
  `a_face_registered_after_start_re_lays_out_text_in_every_ui_runtime_on_the_next_frame`,
  `a_face_registered_before_the_ui_runtime_is_built_measures_on_its_first_frame` and
  `a_font_change_notice_requests_a_redraw_for_every_presentation`; flui-rendering's
  `font_change_contract` (`tests/text_context.rs`); flui-painting's `registration_contract`
  (`context.rs`); in flui-app's `owner_dispatch_matrix`,
  `a_registration_notifies_every_ui_runtime_window`,
  `a_duplicate_registration_is_refused_and_notifies_nothing`,
  `bytes_with_no_face_are_refused_and_notify_nothing` and
  `a_registration_from_inside_a_realm_task_reaches_that_ui_runtime_after_it_returns`.
- A registry test: a source-cache prune while the registry holds the blob keeps keys equal, and
  an arm without the registry shows the keys change
  (`a_held_blob_keeps_its_keys_across_a_prune`, `crates/flui-painting/src/text_layout/context.rs`).
- A same-key-twice test: rasterizing the same keys twice on one rasterizer yields equal bitmaps
  (`rasterizing_a_key_twice_draws_the_same_bitmap`, `crates/flui-painting/tests/parley_oracle.rs`).
- A baseline test against today's rounding, measurement side:
  `crates/flui-painting/tests/parley_metrics_oracle.rs` measures the bundled Roboto
  at 13, 14, 16, 18 and 32 px, default and 1.5 line height, and finds the width, height and
  device baseline, `(baseline * scale).round()` at scales 1, 1.25, 1.5 and 2, that the
  cosmic-text layout measured; since step 5 those numbers are literals recorded from it before
  it was removed. It holds because Parley's layout is unquantized: quantized, Parley rounds ascent,
  descent and the leading halves to whole logical pixels, and seven of the forty cases landed
  on another device row (16 px at 1.5 line height: 17 against 17.47, so 34 against 35 at
  scale 2). The glyph side (step 4): the same test compares the device row the painted runs
  place the first baseline on (`ShapedRun::placed_glyphs`) with cosmic-text's, and
  `a_2x_baseline_row` reads it back.
- `the_engine_does_not_shape` ([ADR-0067](ADR-0067-engine-owned-glyph-atlas.md)) extended so the
  engine's source and manifest name no Parley, fontique, skrifa, swash or cosmic-text crate.
- The process-global state gate ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) with
  `FONT_SYSTEM` removed from its allowlist (§10 step 6a).
- The feed off the owner thread (§10 step 6b): in flui-runtime's `font_registration_matrix`,
  `the_first_frame_renders_bundled_text_before_the_host_feed_lands` and
  `text_in_a_host_only_family_re_lays_out_when_the_feed_lands`; flui-painting's
  `host_feed_contract` (`tests/font_registration.rs`) and
  `a_source_that_panics_is_skipped_and_the_collection_stays_usable` (`context.rs`); in
  flui-app, `the_host_feed_runs_off_the_owner_thread_and_wakes_once` (`runtime.rs`) and
  `a_landed_host_feed_wakes_every_ui_runtime_window` (`owner_dispatch_matrix`).
- Parley measures in the default build (§10 step 4a): the rows of `text_context_contract`
  (`crates/flui-painting/tests/main.rs`); in `crates/flui-painting/tests/font_registration.rs`,
  `a_face_registered_on_the_collection_reaches_measurement_paint_and_carets`; in
  `crates/flui-runtime/src/ui_runtime/tests/text_context.rs`,
  `a_ui_runtime_measures_text_with_the_faces_of_its_own_collection`; the default-family, monospace
  and bold rows of `parley_metrics_round_to_todays_baseline`, which fail if any of them
  measures in a face other than the bundled Roboto Regular;
  `a_host_copy_of_a_bundled_family_is_not_fed` (`crates/flui-painting/src/text_layout/context.rs`); every row
  of `measured_lines_are_painted_lines` (`crates/flui-painting/tests/parley_metrics_oracle.rs`)
  and `an_empty_paragraph_measures_a_line_of_its_style`; and,
  for the merge gate's face agreement, every row of
  `measured_width_equals_painted_width_on_host_faces`
  (`crates/flui-painting/tests/host_faces_oracle.rs`), which fails on a bundled-only collection.
  Since step 5 its rows compare measured with painted, equal by identity, and fail on a glyph
  painted as `.notdef`; the host-dependent comparison with the cosmic-text layout left with it.
- Paint draws the runs of the layout that measured (§10 step 4b): the hard-break rows of
  `measured_lines_are_painted_lines` (`a_trailing_newline_is_a_line`, `crlf_breaks`,
  `a_line_separator_breaks`, `a_paragraph_separator_breaks`, `next_line_does_not_break`);
  `truncated_paragraph_paints_what_it_measured` (`crates/flui-painting/tests/text_overflow_unit.rs`),
  whose ellipsis line inks within the measured width; the synthetic bold, oblique, multi-line
  and host-face rows of `paragraph_extent_covers_every_rasterized_glyph`
  (`crates/flui-painting/tests/damage_extent.rs`); and the readback table
  `parley_runs_read_back` (`crates/flui-engine/src/paragraph_readback_tests.rs`):
  `latin_breaks_at_a_line_separator`, `synthetic_bold_inks_more_than_regular`,
  `cjk_breaks_at_a_line_separator`, `colour_emoji_on_line_two`,
  `arabic_rtl_right_aligns_each_line`, `crlf_puts_b_on_line_two` and `a_2x_baseline_row`.
- Carets, selection and hit-testing read the Parley layout (§10 step 5): the table
  `caret_contract` (`crates/flui-painting/tests/main.rs`, rows in `caret_contract.rs`):
  `caret_position`, `two_space_run_word_boundary`, `a_combining_mark_is_one_hit_target`,
  `a_zwj_family_is_one_hit_target`, `rtl_paragraph_carets_run_right_to_left`,
  `mixed_bidi_boxes_carry_their_run_direction`,
  `a_trailing_newline_puts_the_caret_on_the_empty_line`, `crlf_is_one_break_for_carets`,
  `multi_line_selection_boxes_follow_their_line`, `carets_sit_on_the_painted_glyphs`,
  `a_soft_wrap_caret_follows_its_affinity` and `truncated_carets_stay_in_kept_lines`;
  `word_boundaries_agree_with_the_layouts_clusters` (`crates/flui-painting/src/parley_text/caret.rs`);
  the readback row `selection_highlights_the_second_line` of `parley_runs_read_back`; the
  registration rows `a_registration_reaches_the_collection` and `bytes_with_no_family_are_refused` of
  flui-painting's `registration_contract`. Unchanged and passing: `text_store_kit`
  (`crates/flui-widgets/tests/`), the `editable_text` tap, drag and double-tap tests, and the
  `harness_editable_*` rows of `render_object_harness`.
- The `arabic_mixed` difference (gate 2): `a_lam_alef_ligature_is_one_glyph_and_two_caret_stops`
  in `caret_contract` (`crates/flui-painting/tests/caret_contract.rs`), on the generated
  `FLUI Probe Arabic` face (`crates/flui-painting/assets/fonts/probe-arabic-ligature.ttf`).
- Host fonts scanned by the app, cosmic-text gone (§10 step 6a): in
  `crates/flui-painting/src/text_layout/fallback_chain.rs`,
  `platform_tables_match_the_recorded_lists` (every platform's table against the lists recorded
  from cosmic-text 0.19, on any host) and `a_platform_chain_picks_han_by_locale` (a regional
  tag such as `ja-JP` picks its language's Han list); in
  `crates/flui-painting/src/text_layout/context.rs`, `a_host_copy_of_a_bundled_family_is_not_fed`
  and `a_missing_path_is_skipped_and_the_feed_completes`, which fail if the feed adds a host copy
  of a held family or rebinds a bound generic; `swash_matches_the_recorded_reference`; the demo
  layer snapshots (`cargo xtask demo-snapshots`) and the perf counts, unchanged with the font
  pin removed.
- Startup cost and retained memory (gate 6): `crates/flui-painting/benches/text_startup.rs`, run
  with `cargo bench -p flui-painting --bench text_startup`; its figures are in Context.
