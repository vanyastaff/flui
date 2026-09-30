# ADR-0092: Text shapes per realm over Parley and crosses the display list as neutral shaped runs

- **Status:** Proposed — gate 1 (§8) met by a prototype on 2026-09-26 (see Context); §10 step 1
  landed (the raster seam), and step 2a, the flui-painting half of step 2, landed
  (`FontCollection` and `TextContext` behind `parley`, the lock lint). Step 2b landed:
  flui-app's `SharedEngineServices` builds the collection (one per owner thread, which is one
  per process while [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)
  fixes one owner thread), `UiRealm::new` takes it, and each realm owns a `TextContext` over
  it. §10 step 3a landed: layout, intrinsics and dry queries measure through the realm's
  `TextContext`, lent through each presentation's pipeline; Parley measures behind
  `parley-layout`; registration re-layout is 3b. §10 step 3b's pipeline half landed: every
  `PipelineOwner` is built with a `TextContextHandle`, nothing in layout, intrinsics or dry
  queries builds a context of its own, and the hot-reload plugin pipeline measures over its own
  image's collection; the font-collection-changed event is the other half. §7's host-face feed
  landed ahead of step 4 as step 3c: the app's collection holds the faces, generic families and
  fallback order of the process font system, fed synchronously before the first frame
  (asynchronously once step 3b's event exists). A passed gate is evidence, not shipped
  behaviour: the record is accepted section by section as the text migration lands §§1–7, and
  gates 2–8 bind those changes. The three supersessions below take effect together, when §§1–5
  are accepted; a section accepted before then supersedes nothing.
- **Date:** 2026-09-25
- **Revised:** 2026-09-26 (rasterization prototype; see Context); 2026-09-29 (§10 step 3
  split into 3a and 3b; the realm lends its context through a shared handle; Parley
  measurement behind `parley-layout`; a pipeline is built with its context, and the hot-reload
  plugin image is a realm of its own for text); 2026-09-30 (§7: the host's faces come from the
  process font system's discovery, with one family rule and one fallback order for both
  shapers; §10 step 3c)
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

The application owns one fontique `Collection` with `shared: true`, wrapped as
`flui_painting::FontCollection`. Faces are only ever added, never removed or changed: the handle
offers `register_font` and no removal, so the rule is the API's, not a convention. It lives in
the host's (`flui-app`'s) shared engine services (`SharedEngineServices`, one per owner thread,
which is one per app while ADR-0091 fixes a single owner thread), held in the host's granted
ADR-0097 trampoline cell with no global of its own, and reaches each realm only through
`UiRealm::new`. Registering a font adds it to the collection and raises a
font-collection-changed event on every realm, which marks every text render object in that
realm as needing layout — closing ADR-0065's named gap.

### 3. Per-realm contexts with no FLUI lock

Each realm owns a `flui_painting::TextContext`: a Parley `FontContext` and `LayoutContext` over a
clone of the collection, as owner-thread state, used through `&mut` and reached through the
layout context explicitly — the handle ADR-0065 deferred. Layout on one realm never waits on
another realm's shaping. FLUI's frame path takes no lock of its own; clippy `disallowed_types`
rejects a `Mutex` or `RwLock` in `flui-painting` (the cosmic-text path's `FONT_SYSTEM` is the one
`#[expect]`ed site until §10 step 6).

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

So the one lock left is one shared-mutex acquisition per realm on its first query after a
registration; a frame with no registration behind it takes none. That is a reading of fontique's
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

### 7. System fonts come from one discovery, resolve by one rule and fall back in one order

Bundled fonts are in the collection before the first frame. The host's faces come from the
process font system's own discovery (fontdb's scan inside cosmic-text's `FontSystem::new`), not
from a second one: `FontCollection::with_host_faces` takes a snapshot of that database under its
lock (file paths, in-memory fonts, family names, generic families, fallback lists) and reads the
files into the collection outside it. fontique's `system` feature is not used (§10 step 5).
The collection is fed once per app, in the host's shared engine services; realms only clone it.

Measurement and paint must pick the same face for the same text, so the two shapers share:

