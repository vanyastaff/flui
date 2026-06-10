# flui Semantic / A11y — Synthesis Audit (2026-06-09)

Caveman synthesis. Three layers: (1) [[2026-06-09-flui-semantic-current-state]] what flui ships today, (2) [[2026-06-09-flutter-3-41-semantic-reference]] the Flutter 3.41.9 reference corpus in `.flutter/flutter-master/`, (3) [[2026-06-09-rust-a11y-ecosystem]] the Rust a11y crates & competitor landscape.

**Bottom line**: flui-semantics crate is ~65% Flutter-port complete on the data side. **3 production-path warn-stubs** and **zero platform bridges** block end-to-end. The Rust ecosystem has converged on **AccessKit** as the IR standard (Slint/Xilem/egui/plushie-iced/AccessKit itself). Path forward = finish the 3 stubs + add AccessKit adapter layer + port `SemanticsConfiguration::absorb` to match Flutter's `:6790` reference. Web path stays manual ARIA (Flutter Web's PR #168653 is the production model — `SemanticRole` per-role + `SemanticBehavior` cross-cutting).

## Snapshot table

| Layer | Status | LOC | Coverage |
|---|---|---:|---|
| `flui-semantics` (types/binding/owner) | ACTIVE | 6 625 | ~65% (tree-data done, OS-API bridge missing) |
| `run_semantics` pipeline integration | STUB (warn) | — | 0% (3 tracing::warn! stubs) |
| Platform bridges (UIA/AT-SPI/NSA/TalkBack/VoiceOver/ARIA) | NONE | 0 | 0% |
| `SemanticsConfiguration::absorb` Flutter-parity | DRIFT | — | 4 drift-points vs `semantics.dart:6790` |
| AccessKit dep declared but unused | ZOMBIE | — | 0 source refs |
| Flutter 3.41.9 reference corpus (`.flutter/flutter-master/`) | AVAILABLE | ~82 213 | 100% (the contract source) |
| Unit tests in flui-semantics | 110 | — | intra-crate only, no parity/ dir |

## Top-10 actions (ranked by leverage)

| # | Action | Source | Rationale |
|---|---|---|---|
| 1 | Wire `run_semantics` → `SemanticsOwner::flush` | `flui-rendering/src/pipeline/owner.rs:2413-2501` | Removes critical stub. Most concentrated user-facing gap. |
| 2 | Wire `perform_semantics_action` → `SemanticsActionHandler` | `flui-rendering/src/binding/mod.rs:382-396` | Action dispatch is the second half of the a11y round-trip. |
| 3 | Revive `SemanticsBuilder` in custom_painter | `flui-rendering/src/delegates/custom_painter.rs:50-58` | Custom painters need a path to contribute semantics. |
| 4 | Port `SemanticsConfiguration::absorb` from Flutter `:6790` | `semantics.dart:6790` | 4 drift-points: concat label/value/hint, `isBlockingUserActions` filter, role absorption, heading-level merge. |
| 5 | Add `role` field to `SemanticsConfiguration` | `semantics.dart:5196` | Role enum exposed in flui but never stored at runtime config. |
| 6 | `SemanticsTree: TreeRead<SemanticsId>+TreeNav<SemanticsId>` | `crates/flui-semantics/src/tree.rs:57` | Asymmetric vs `LayerTree`; per [[flui-tree-unified-interface-intent]]. |
| 7 | Add `accesskit` 0.24 + `accesskit_consumer` 0.36 as deps | `flui-semantics/Cargo.toml:39` (currently 0.21 dead) | `accesskit_winit` 0.33 + platform adapters handle UIA/AT-SPI/NSA out of box. |
| 8 | `SemanticsNode` → `accesskit::Node` mapping | one-shot | 1:1 role/label/value/description mapping. See [[2026-06-09-rust-a11y-ecosystem]]. |
| 9 | `UpdateSemantics` + `DispatchSemanticsAction` callbacks | embedder ABI `embedder.h:298` | Reuse Flutter's per-platform contract. |
| 10 | Manual `web-sys` ARIA path (model: Flutter PR #168653) | `engine/src/flutter/lib/web_ui/lib/src/engine/semantics/` | No accesskit web-adapter shipped; mirror Flutter's per-role split. |

## Top-5 hidden gaps (Flutter has, flui doesn't)

| Gap | Flutter ref | Why it matters |
|---|---|---|
| `SemanticsInputType` enum (6 values: none/text/url/phone/search/email) | `engine/src/flutter/lib/ui/semantics.dart:561` | Text input semantics completeness; hint for keyboard type. |
| `SemanticsValidationResult` enum (None/Valid/Invalid) | `engine/src/flutter/lib/ui/semantics/semantics_node.h:116` | Form validation feedback to AT. |
| Heading-level merge in `absorb` | `semantics.dart:6790` via `_mergeHeadingLevels` | Document outline (a11y tree view) breaks without it. |
| `isBlockingUserActions` filter on absorb | `semantics.dart:6790` via `_kUnblockedUserActions` | Modal dialogs swallowing AT gestures. |
| Traversal-parent vs parent (OverlayPortal graft) | `semantics.dart:2773` + `SemanticsConfiguration.traversalChildIdentifier` | Future overlay-portal semantic graft. Not blocking now. |

## Competitor position (mid-2026)

| Tier | Members | Common ground |
|---|---|---|
| Production a11y on all 6 desktop+mobile platforms | Flutter, Slint, plushie-iced, Qt, GTK | All converge on platform-native bridges (UIA/AT-SPI/NSA/ATK) |
| Alpha/stub a11y | Xilem, egui (via accesskit path), Servo servoshell (web content) | AccessKit-backed, still landing per-platform polish |
| Webview-inherited ARIA | Dioxus, Tauri, Dioxus-mobile | Inherit Chromium/Safari a11y tree; native-layer gaps |
| No a11y | Makepad | Acknowledged gap, no bridge shipped |

**flui's bet**: same as Slint/egui (AccessKit) but with Flutter-semantics compatibility. **Differentiation**: the Flutter-port contract (`SemanticsNode`/`SemanticsConfiguration`/`SemanticsRole`/`SemanticsFlag`/`SemanticsAction` types are already 65% parity — that's the asset to defend).

