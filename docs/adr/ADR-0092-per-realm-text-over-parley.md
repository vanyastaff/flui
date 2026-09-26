# ADR-0092: Text shapes per realm over Parley and crosses the display list as neutral shaped runs

- **Status:** Proposed — gate 1 (§8) met by a prototype on 2026-09-26 (see Context). A passed
  gate is evidence, not shipped behaviour: the record is accepted section by section as the text
  migration lands §§1–7, and gates 2–8 bind those changes. The three supersessions below take
  effect together, when §§1–5 are accepted; a section accepted before then supersedes nothing.
- **Date:** 2026-09-25
- **Revised:** 2026-09-26 (rasterization prototype; see Context)
- **Supersedes (when §§1–5 are accepted):** [ADR-0077](ADR-0077-migrate-to-parley.md)
  (absorbed: its direction, its preconditions and its "If later Rejected" branch are carried
  here)
- **Supersedes (when §§1–5 are accepted):**
  [ADR-0016](ADR-0016-unified-font-system-registration.md),
  [ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md)
- **Amends (on acceptance):** [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  (Part 1: the process-wide font doors become a per-realm context; Part 2: the `Paragraph`
  payload), [ADR-0066](ADR-0066-display-list-command-representation.md) (`DrawOp::Paragraph`),
  [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (`GlyphKey`, the rasterization door, where the
  atlas lives; the atlas's key type becomes a `GlyphRasterizer` parameter)
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the text store answers geometry from this
  layout), [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) (the raster
  thread and the `GpuContext` atlas), [ADR-0097](ADR-0097-no-process-global-state-gate.md)
  (`FONT_SYSTEM` leaves the allowlist here)
- **Refs:** decision D12 in the [decision index](../../design/decisions.md); roadmap exit B1
  (per-realm fonts)

## Context

Text is one process-wide, locked object today.

- `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`
  (`crates/flui-painting/src/text_layout/layout.rs:124`, a `parking_lot::Mutex`, `:18`) holds
  the cosmic-text `FontSystem`, its scaler and a database generation. Every measurement on every
  realm and every glyph rasterization takes that lock: `SharedFontSystem::rasterize` locks it and
  rasterizes with the scaler kept beside the database (`layout.rs:347-366`).
- Its initializer scans the host's fonts synchronously (`FontSystem::new()`, `layout.rs:147`),
  then rebuilds the system around FLUI's emoji-forbidding fallback (`layout.rs:161-164`,
  `font_resolve.rs:277`). The runtime forces that initialization while constructing its shared
  services (`crates/flui-app/src/app/runtime.rs:148`), so the full scan sits before the first
  frame.
- The glyph key is cosmic-text's own: `pub struct GlyphKey(pub(super) cosmic_text::CacheKey)`
  (`crates/flui-painting/src/text_layout/glyphs.rs:23`), stable only because the process-wide
  database is append-only.
- The display list carries the shaper's layout: `DrawOp::Paragraph { layout: Arc<TextLayout>, … }`
  (`crates/flui-painting/src/display_list/command.rs:167-174`), where `TextLayout` wraps
  cosmic-text's buffer. The crate still re-exports `cosmic_text::fontdb::Family`
  (`crates/flui-painting/src/lib.rs:87`).
- Every painter builds its own glyph atlas over that shared system
  (`crates/flui-engine/src/painter/mod.rs:163-167`), one per window.
- `register_font` appends a face and bumps the generation (`layout.rs:391-402`); the family
  resolver rebuilds on the new generation (`font_resolve.rs:641-662`), but nothing marks text
  render objects for layout. [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  records this as a named gap and defers "a `FontContext` handle threaded through layout" until a
  second font source or closing the ambient-reach ratchet becomes a requirement. Per-realm fonts
  (roadmap B1) and the no-process-global-state gate
  ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) are both of those triggers.
- Unicode data comes from three places: cosmic-text's own dependencies, `unicode-segmentation`
  (`crates/flui-painting/Cargo.toml:45`, `crates/flui-widgets/Cargo.toml:50`) and
  `unicode-script` (`crates/flui-painting/Cargo.toml:40`). Editor boundaries and shaper
  boundaries can disagree.

[ADR-0077](ADR-0077-migrate-to-parley.md) proposed moving to Parley on the strength of a spike
(`tools/text-spike`, `parley = "=0.11.1"` at `tools/text-spike/Cargo.toml:23`) that measured
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
- **Shape.** A per-realm `ParleyText` over fontique's shared `Collection` and
  `SourceCache::new_shared`; `fork()` gives a second realm a context over the same fonts. The
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
- **Key stability.** Keys are equal across re-shaping and across forked realms. They survive
  source-cache prunes only while the raster-side registry holds the blob: fontique's shared
  `SourceCache` holds `WeakBlob`s, so an unheld blob reloads under a new id (0 became 1 in the
  prototype) and every key for that font changes.
- **Locks.** FLUI adds no lock. fontique's shared mode takes internal mutexes on local cache
  misses: `Collection::family` locks `system.fonts` (fontique `collection/mod.rs:326`) and
  `SourceCache::get` locks the shared cache (fontique `source_cache.rs:104`); those locks are
  shared across forked realms. Gate 1 only requires the rasterizer to run outside shaping, which
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

## Decision

### 1. Parley shapes and lays out, one `Layout` per paragraph

`flui-painting` moves from cosmic-text to Parley. Each `RenderParagraph` shapes its own `Layout`;
a multi-paragraph buffer is never one `Layout` (ADR-0077's quadratic `Item`-boundary finding).
FLUI's own family resolution ([ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md)) carries over:
a style's family is resolved by FLUI's policy before shaping, whatever the shaper's fallback does.

### 2. One shared font collection, handed down explicitly

The application owns one fontique `Collection` with `shared: true`. Faces are only ever added,
never removed or changed. It lives in the runtime's shared engine services and is passed to each
realm at construction; no `static` holds it. Registering a font adds it to the collection and
raises a font-collection-changed event on every realm, which marks every text render object in
that realm as needing layout — closing ADR-0065's named gap.

### 3. Per-realm contexts with no FLUI lock

Each realm owns a Parley `FontContext` and `LayoutContext` (over a clone of the collection) as
owner-thread state, used through `&mut` and reached through the layout context explicitly — the
handle ADR-0065 deferred. Layout on one realm never waits on another realm's shaping. FLUI's frame
path takes no lock of its own. fontique's shared collection and source cache lock internally on a
local cache miss (first use of a family or source, or after a registration), and those locks are
shared by every realm's clone. A frame that hits only local caches takes none; that is an
inference from fontique's code, not a measurement.

### 4. A neutral shaped-run contract on the display list

`DrawOp::Paragraph` carries shaped runs defined by `flui-painting`, not a shaper type. A run
names its font by blob identity (blob id plus face index), its size, its normalized variation
coordinates and synthesis, and a list of glyphs (glyph id, position, subpixel bin), with span
colour and decoration as today. Nothing in the run names Parley, fontique, skrifa or cosmic-text,
so the engine and any second raster backend
([ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)) consume runs without a shaper
dependency. "Painted as measured" (ADR-0065) still holds by identity: the runs are produced from
the same `Layout` that measured the paragraph.

Open for the migration:

- The run cannot carry `parley::FontData`. The raster side needs the blob bytes through a
  FLUI-owned blob handle, for example a `FontBlob` id plus a per-frame table of the blobs a frame
  references first. The migration settles the shape, and the engine-names-no-shaper test pins it.
- The prototype did not show neutrality (Context).

### 5. Glyph keys carry font identity; rasterization is a raster-side trait

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
and would rebuild hinting state per glyph. The atlas needs a way to feed new faces to the
rasterizer it owns once built; that door is open, with §4's blob handle.

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
already brings. `unicode-segmentation` and `unicode-script` leave the workspace, and no
`unicode-bidi` is added.

### 7. System fonts load asynchronously

Bundled fonts are in the collection before the first frame. The host font scan runs off the owner
thread, and its faces arrive through the same font-collection-changed event as a registration.

### 8. Acceptance gates

These are ADR-0077's preconditions, carried over and extended. Gate 1 is met (2026-09-26,
Context). Gates 2–8 are conditions on the migration changes.

1. **Rasterization prototype (blocking).** Parley-shaped glyphs rasterize into `GlyphImage`s the
   existing atlas accepts unmodified, for one Latin and one complex-script case, with a
   `GlyphKey` stable across repeated rasterization of the same glyph; no process-global font
   state; the rasterizer runs outside the shaping lock. The prototype evaluates `glifo` against
   hand-written skrifa-outline-to-atlas glue and records the choice.
   Met, with swash standing in for the hand-written glue.
2. **Cluster differences.** The `arabic_mixed` and `emoji_zwj` glyph-count differences from the
   spike are either pinned by a boundary test or enumerated and accepted in writing (ADR-0077
   precondition 2).
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
   not a default one (ADR-0077 precondition 5).
7. **Upstream report.** The `Item`-boundary issue is filed upstream (ADR-0077 precondition 6).
8. **Determinism and baseline.** Rasterizing the same key twice yields equal bitmaps, and glyph
   baselines round as today's path does (`(run.line_y * scale).round()`), pinned against today's
   output.

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
2. **Per-realm font context; `FONT_SYSTEM` leaves.**
   - A FLUI `FontContext` handle, constructed by the runtime's shared engine services (not
     forced at `crates/flui-app/src/app/runtime.rs:148`), goes to each realm and down every
     measuring path explicitly. `shared_font_system()` and the `static FONT_SYSTEM` are
     deleted.
   - Shaping is still cosmic-text behind the handle. The handle's inside is replaced in step 4,
     not its shape.
   - The shared fontique `Collection` is created here, and `register_font` feeds it.
     Registering raises a font-collection-changed event on every realm, which marks text render
     objects for layout (ADR-0065's named gap).
   - *Acceptance:* the globals allowlist is shorter by `text_layout::layout::FONT_SYSTEM`. A
     two-realm test: a font registered through realm A re-lays out text in realm B (fails on
     main). Test bootstraps construct the handle. A registry test: a source-cache prune while
     the registry holds the blob keeps keys equal, and fails without the registry.
3. **Neutral shaped runs on the display list.**
   - `DrawOp::Paragraph` carries flui-painting's `ShapedParagraph`: runs naming a FLUI-owned
     font blob id, face index, size, interned variation and synthesis, with glyph id, position,
     subpixel bin and span colour. It replaces `Arc<TextLayout>`
     (`display_list/command.rs:167-174`). Runs are produced from the same shaped layout that
     measured, so "painted as measured" still holds.
   - A per-frame table carries the blobs a frame names first, which is the door §5 leaves open.
     The engine's atlas becomes `GlyphAtlas<SwashRasterizer>`, the `parley` feature folds into
     the default build, and the atlas's default parameter goes.
   - *Acceptance:* `draw_command_fits_its_budget` holds; the text readback suite passes
     unmodified; `the_engine_does_not_shape` is extended so the engine's manifest names no
     parley, fontique, skrifa, swash or cosmic-text. Glyph baselines round as today
     (`(run.line_y * scale).round()`), pinned against today's output (gate 8, second half).
4. **Paragraph and editable text on Parley.**
   - `TextLayout` shapes one Parley `Layout` per paragraph, using per-realm
     `FontContext`/`LayoutContext` over the shared collection. Carets, selection and
     hit-testing come from clusters.
   - The host font scan runs off the owner thread (§7). This step settles the `system` feature
     question: `fontique/system` reaches `windows`, which tier S forbids, and needs fontconfig
     headers on Linux. Either host discovery goes through the platform layer and feeds the
     collection, or a reach grant is recorded.
   - The cosmic-text path stays behind a flag for one release as the rollback.
   - *Acceptance:* gates 2–7. The existing selection and offset↔cursor tests pass unchanged for
     LTR, RTL and mixed bidi. The `complex-scripts` decision is recorded.
5. **cosmic-text removed.**
   - cosmic-text, `unicode-segmentation` and `unicode-script` leave the workspace.
   - `SharedFontSystem`, `Shaper`, the cosmic `GlyphKey` and `pub use cosmic_text::fontdb::Family`
     go, and the rollback flag is removed.
   - *Acceptance:* `cargo tree -i cosmic-text` is empty; clippy `disallowed_types` rejects
     `Mutex` in flui-painting's text path; §§1–5 are accepted and the back-links in
     Consequences are written.

## Alternatives considered

- **Stay on cosmic-text with per-realm `FontSystem`s.** Rejected as the target: each realm would
  scan and hold its own database, and the shaping and memory gap ADR-0077 measured stands. Gate 1
  is met, so this is no longer the fallback path; it returns only if a migration gate fails
  (below).
- **glifo as the rasterizer.** Not adopted now (§5).
- **A per-realm mutable collection.** Rejected: every realm would load the same faces, and a
  shared glyph atlas could not key on them.
- **OS text stacks in production (DirectWrite, Core Text).** Rejected: layout would differ by
  platform. Only rasterization and hinting may vary (§5).
- **Keep `Arc<TextLayout>` on the display list.** Rejected: it ties the engine and any second
  backend to the shaper, and a Parley `Layout` is not a stable wire type.
- **A `flui-text` crate now.** Rejected until measured (§9).

## Consequences

- `FONT_SYSTEM` and `flui_painting::shared_font_system()` are deleted, and with them the lock on
  every measurement and every glyph rasterization. `SharedEngineServices` constructs the
  collection instead of forcing the font scan.
- Every capability context that measures text gains an explicit font-context handle; test
  bootstraps construct one.
- The first frame renders with bundled faces. A text run styled with a system-only family may
  re-lay out when the scan completes; that visible swap is the price of taking the scan off the
  startup path.
- `pub use cosmic_text::fontdb::Family` is replaced by FLUI's own family type, one fewer upstream
  type on a public path ([ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)).
- The engine names no shaper crate; its glyph atlas moves from each painter to the `GpuContext`.
- The editor's segmentation changes data source; word and grapheme boundaries may shift at the
  edges where `unicode-segmentation` and ICU4X disagree, which gate 3's tests expose.
- **Gate 1 was met; the fallback below applies only if a migration gate (2–8) fails.** Then
  cosmic-text stays the shaper, ADR-0016 and ADR-0059 stay in force, and
  §§2–5 are re-scoped over cosmic-text: per-realm contexts and neutral shaped runs need only
  `&mut` access to a realm's own context, which cosmic-text also allows. The measured Parley
  advantage then stands as an unclaimed opportunity, as ADR-0077 recorded.
- **Back-links when §§1–5 are accepted.** The change that accepts the last of §§1–5 adds
  `Superseded-by: ADR-0092` to ADR-0077, ADR-0016 and ADR-0059, rewrites the Status lines of
  ADR-0016 and ADR-0059 (today "to be superseded by ADR-0077",
  `docs/adr/ADR-0016-unified-font-system-registration.md:3` and
  `docs/adr/ADR-0059-flui-stays-on-cosmic-text.md:3`) to name this record, and adds
  "Amended by ADR-0092" to ADR-0065, ADR-0066 and ADR-0067. Until then the older records are
  unchanged.

## Verification

The gate 1 prototype exists on `spike/parley_atlas` (not merged). The raster seam and the
same-key-twice test exist; the rest do not exist yet.

- The raster seam, `ParleyGlyphKey`, `FontRegistry` and `SwashRasterizer` exist behind
  flui-painting's `parley` feature, with the oracle (`crates/flui-painting/tests/parley_oracle.rs`).
- The gate 1 prototype and its oracle glyph tests, with the glifo/skrifa choice recorded.
- A two-realm test: registering a font in one realm makes text in the other re-lay out. FLUI's
  text path names no `Mutex` (clippy `disallowed_types` in `flui-painting`); fontique's miss-path
  locks are outside that check (§3).
- A registry test: a source-cache prune while the registry holds the blob keeps keys equal, and
  the test fails without the registry.
- A same-key-twice test: rasterizing one key twice yields equal bitmaps
  (`one_key_rasterizes_to_equal_images_twice`).
- A baseline test against today's rounding.
- `the_engine_does_not_shape` ([ADR-0067](ADR-0067-engine-owned-glyph-atlas.md)) extended so the
  engine's manifest names no Parley, fontique, skrifa or cosmic-text crate.
- The process-global state gate ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) with
  `FONT_SYSTEM` removed from its allowlist.
- A first-frame test that renders bundled text before the system scan completes.