- **one family rule**: `resolve_family_name` (ADR-0059's rule) resolves a style against the fonts
  each side holds, and Parley is handed that one family, with nothing after it. A side holds a
  family only when spelled exactly as its fonts name it: fontdb matches names exactly, so the
  Parley side narrows fontique's case-insensitive lookup to the same question;
- **one fallback order**: past that family, cosmic-text walks the script's platform list and then
  the common list; the same lists (`FallbackChain`, built once beside the process font system)
  are written into the collection as each script's fallback families, with the common list
  after the script's own, and as the emoji generic. The family the sans-serif generic names ends
  every script's list, standing in for cosmic-text's last resort (any face not forbidden), which
  fontique lacks;
- **one set of generics**: the collection's generic families name the families the process font
  system binds them to, and system-ui names sans-serif's.

A face whose family the collection already holds (the bundled faces) is not fed again. Until the
font-collection-changed event exists (§10 step 3b) the feed runs synchronously on the owner
thread before the first frame, because text measured before a later feed would keep its old
measurement; with the event it moves off the owner thread and its faces arrive through that event
as a registration does. On wasm32 fontdb finds no host fonts and the platform has no common
list, so the collection holds the bundled and registered faces and every script falls back to the
sans-serif family alone. A bundled-only collection (`FontCollection::new()`) falls back to Roboto
for every script, as cosmic-text's last resort does over the bundled faces.

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
2. **Per-realm text context**, in two halves that land separately: 2a in `flui-painting`
   (the types, the lint and their tests), 2b in the runtime (the bullet on shared engine
   services and `UiRealm::new`). 2a and 2b have landed.
   - `flui_painting::FontCollection` wraps the shared fontique `Collection` (no host scan; with
     `bundled-fonts`, the bundled faces registered and the generic families bound to Roboto) and
     offers `register_font` and no removal. `TextContext`, built from it, owns Parley's
     `FontContext` and `LayoutContext` and shapes a paragraph through `&mut`. Both types exist in
     every build, so a realm's constructor has one signature with or without `parley`; shaping
     and registration sit behind the `parley` feature, off by default.
   - (2b) The runtime's shared engine services hold the collection, and each realm owns a
     `TextContext` built from the collection passed to `UiRealm::new`.
   - The cosmic-text path is unchanged, and `FONT_SYSTEM` and `shared_font_system()` stay until
     step 6: this step adds the Parley path beside it rather than routing cosmic-text through a
     new handle, so the handle's shape is Parley's from the start.
   - clippy `disallowed_types` rejects `Mutex` and `RwLock` in `flui-painting`; the cosmic-text
     path's `FONT_SYSTEM` is the one `#[expect]`ed site.
   - *Acceptance (2a):* two contexts over one collection shape on two threads whose intervals
     overlap, with equal metrics; a face registered after both contexts exist shapes in each;
     the Parley path never builds `FONT_SYSTEM`; `cargo xtask globals` is unchanged.
   - *Acceptance (2b):* two realms built over one collection each hold one context over it
     (`FontCollection::ptr_eq`, and a holder count of one per realm), a realm releases its
     context when it drops, a second presentation adds none, and the realms a runner builds get
     the runtime's collection (every runner site builds its realm through flui-app's one
     `build_runtime_realm`). That a face registered on the collection shapes in every context
     built from it is 2a's `a_face_registered_after_the_fork_shapes_in_every_realm`; with the
     realms' contexts proven to be built from that same collection, it is not repeated at the
     runtime level, which would need `parley` on the runtime's test build.
