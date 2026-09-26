# ADR-0092: Text shapes per realm over Parley and crosses the display list as neutral shaped runs

- **Status:** Proposed — accepted when the rasterization gate (§8, gate 1) passes
- **Date:** 2026-09-25
- **Supersedes (on acceptance):** [ADR-0077](ADR-0077-migrate-to-parley.md) (absorbed: its
  direction, its preconditions and its "If later Rejected" branch are carried here)
- **Supersedes (on acceptance):** [ADR-0016](ADR-0016-unified-font-system-registration.md),
  [ADR-0059](ADR-0059-flui-stays-on-cosmic-text.md)
- **Amends (on acceptance):** [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  (Part 1: the process-wide font doors become a per-realm context; Part 2: the `Paragraph`
  payload), [ADR-0066](ADR-0066-display-list-command-representation.md) (`DrawOp::Paragraph`),
  [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (`GlyphKey`, the rasterization door, where the
  atlas lives)
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

### 3. Per-realm contexts with no lock

Each realm owns a Parley `FontContext` and `LayoutContext` (over a clone of the collection) as
owner-thread state, used through `&mut` and reached through the layout context explicitly — the
handle ADR-0065 deferred. Layout on one realm never waits on another realm's shaping, and there is
no mutex on the frame path.

### 4. A neutral shaped-run contract on the display list

`DrawOp::Paragraph` carries shaped runs defined by `flui-painting`, not a shaper type. A run
names its font by blob identity (blob id plus face index), its size, its normalized variation
coordinates and synthesis, and a list of glyphs (glyph id, position, subpixel bin), with span
colour and decoration as today. Nothing in the run names Parley, fontique, skrifa or cosmic-text,
so the engine and any second raster backend
([ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)) consume runs without a shaper
dependency. "Painted as measured" (ADR-0065) still holds by identity: the runs are produced from
the same `Layout` that measured the paragraph.

### 5. Glyph keys carry font identity; rasterization is a raster-side trait

`GlyphKey` becomes blob identity, face index, glyph id, size, variation hash, subpixel bin and
hinting mode. It is stable for the life of the collection, because faces are append-only. A
`GlyphRasterizer` trait turns a key plus a font reference into the `GlyphImage` ADR-0067's atlas
already accepts. The rasterizer is owned by the raster side and runs outside any shaping lock; the
atlas belongs to the `GpuContext` and is single-owned on the raster thread
([ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)). One shaper everywhere;
only the rasterizer and hinting may differ per platform.

### 6. ICU4X is the single Unicode source

Grapheme and word segmentation for editing, bidi and line breaking use the ICU4X data Parley
already brings. `unicode-segmentation` and `unicode-script` leave the workspace, and no
`unicode-bidi` is added.

### 7. System fonts load asynchronously

Bundled fonts are in the collection before the first frame. The host font scan runs off the owner
thread, and its faces arrive through the same font-collection-changed event as a registration.

### 8. Acceptance gates

These are ADR-0077's preconditions, carried over and extended. The ADR is accepted when gate 1
passes; gates 2–7 are conditions on the migration PRs.

1. **Rasterization prototype (blocking).** Parley-shaped glyphs rasterize into `GlyphImage`s the
   existing atlas accepts unmodified, for one Latin and one complex-script case, with a
   `GlyphKey` stable across repeated rasterization of the same glyph; no process-global font
   state; the rasterizer runs outside the shaping lock. The prototype evaluates `glifo` against
   hand-written skrifa-outline-to-atlas glue and records the choice.
2. **Cluster differences.** The `arabic_mixed` and `emoji_zwj` glyph-count differences from the
   spike are either pinned by a boundary test or enumerated and accepted in writing (ADR-0077
   precondition 2).
3. **Caret and selection.** The existing selection and offset↔cursor tests pass unchanged in
   behaviour for LTR, RTL and mixed bidi, deriving positions from clusters, since Parley's glyph
   runs do not carry byte indices.
4. **Conformance per area.** Bidi visual order, UAX #14 break points, one variable font on the
   weight axis, and `cargo build --target wasm32-unknown-unknown` for the crate that depends on
   Parley (ADR-0077 precondition 4).
5. **Family resolution.** ADR-0059's regression (a missing family never falls through to an emoji
   face, issue #927) is pinned by a test on the new stack.
6. **Initialization cost.** Init time and memory measured against FLUI's real initialization,
   not a default one (ADR-0077 precondition 5).
7. **Upstream report.** The `Item`-boundary issue is filed upstream (ADR-0077 precondition 6).

### 9. No `flui-text` crate yet

Shaping stays in `flui-painting`. A separate text crate is considered only after the migration,
and only if `cargo build --timings` shows shaping separates cleanly from recording.

## Alternatives considered

- **Stay on cosmic-text with per-realm `FontSystem`s.** Rejected as the target: each realm would
  scan and hold its own database, and the shaping and memory gap ADR-0077 measured stands. It is
  the fallback if gate 1 fails (below).
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
- **If gate 1 fails,** cosmic-text stays the shaper, ADR-0016 and ADR-0059 stay in force, and
  §§2–5 are re-scoped over cosmic-text: per-realm contexts and neutral shaped runs need only
  `&mut` access to a realm's own context, which cosmic-text also allows. The measured Parley
  advantage then stands as an unclaimed opportunity, as ADR-0077 recorded.
- **Back-links on acceptance.** The accepting change adds `Superseded-by: ADR-0092` to ADR-0077,
  ADR-0016 and ADR-0059, rewrites the Status lines of ADR-0016 and ADR-0059 (today "to be
  superseded by ADR-0077", `docs/adr/ADR-0016-unified-font-system-registration.md:3` and
  `docs/adr/ADR-0059-flui-stays-on-cosmic-text.md:3`) to name this record, and adds
  "Amended by ADR-0092" to ADR-0065, ADR-0066 and ADR-0067. Until then the older records are
  unchanged.

## Verification

None of these exist yet.

- The gate 1 prototype and its oracle glyph tests, with the glifo/skrifa choice recorded.
- A two-realm test: registering a font in one realm makes text in the other re-lay out, and
  neither realm's layout takes a lock (no `Mutex` in the text path, checked by clippy
  `disallowed_types` in `flui-painting`).
- `the_engine_does_not_shape` ([ADR-0067](ADR-0067-engine-owned-glyph-atlas.md)) extended so the
  engine's manifest names no Parley, fontique, skrifa or cosmic-text crate.
- The process-global state gate ([ADR-0097](ADR-0097-no-process-global-state-gate.md)) with
  `FONT_SYSTEM` removed from its allowlist.
- A first-frame test that renders bundled text before the system scan completes.
