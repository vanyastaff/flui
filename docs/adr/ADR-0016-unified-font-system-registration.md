# ADR-0016: One process-wide `FontSystem` owned by `flui-painting`, shared by layout and render — custom/icon fonts register once and BOTH paths see them

*Collapse the two disjoint `cosmic_text::FontSystem` instances (layout's private `flui-painting` singleton and the engine's per-`TextRenderer` instance) into a single `Arc<Mutex<FontSystem>>` owned by `flui-painting`'s `PaintingBinding`. A public `register_font` API loads bytes into that one instance, so a registered custom or icon font is visible to measure AND paint by construction — not by keeping two databases in sync. Bundle a permissively-licensed Material icon font behind a default-on feature and register it at app bootstrap so `Icon` renders a real glyph out of the box.*

---

- **Status:** Accepted (chief-architect ARCH-GATE: **ACCEPTABLE** — infra decision only. The `Icon` glyph-rendering slice, the engine `TextRenderer` migration, and the bundled-font wiring are separate DEV tasks, each DoD-cross-checked against `.flutter/`.)
- **Date:** 2026-07-02
- **Deciders:** chief-architect; consult api-design-lead (the new `PaintingBinding::register_font` / `font_system()` public surface + the `flui_painting::FontSystem` re-export that pins the shared type), async-systems/scheduler owner (confirming the shared-lock stays off the async path and layout/paint phases are disjoint), systems-perf-lead (lock-contention and multi-window font-DB sharing), qa-lead (the type-identity failsafe test + the register→measure→paint consistency tests)
- **Relates to:** **Unblocks the deferred glyph-rendering half of the `Icon` widget** (`docs/research/2026-07-02-icon-widget-plan.md` §"THE GAP", B1.1 — the widget-layer slice shipped honestly with glyph rendering NO-GO pending this ADR). Sibling in spirit to ADR-0011/0012/0013: close a gap by making the correct state *structural* (one source of truth) rather than bolting on a parallel channel that must be kept consistent.
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs (painting API, engine migration, bundled-font wiring, Icon glyph assertion).

---

## Context

### The gap (verified)

FLUI cannot render a glyph from a non-system font (icon fonts, bundled app fonts). Two independent problems compound:

1. **No public font-registration API, and the layout `FontSystem` is unreachable.** `crates/flui-painting/src/text_layout/layout.rs:48` holds a private `static FONT_SYSTEM: OnceLock<Mutex<FontSystem>>` initialised with a bare `FontSystem::new()`; the accessor `font_system()` at `:51` is `pub(super)`. Nothing outside that module can inject font bytes. Every widget shapes and *measures* against this instance during layout (`from_spans` at `:169` locks it at `:220`).

2. **A second, disjoint `FontSystem` in the engine.** `crates/flui-engine/src/wgpu/text.rs:360` gives each `TextRenderer` its own `font_system: FontSystem`, initialised by `initialize_font_system` (`:400`) — system fonts if present, else the embedded `Roboto-Regular.ttf` (`load_embedded_fonts`, `:425`). This is what glyphon rasterises glyphs from on the GPU.

3. **`FontLoader` exists but is dead.** `crates/flui-engine/src/wgpu/font_loader.rs` can `load_bytes`/`load_file`/`load_directory` into a `&mut FontSystem` handed to it — but it has **zero callers** and no way to reach the private layout singleton.

Net: a `TextSpan { font_family: Some("MaterialIcons"), text: "\u{e87d}" }` resolves to tofu. The font is in neither `FontSystem`, and the layout one cannot be given fonts at all. This blocks the `Icon` widget's glyph rendering *and* any bundled/custom application font (Flutter's `pubspec` asset fonts, `dart:ui.loadFontFromList`).

### The consistency trap this ADR must not build

The two `FontSystem`s must agree on font data or text **measures against one font and paints with another** — the width computed during layout won't match the glyphs rasterised, producing overflow, clipping, or mis-aligned carets. Today they *coincidentally* agree because both load system fonts plus Roboto. The moment a font is registered into only one of them, they diverge silently — exactly the "MVP reported as parity" failure mode `AGENTS.md` §Definition-of-Done warns against. Any design that keeps two databases and *synchronises* them (see Alternatives, Option A) makes divergence a live, order-dependent bug (a font registered after engine init is invisible to paint). "One fact, one place" (studio standard) says: there should be one `FontSystem`.