3. **Layout reaches the realm's text context; registration re-lays out text.** Two halves that
   land separately. 3a has landed. A third part, 3c, feeds the host's faces into the collection
   (§7); it has landed.
   - (3a) The realm lends its `TextContext` to each presentation's layout, and the box layout,
     intrinsics, dry-layout and dry-baseline contexts expose it (`ctx.text()`, a scoped
     `&mut TextContext`). Every `TextPainter` measuring method takes `&mut TextContext`, and
     `RenderParagraph` and `RenderEditable` pass the lent one.
   - (3a) How the realm lends it: a shared handle, `TextContextHandle`
     (`Rc<RefCell<TextContext>>`), held by the realm and cloned into every presentation's
     `PipelineOwner` when the presentation is assembled (`RealmCapabilities::text`, a required
     field). Threading `&mut TextContext` down would change `run_frame`, `run_layout` and every
     binding and harness that drives them, while `pump` and `render_frame` take `&self`. The
     realm and the pipeline owners are already `!Send`; a typed render object sees only the
     scoped borrow, taken from `&mut` context, so it cannot hold two loans or lay out a child
     while it holds one. The raw `RenderObject` methods and the erased layout context carry the
     context as `TextSource`, an opaque token only flui-rendering can borrow, so a direct
     `RenderObject` implementation cannot hold a loan across a child query either.
   - (3a) Parley measures behind `parley-layout`, not `parley`: the workspace test scope turns
     `parley` on for CI's `test` and `fast-lane` jobs, and if `parley` switched measurement, CI
     would measure every text-size test with Parley while the build that ships measures with
     cosmic-text. `parley` compiles the Parley measurement and its tests pin a painter to it.
     Under `parley-layout` size, baselines and intrinsics come from Parley while glyphs and
     carets still come from cosmic-text, until steps 4 and 5 (flui-painting `ARCHITECTURE.md`,
     mapping decisions 14 and 15).
   - (3b) Registering raises a font-collection-changed event on every realm, which marks text
     render objects for layout (ADR-0065's named gap); flui-app's `register_font` moves from
     `FONT_SYSTEM` to the collection.
   - (3b) Every pipeline is built with a text context: `PipelineOwner::new` and
     `new_with_capacity` take a `TextContextHandle`, `PipelineOwner` has no `Default`, and a
     layout, intrinsic or dry-query context takes a `TextSource`, so no path builds a context
     of its own. A presentation's pipeline is built inside `PresentationState::new` from
     `RealmCapabilities::text`. A frame driver that moves the owner out of its slot for a
     typestate transition uses `PipelineOwner::take_idle`, whose placeholder shares the
     context. A pipeline with no realm behind it passes `TextContextHandle::standalone`, a
     context over a collection of its own holding the bundled faces.
   - (3b) The hot-reload plugin pipeline (`flui-hot-reload`'s `pipeline.rs`) is one such
     pipeline: `app_plugin!` mounts it with a standalone context, not the host realm's. The
     plugin is a `dlopen`ed image the host reaches only through `flui_app_build(width,
     height)`, which has no parameter that could carry the host's handle, and `abi_token` covers
     only the `Scene` and `LayerTree` layouts, so nothing would check that the two images agree
     on `TextContext`'s layout if one were passed. The plugin image is therefore a realm of its
     own for text, and faces the host app registers do
     not reach it; carrying font bytes across the FFI into the plugin's collection is a
     follow-up, or goes with ADR-0094's replacement of the `dlopen` path.
   - *Acceptance (3a):* two realms over two collections measure through their own contexts, and
     a frame on one lends nothing of the other's; every presentation's pipeline holds the
     realm's handle; a painter measures through the context it is given, and a registration on
     that collection invalidates its cache; Parley's metrics on the bundled Roboto round to
     today's baselines; a layout that panics while holding the context releases it.
   - *Acceptance (3b):* a two-realm test: a font registered through realm A re-lays out text in
     realm B (fails on main). Test bootstraps construct the collection.
   - *Acceptance (3b, pipelines):* a pipeline constructor without a context does not compile; an
     owner taken out of its slot leaves one that measures through the same context; a plugin
     pipeline measures through the context it is mounted with and lays its root out at each
     frame's surface size. The default build still measures
     with cosmic-text through `FONT_SYSTEM`, and `parley-layout` still shapes for paint there,
     until steps 4 and 5.
   - (3c) Host faces in the collection, ahead of step 4, so that text the bundled faces do not
     cover (CJK, emoji, a family chain such as Cupertino's `-apple-system`, `system-ui`,
     `Segoe UI`) measures in the face it paints with once Parley measures by default. flui-app's
     shared engine services build the collection with `FontCollection::with_host_faces` over the
     process font system (§7): its faces, generics and fallback order; the family rule is shared
     with `shape.rs`. Standalone contexts, test bootstraps and the hot-reload plugin keep
     `FontCollection::new()`, the bundled faces alone, so they stay deterministic. This is the
     measurement/paint face agreement step 4's gate asks for.
   - *Acceptance (3c):* on the host's faces, Parley's measured width and height equal the painted
     cosmic-text layout's within 0.05 px for Latin at 400 and 700, monospace, Cupertino's chain
     at 400 and 600, CJK, emoji and mixed text, at 16 and 32 px, and fail on a bundled-only
     collection; every family the process font system carries is in the collection; each
     script's fallback families and the emoji generic follow the paint side's lists in order,
     with the sans-serif family last; a family spelled in another case than the fonts name it
     resolves alike on both sides; on a bundled-only collection a glyph the named family lacks
     measures in Roboto;
     the runtime feeds once per app, not per realm; a font file that cannot be read is skipped
     and the feed completes. `cargo xtask globals` is unchanged.
4. **Neutral shaped runs on the display list.**
   - `DrawOp::Paragraph` carries flui-painting's `ShapedParagraph`: runs naming a FLUI-owned
     font blob id, face index, size, interned variation and synthesis, with glyph id, position,
     subpixel bin and span colour. It replaces `Arc<TextLayout>`
     (`display_list/command.rs:167-174`). Runs are produced from the same shaped layout that
     measured, so "painted as measured" still holds.
   - A per-frame table carries the blobs a frame names first, which is the door §5 leaves open.
     The engine's atlas becomes `GlyphAtlas<SwashRasterizer>`, the `parley` feature folds into
     the default build, and the atlas's default parameter goes.
   - `TextPainter` measures on Parley in the default build and the `parley-layout` feature is
     removed, so the runs painted come from the layout that measured. Folding `parley` alone
     would leave measurement on cosmic-text while paint moves to Parley runs.
   - *Acceptance:* `draw_command_fits_its_budget` holds; the text readback suite passes
     unmodified; `the_engine_does_not_shape` is extended so the engine's manifest names no
     parley, fontique, skrifa, swash or cosmic-text. Glyph baselines round as today
     (`(run.line_y * scale).round()`), pinned against today's output (gate 8, second half). A
     registry test: a source-cache prune while the registry holds the blob keeps keys equal, and
     fails without the registry.
5. **Paragraph and editable text on Parley.**
   - `TextLayout` shapes one Parley `Layout` per paragraph on the realm's `TextContext`.
     Carets, selection and hit-testing come from clusters.
   - `TextDirection` sets the paragraph's bidi base direction. Parley 0.11.1 offers no way to
     set it, so until then the Parley path only aligns lines by it (flui-painting
     `ARCHITECTURE.md`, mapping decision 12); this step needs a Parley release that does, or
     directional isolates around the text.
   - The host font scan runs off the owner thread (§7), once step 3b's event exists. The
     `system` feature question is settled: `fontique/system` reaches `windows`, which tier S
     forbids, and needs fontconfig headers on Linux, so it is not used. Host discovery is
     fontdb's, which the collection is fed from (step 3c); no reach grant is needed.
   - The cosmic-text path stays behind a flag for one release as the rollback.
   - *Acceptance:* gates 2–7. The existing selection and offset↔cursor tests pass unchanged for
     LTR, RTL and mixed bidi. The `complex-scripts` decision is recorded.
6. **cosmic-text removed; `FONT_SYSTEM` leaves.**
   - cosmic-text, `unicode-segmentation` and `unicode-script` leave the workspace. Host
     discovery stays fontdb's, which becomes a direct dependency, and FLUI owns the per-platform
     fallback tables `FallbackChain` reads from cosmic-text today.
   - `FONT_SYSTEM`, `shared_font_system()`, `SharedFontSystem`, `Shaper`, the cosmic `GlyphKey`
     and `pub use cosmic_text::fontdb::Family` go, and the rollback flag is removed.
   - *Acceptance:* `cargo tree -i cosmic-text` is empty; the globals allowlist is shorter by
     `text_layout::layout::FONT_SYSTEM`; the `disallowed_types` `#[expect]` in
     `text_layout/layout.rs` is gone; §§1–5 are accepted and the back-links in Consequences are
     written.

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

- `FONT_SYSTEM` and `flui_painting::shared_font_system()` are deleted at §10 step 6, with
  cosmic-text, and with them the lock on every measurement and every glyph rasterization. Until
  then the bundled faces sit in both the fontique collection and `FONT_SYSTEM`.
  Since §10 step 2b `SharedEngineServices` constructs the collection; once `FONT_SYSTEM` is gone
  it no longer forces the font scan.
- Every capability context that measures text gains an explicit font-context handle; test
  bootstraps construct one.
- The first frame already has the host's faces: the feed is synchronous until step 3b's event
  exists, and costs a second read of the host's font files before the first frame (about 35 ms
  over 76 families on the Windows development host). Once the feed moves off the owner thread,
  a text run styled with a system-only family may re-lay out when it completes; that visible
  swap is the price of taking the scan off the startup path.
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

The gate 1 prototype exists on `spike/parley_atlas` (not merged). The raster seam, the
same-key-twice test, and the per-realm text context (§10 step 2, both halves) with its tests
exist, and so does layout measuring through the realm's context (§10 step 3a); the rest do not
exist yet.

- The raster seam, `ParleyGlyphKey`, `FontRegistry` and `SwashRasterizer` exist behind
  flui-painting's `parley` feature, with the oracle (`crates/flui-painting/tests/parley_oracle.rs`).
- The gate 1 prototype and its oracle glyph tests, with the glifo/skrifa choice recorded.
- `FontCollection` and `TextContext` exist, with `crates/flui-painting/tests/text_context.rs`:
  `two_realms_shape_in_parallel` (two contexts over one collection shape on two threads whose
  intervals overlap, with equal metrics), `a_face_registered_after_the_fork_shapes_in_every_realm`
  (fails when the collection is not shared), and
  `the_parley_path_never_builds_the_process_font_system` (its own binary, so it holds under
  `cargo test` too, which closes the prototype's per-process caveat).
- FLUI's text path names no `Mutex` or `RwLock`: clippy `disallowed_types` in
  `crates/flui-painting/clippy.toml`, with `FONT_SYSTEM` the one `#[expect]`ed site until §10
  step 6. fontique's own locks are outside that check (§3).
- Realm tests (§10 step 2b), in `crates/flui-runtime/src/ui_realm/tests/text_context.rs`:
  `two_realms_hold_contexts_over_the_one_collection_they_were_given`,
  `dropping_a_realm_releases_its_text_context` and `a_second_presentation_adds_no_text_context`;
  in flui-app, `separate_realm_windows_shape_over_the_runtimes_font_collection` (through
  `build_runtime_realm`, the one call every runner site builds its realm with) and
  `ensure_services_resolves_both_and_caches_them` (one collection per runtime).
- Layout measures through the realm's context (§10 step 3a): in
  `crates/flui-runtime/src/ui_realm/tests/text_context.rs`,
  `two_realms_measure_text_through_their_own_contexts` and
  `every_presentation_pipeline_holds_the_realms_text_context`; in
  `crates/flui-painting/tests/text_painter_unit.rs` (under `parley`),
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
  `crates/flui-painting/tests/host_faces_oracle.rs` (`parley`),
  `measured_width_equals_painted_width_on_host_faces` and
  `every_family_the_process_font_system_carries_resolves_in_the_collection`; in
  `crates/flui-painting/src/text_layout/fallback_chain.rs`,
  `fontique_fallbacks_follow_the_paint_chain_in_order`,
  `the_sans_serif_family_ends_every_script_fallback` and
  `the_emoji_generic_is_the_common_list`; in `crates/flui-painting/src/parley_text/shape.rs`,
  `a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection`; the row
  `the_collection_resolves_the_family_the_font_system_does` of `family_resolution_contract`
  (`font_resolve.rs`); `a_missing_path_is_skipped_and_the_feed_completes` (`context.rs`); in
  flui-app, `the_runtime_feeds_host_faces_once_for_every_realm` (`runtime.rs`) and the feed count
  in `separate_realm_windows_shape_over_the_runtimes_font_collection`.
- A two-realm test: registering a font in one realm makes text in the other re-lay out (§10
  step 3b).
- A registry test: a source-cache prune while the registry holds the blob keeps keys equal, and
  the test fails without the registry.
- A same-key-twice test: rasterizing one key twice yields equal bitmaps
  (`one_key_rasterizes_to_equal_images_twice`).
- A baseline test against today's rounding, measurement side:
  `crates/flui-painting/tests/parley_metrics_oracle.rs` (`parley`) measures the bundled Roboto
  on both paths at 13, 14, 16, 18 and 32 px, default and 1.5 line height, and finds equal width
  and height and the same device baseline, `(baseline * scale).round()`, at scales 1, 1.25,
  1.5 and 2. It holds because Parley's layout is unquantized: quantized, Parley rounds ascent,
  descent and the leading halves to whole logical pixels, and seven of the forty cases landed
  on another device row (16 px at 1.5 line height: 17 against 17.47, so 34 against 35 at
  scale 2). The glyph side waits for step 4.
- `the_engine_does_not_shape` ([ADR-0067](ADR-0067-engine-owned-glyph-atlas.md)) extended so the
  engine's manifest names no Parley, fontique, skrifa or cosmic-text crate.
- The process-global state gate ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) with
  `FONT_SYSTEM` removed from its allowlist.
- A first-frame test that renders bundled text before the system scan completes.