## Risks

1. **MSRV lock-in**: accesskit 0.24+ MSRV 1.85. If flui MSRV < 1.85 → pin `accesskit 0.21.x` (last pre-edition-2024). Check before dep bump.
2. **`accesskit_ios` 0.1.0 immaturity** (May 2026 initial). For iOS path either: (a) wait for ≥0.5, (b) build raw UIAccessibility bindings (like `accessibility` 0.2.0 eiz, but stagnant).
3. **Rich text + hypertext**: AccessKit adapters don't support. Flutter `Link`/`AttributedString` need workaround (text range + custom action).
4. **WASM bin size**: `accesskit_winit` pulls winit X11/Wayland ~500KB+ → not viable on wasm. cfg: `accesskit` core only on wasm, ARIA via `web-sys` only.
5. **Flutter Web iOS WebKit regression #179784** (Dec 2025, still open) — manual ARIA path inherits the bug. Not flui-specific.
6. **Test harness cost**: parity with Flutter's `packages/flutter/test/semantics/` (8 files) + `widgets/semantics_*.dart` (~30 files) = ~1k+ unit tests once ported. Estimate: 2-3 PRs of work.

## Cross-references

- [[2026-06-09-flui-semantic-current-state]] — file:line map of existing flui semantics
- [[2026-06-09-flutter-3-41-semantic-reference]] — file:line map of Flutter 3.41.9 reference corpus
- [[2026-06-09-rust-a11y-ecosystem]] — Rust crates + competitor framework matrix
- Internal: `docs/research/2026-05-22-flui-layer-semantics-audit.md` (the prior 25-finding audit, now superseded for the platform-bridge section)
- Internal: `docs/research/2026-05-22-flui-rendering-engine-audit.md` (the 3-stub inventory)
- Internal: `docs/research/2026-05-22-flutter-flui-gap-matrix.md` (~65% coverage note)
- Internal: `docs/research/2026-05-22-rust-ui-ecosystem-lessons.md`
- Memory: `[[no-quick-wins-vanyastaff]]` (the no-defer-with-excuse rule applies to all 3 stub fixes)
- Memory: `[[flui-tree-unified-interface-intent]]` (TreeRead/TreeNav symmetry)
- Memory: `[[fable5-models-and-flui-env]]` (model context for any future PR work)