### Two facts that make unification the right call, not just the tidy one

- **The two `FontSystem`s are the SAME Rust type.** `cosmic-text` resolves to a single `0.18.2` in `Cargo.lock` — `flui-painting` depends on `cosmic-text = "0.18"` directly; `flui-engine` depends on `glyphon = "0.11"`, which re-exports **that same** `cosmic-text 0.18.2` as `glyphon::FontSystem`. There is exactly one copy of `cosmic-text` in the tree, so `glyphon::FontSystem` *is* `flui_painting`'s `cosmic_text::FontSystem`. An `Arc<Mutex<FontSystem>>` created in `flui-painting` can be handed to glyphon's `&mut FontSystem`-taking methods with no conversion. (Confirmed: `grep 'name = "cosmic-text"' Cargo.lock` → single `0.18.2` entry.)

- **The layering already points the right way.** `flui-engine` depends on `flui-painting` (`crates/flui-engine/Cargo.toml`: `flui-painting = { path = "../flui-painting" }`); `flui-painting` does **not** depend on `flui-engine`. So `flui-painting` is the lowest crate that owns a `FontSystem`, and the engine can freely reach *down* to a shared instance owned there. `flui-painting` already ships `PaintingBinding` (`crates/flui-painting/src/binding.rs:354`) — the Flutter-parity singleton, already initialised at startup via `RenderingFlutterBinding` (`crates/flui-app/src/bindings/renderer_binding.rs:320`) and already the owner of a `SystemFontsNotifier` for font-change events. It is the natural, existing home for the shared `FontSystem`.

### Flutter parity confirms it

Flutter has **one** Skia `FontCollection`, shared by paragraph layout and paint. `dart:ui.loadFontFromList(Uint8List, {fontFamily})` and the framework's `FontLoader.load()` (`.flutter/flutter-master/packages/flutter/lib/src/services/font_loader.dart:16` — `addFont` at `:36` → `loadFontFromList` at `:76`) add bytes to that single collection; asset fonts declared in `pubspec` (including the framework's bundled `MaterialIcons-Regular.otf`) flow through the same path. FLUI's dual-`FontSystem` split is the *divergence*; unifying it restores the Flutter model (Prime Directive rule #1 — loyal to behaviour).

---

## Decision

**We will collapse the two `FontSystem`s into one process-wide `Arc<Mutex<FontSystem>>` owned by `flui-painting::PaintingBinding`, expose a public `register_font` API on it, and register a feature-gated bundled Material icon font at app bootstrap.**

Concretely:

1. **`flui-painting` owns the single `FontSystem`.**
   - Move the shared instance onto `PaintingBinding` (or a `PaintingBinding`-owned `OnceLock<Arc<Mutex<FontSystem>>>`), initialised once with system fonts and the embedded Roboto fallback (the logic currently in the engine's `initialize_font_system` / `load_embedded_fonts` moves down here — the *lowest* owner loads the baseline). Keep `parking_lot::Mutex` (non-poisoning, matching the existing choice and its documented rationale at `layout.rs:29`).
   - The layout `FONT_SYSTEM` in `text_layout/layout.rs` becomes a thin accessor onto this shared instance (or is deleted in favour of `PaintingBinding::instance().font_system()`); layout keeps locking per-shape exactly as today.
   - Re-export the type as `flui_painting::FontSystem` (`pub use cosmic_text::FontSystem;`) so **every** consumer, including the engine, names the shared type through `flui-painting` — this is the semver-relevant pin that keeps the shared type single-sourced.

2. **Public registration API on `PaintingBinding` (`flui-painting`).**
   - `pub fn register_font(&self, bytes: &[u8]) -> PaintingResult<()>` — locks the shared `FontSystem`, calls `db_mut().load_font_data(bytes.to_vec())`, fires the existing `SystemFontsNotifier` so laid-out text re-shapes against the new face. (Optionally surface the parsed family name(s); keep the minimal signature first.)
   - `pub fn font_system(&self) -> Arc<Mutex<FontSystem>>` — hands out a clone of the `Arc` for the engine to hold.
   - api-design-lead signs off this surface (new `pub` on `PaintingBinding`, the `FontSystem` re-export, `#[non_exhaustive]` posture on any new error variant).

3. **`flui-engine` borrows the shared instance instead of owning one.**
   - `TextRenderer.font_system: FontSystem` → `font_system: Arc<Mutex<FontSystem>>`, cloned from `PaintingBinding::instance().font_system()` in `TextRenderer::new`. Delete the per-instance `initialize_font_system` / `load_embedded_fonts` (baseline now loaded by `flui-painting`).
   - glyphon call sites lock the shared mutex only where glyphon needs `&mut FontSystem` (the `prepare` phase); `render` does not need it. Layout (shape) and engine (prepare) run in **disjoint pipeline phases** within a frame, so contention is negligible.
   - `FontLoader` (currently dead) is retained as the engine-side *fs* convenience (`load_file`/`load_directory`, which need filesystem access the layout crate should not have) — rewired to operate on the shared instance via `PaintingBinding`. The `register_font(bytes)` primitive lives in `flui-painting`; `FontLoader` is sugar on top.

4. **Bundle a Material icon font, feature-gated, registered at bootstrap.**
   - Bundle a permissively-licensed classic Material icon font (`MaterialIcons-Regular.ttf`, **Apache-2.0** — note: the Google Material icon fonts are Apache-2.0, not OFL; compatible with the workspace license posture; add it to the licence inventory / `deny.toml` allowances). Ship the bytes behind a **default-on** cargo feature (`material-icons`) co-located with the `Icon` widget in `flui-widgets`, so size-sensitive targets (wasm, embedded) can opt out. Full variable Material Symbols (~3–4 MB) is rejected for binary size; the classic static face (~1–2 MB) is the default.
   - Register it at **app bootstrap** (`flui-app`, the top crate that depends on both `flui-widgets` and `flui-painting`): `flui_painting::PaintingBinding::instance().register_font(flui_widgets::MATERIAL_ICONS_TTF)`, guarded by the feature. Flutter's parity home is `PaintingBinding`, but the *bytes* originate in a higher crate (`flui-widgets`), so the register *call* is made from `flui-app`'s binding init — the baseline system/Roboto load stays in `flui-painting`.

5. **Pin the shared type against version skew.** Hoist `cosmic-text` into `[workspace.dependencies]` pinned to the version glyphon re-exports (`0.18`), and add the type-identity test below. Bumping `glyphon` now requires re-verifying its `cosmic-text` still matches `flui-painting`'s — the test makes that a compile failure, not a silent runtime split.

---

## Consequences

**Positive**

- **The measure/paint mismatch class is eliminated by construction.** One `FontSystem` → a font registered anywhere is visible to both layout and every engine instance, in any order. No resync, no generation counter, no "font in one but not the other."
- **Restores Flutter's single-font-collection model** (Prime Directive #1).
- **Multi-window is strictly better**: N engine instances now share ONE font DB (fonts loaded once, not per surface).
- **`FontLoader` stops being dead code** — it becomes the fs-side sugar over the real primitive.
- **`Icon` glyph rendering unblocks** with an honest, testable assertion (measured width > 0 for a bundled codepoint), and any app can register its own fonts.

**Negative / Trade-offs**

- **Engine `TextRenderer` migration touches internals** (ownership `FontSystem` → `Arc<Mutex<FontSystem>>`, lock at glyphon `prepare`). Isolated to `flui-engine`; no conflict with the concurrent `Icon` builder in `flui-widgets` (different crate/files).
- **A process-wide lock now spans layout shape AND engine prepare.** Mitigated because the phases are disjoint within a frame; if profiling ever shows contention (systems-perf-lead), the fallback is a read-mostly `RwLock` or a per-frame font-DB snapshot — but do NOT pre-optimise.
- **Version-skew coupling**: `flui-painting`'s `cosmic-text` and `glyphon`'s must stay the same version, or the shared type stops compiling. This is a *compile-time* failsafe (good), pinned by the workspace dep + the type-identity test, but it constrains `glyphon` upgrades.
- **Binary size**: the bundled icon font adds ~1–2 MB by default. Mitigated by the opt-out feature; wasm/embedded turn it off.
- **Licence surface**: adds an Apache-2.0 font asset — record in `deny.toml` / licence inventory.

**Follow-ups**

- DEV: `flui-painting` shared-`FontSystem` + `register_font` API (api-design-lead review).
- DEV: `flui-engine` `TextRenderer` migration to the shared `Arc` + rewire `FontLoader`.
- DEV: bundle `MaterialIcons-Regular.ttf` behind `material-icons`; bootstrap registration in `flui-app`; `deny.toml` licence entry.
- DEV: flip the `Icon` plan's deferred glyph assertion from "DO NOT assert rendered glyph" to a live measured-width test.
- Separate, out of scope here: generating `Icons.*` codepoint constants (a widget-layer concern, not font infra).

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **(A) Two `FontSystem`s + a registry both consult / dual-load.** A `register_font` feeds the layout singleton; the engine loads the same registry at init. | Keeps the divergence and only *manages* it. A font registered after engine init is invisible to paint unless a generation-counter resync is added every frame — new infra to maintain a consistency the unified design gets for free. Violates "one fact, one place." This is the exact trap §Context names. |
| **(C) Registry in `flui-assets`, both crates consult it.** | `flui-painting` does **not** currently depend on `flui-assets`; `flui-assets` pulls `tokio::fs` (async). Adding a `flui-painting → flui-assets` edge injects tokio into the low, synchronous layout crate — a layering regression. `flui-assets`' `FontData`/`FontAsset` remain useful as an *asset-loading* front-end that ultimately calls `register_font`, but they are not the source of truth. |
| **Unify, but host the shared `FontSystem` in the engine.** | Wrong direction: `flui-painting` (layout) is lower than `flui-engine` and cannot depend up. Layout must reach the fonts, so the owner must be at painting's level or below. |
| **Bundle the full Material Symbols variable font by default.** | ~3–4 MB in every binary. The classic static face behind an opt-out feature covers `Icon` parity at a fraction of the size. |
| **Make icon-font provision entirely the app author's job (bundle nothing).** | Diverges from Flutter, where `Icon(Icons.x)` works out of the box because the framework bundles `MaterialIcons`. The opt-out feature preserves that ergonomics without forcing the cost on size-sensitive builds. |
| **Separate `flui-material-icons` crate for the bundled bytes.** | Not justified yet (studio crate-split rule: split per boundary, not per asset). A feature flag on `flui-widgets` is lighter. Revisit only if generated `Icons.*` constants + multiple icon sets grow into a real boundary. |

---

## Test plan

1. **Type-identity failsafe (compile-time, `flui-engine`).** A test that constructs a `FontSystem` via `flui_painting` and passes it to a glyphon `&mut FontSystem`-taking call. It compiles **iff** `glyphon`'s and `flui-painting`'s `cosmic-text` are the same version — the guard against silent version skew.
2. **Register → measure (layout path, `flui-painting`).** Register a known font (a test `.ttf` with a private-use glyph), lay out that codepoint via `TextLayout`, assert `metrics().width > 0` — a non-tofu, non-zero advance. Would fail today (no injection API).
3. **Register → paint (render path, `flui-engine`).** After registration, the `TextRenderer` holding the shared `Arc` shapes the same codepoint to a glyph with non-zero advance in its buffer — proving the render path sees it too.
4. **Single-source-of-truth invariant.** Register a font **after** a `TextRenderer` is constructed; assert both the layout path AND the engine's buffer see it. This test *fails under Option A* without a resync and *passes trivially* under this decision — it encodes the property the ADR buys.
5. **Icon E2E (`flui-widgets`, unblocked).** With the bundled font registered, `Icon` for a bundled codepoint (e.g. `U+E87D`) produces a `RenderParagraph` whose measured width > 0 — the assertion the Icon plan deferred as "dishonest with no icon font."
6. **Feature-off build.** `--no-default-features` (no `material-icons`) still compiles and runs; `Icon` degrades to tofu/empty without panicking (documented behaviour).

---

## Blast radius

- **`flui-painting`**: shared `FontSystem` onto `PaintingBinding`; `register_font` + `font_system()` public API; baseline system/Roboto load moves here; `text_layout/layout.rs` `FONT_SYSTEM` becomes an accessor. *(Public surface change → api-design-lead + semver note; this is a 0.2.x additive change.)*
- **`flui-engine`**: `TextRenderer.font_system` → `Arc<Mutex<FontSystem>>`; glyphon call sites lock; delete per-instance font init; rewire `FontLoader` onto the shared instance.
- **`flui-app`**: bootstrap registers the bundled icon font (feature-gated) via `PaintingBinding`.
- **`flui-widgets`**: bundled `MaterialIcons-Regular.ttf` bytes behind `material-icons`; the `Icon` glyph assertion flips from deferred to live. *(Different files from the concurrent Icon-widget build — no conflict.)*
- **Workspace**: hoist `cosmic-text` to `[workspace.dependencies]`; add the icon font to `deny.toml` / licence inventory.

---

## Implementation progress

- **Slice 1 — shared `FontSystem` + registration API (`flui-painting`)** ✅ `ebc134cc`.
  Landed additively. Refinement vs the original blast-radius sketch: the single
  `Arc<Mutex<FontSystem>>` stays owned by the `text_layout` module (its existing home)
  rather than moving to a `PaintingBinding` field, and access is mediated by a new
  `SharedFontSystem` newtype exposing a scoped `with_mut(|&mut FontSystem| …)` callback
  instead of a raw `Arc<Mutex<…>>` getter — keeping the lock type off the public surface
  (port-check SP-6 clean **by design**, no suppression marker). Public API:
  `PaintingBinding::{font_system() -> SharedFontSystem, register_font(&[u8]) -> Result<()>}`,
  `PaintingError::RegisterFontFailed`, re-exports `flui_painting::{FontSystem, SharedFontSystem}`.
  Test-plan items 2 and 4 (register→measure visibility, single-source-of-truth) are
  covered at the painting layer. The baseline system/Roboto load did **not** move here
  yet (still lazy `FontSystem::new()`); folded into the engine slice.
- **Slice 2 — engine glyph pipeline onto the shared handle (`flui-engine`)** ⏳ next.
- **Slice 3 — bundled Material icon font + app-bootstrap registration** ⏳.
- **Slice 4 — flip the `Icon` glyph assertion live (`flui-widgets`)** ⏳.

---

## References

- Icon plan (the deferral this unblocks): `docs/research/2026-07-02-icon-widget-plan.md` §"THE GAP".
- Layout singleton: `crates/flui-painting/src/text_layout/layout.rs:48` (`FONT_SYSTEM`), `:51` (`pub(super) font_system()`), `:220` (per-shape lock).
- Engine second FontSystem: `crates/flui-engine/src/wgpu/text.rs:360`, `:400` (`initialize_font_system`), `:425` (`load_embedded_fonts`).
- Dead loader: `crates/flui-engine/src/wgpu/font_loader.rs` (zero callers).
- Shared-owner home: `crates/flui-painting/src/binding.rs:354` (`PaintingBinding`), `:359` (`SystemFontsNotifier`); init at `crates/flui-app/src/bindings/renderer_binding.rs:320`.
- Version proof: single `cosmic-text 0.18.2` in `Cargo.lock`; `glyphon 0.11` re-exports it.
- Flutter parity: `.flutter/flutter-master/packages/flutter/lib/src/services/font_loader.dart:16` (`FontLoader`), `:36` (`addFont`), `:76` (`loadFontFromList`); `dart:ui.loadFontFromList`; single Skia `FontCollection` shared by layout + paint.
- Related ADRs: ADR-0013 (close a gap by reusing existing machinery, not a parallel channel); ADR-0002 (engine-wide threading — lock/ownership posture).
</content>
</invoke>
