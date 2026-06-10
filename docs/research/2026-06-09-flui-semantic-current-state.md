# flui Semantic / A11y — Current State (2026-06-09)

Caveman file:line map of what flui already has. Synthesized by `rust-studio:rust-scout` agent. Companion to [[2026-06-09-flui-semantic-a11y-audit]] (synthesis) and [[2026-06-09-flutter-3-41-semantic-reference]] (Flutter reference corpus).

## Cargo dep: zombie `accesskit`

`C:\Users\vanya\RustroverProjects\flui\crates\flui-semantics\Cargo.toml:39` — `accesskit = "0.21"`. **Zero source-file refs** in flui-semantics (grep returns nothing). Dead dep. Either use it or remove.

## `flui-semantics` crate (6,625 LOC, 110 unit tests, 12 files)

```
crates/flui-semantics/src/
  lib.rs              (1-180)  module docs + 5-tree architecture diagram + re-exports
  action.rs           (21-260) SemanticsAction enum (28 variants) + 4 tests
  binding.rs          (1-608)  SemanticsBinding, SemanticsHandle, AccessibilityFeatures, SemanticsActionEvent, SemanticsService
                              + 10 tests
  configuration.rs    (1-1058) SemanticsConfiguration (builder) + 9 tests
  event.rs            (22-257) SemanticsEvent, SemanticsEventType + 6 tests
  flags.rs            (16-217) SemanticsFlag (u64, 28 variants) + SemanticsFlags + 4 tests
  node.rs             (52-470) SemanticsNode (parent/children/element_id/config/rect/transform/dirty) + 9 tests
  owner.rs            (117-525) SemanticsOwner (tree + callback + flush) + 13 tests
  properties.rs       (1-529)  CustomSemanticsAction, SemanticsProperties, AttributedString,
                              SemanticsHintOverrides, SemanticsSortKey, SemanticsTag,
                              StringAttribute, StringAttributeType, TextDirection + 6 tests
  role.rs             (1-394)  SemanticsRole (30 variants), Assertiveness,
                              AccessibilityFocusBlockType, DebugSemanticsDumpOrder + 13 tests
  tree.rs             (57-565) SemanticsTree (Slab<SemanticsNode>) + 27 tests
  update.rs           (1-201)  SemanticsNodeData, SemanticsTreeUpdate, SemanticsTreeUpdateBuilder + 4 tests
```

## Foundation integration

- `C:\Users\vanya\RustroverProjects\flui\crates\flui-foundation\src\id.rs:647-651` — `pub type SemanticsId = Semantics;`
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-foundation\src\lib.rs:200,263` — re-exports
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-foundation\README.md:61,64,99,261` — SemanticsId documented in 5-tree family

## Rendering pipeline integration

- `crates/flui-rendering/src/lib.rs:84` — `pub use flui_semantics as semantics`
- `crates/flui-rendering/src/lib.rs:153-155` — re-exports
- `crates/flui-rendering/src/pipeline/phase.rs:148,154,168-171` — `pub struct Semantics` (PipelinePhase, sealed)
- `crates/flui-rendering/src/pipeline/owner.rs:155-158` — `semantics_enabled: AtomicBool`
- `crates/flui-rendering/src/pipeline/owner.rs:2050` — `pub fn into_semantics(self) -> PipelineOwner<Semantics>`
- `crates/flui-rendering/src/pipeline/owner.rs:2413-2501` — **`run_semantics` STUB** (tracing::warn per node + clear dirty)
- `crates/flui-rendering/src/pipeline/owner.rs:332-335` — `run_frame` wires `run_semantics`
- `crates/flui-rendering/src/pipeline/owner.rs:154-156` — `debug_doing_semantics` flag
- `crates/flui-rendering/src/pipeline/notifier.rs:35-111` — `on_semantics_owner_created`/`disposed` callbacks
- `crates/flui-rendering/src/pipeline/dirty.rs:70` — `pub needs_semantics: Vec<DirtyNode>`
- `crates/flui-rendering/src/storage/flags.rs:13,112,129,185-192,797,811` — `NEEDS_SEMANTICS` bit / `markNeedsSemanticsUpdate`
- `crates/flui-rendering/src/traits/render_object.rs:80-92` — `pub trait SemanticsCapability { fn describe_semantics_configuration(...) }`
- `crates/flui-rendering/src/traits/render_object.rs:149` — supertrait of `RenderObject<P>`
- `crates/flui-rendering/src/traits/render_box.rs:381` — `+ SemanticsCapability` supertrait bound
- `crates/flui-rendering/src/traits/render_sliver.rs:343` — `+ SemanticsCapability` supertrait bound
- `crates/flui-rendering/src/objects/*.rs` — **22 concrete `impl SemanticsCapability for Render* {}` empty/zero-arg opt-outs** (absorb_pointer:141, aspect_ratio:329, center:166, clip:569, colored_box:106, constrained_box:205, fitted_box:336, flex:422, fractional_translation, fractionally_sized_box, ignore_pointer, limited_box, meta_data, offstage, opacity, padding, repaint_boundary, sized_box, sliver_*, stack, transform)
- `crates/flui-rendering/src/delegates/custom_painter.rs:28-70` — `pub struct SemanticsBuilder` (inert shell, WARN_ONCE)
- `crates/flui-rendering/src/delegates/custom_painter.rs:142-145` — `fn semantics_builder(&self) -> Option<SemanticsBuilder>`
- `crates/flui-rendering/src/binding/mod.rs:78,357-397` — **Semantics Actions section; `perform_semantics_action` warn-stub**
- `crates/flui-rendering/src/binding/mod.rs:452-484` — `pub fn debug_dump_semantics_tree` (placeholder text "Semantics not generated")

## App integration

- `crates/flui-app/src/bindings/mod.rs:10,19,42` — re-export `SemanticsBinding`
- `crates/flui-app/src/bindings/renderer_binding.rs:51` — `use flui_semantics::{Assertiveness, SemanticsAction, SemanticsBinding}`
- `crates/flui-app/src/bindings/renderer_binding.rs:242-256` — `set_semantics_enabled` → `SemanticsBinding::set_platform_semantics_enabled`
- `crates/flui-app/src/bindings/renderer_binding.rs:259-263` — `announce()` → `SemanticsBinding::announce`
- `crates/flui-app/src/bindings/renderer_binding.rs:279-281` — `semantics()` accessor
- `crates/flui-app/src/debug/flags.rs:22,42,71,110,155,208` — `DebugFlags.show_semantics: AtomicBool`
- `crates/flui-view/src/binding.rs:14,365-366,1128-1135` — `did_change_accessibility_features` observer + `handle_accessibility_features_changed`

## Platform bridges — ZERO

```
crates/flui-platform/src/platforms/windows/    NO a11y refs (no UIA calls)
crates/flui-platform/src/platforms/macos/      NO a11y refs (no NSAccessibility calls)
crates/flui-platform/src/platforms/linux/      NO a11y refs (no AT-SPI calls)
crates/flui-platform/src/platforms/android/    NO a11y refs (no TalkBack bridge)
crates/flui-platform/src/platforms/ios/        NO a11y refs (no VoiceOver bridge)
crates/flui-platform/src/platforms/web/        NO a11y refs (no ARIA)
crates/flui-platform/src/traits/lifecycle.rs:4  only "lifecycle semantics" (i.e. naming, not accessibility)
```

No `windows` crate, no `atspi` crate, no `objc2-accessibility` crate, no `web_sys` for ARIA. Zero platform a11y dependencies anywhere in workspace.

## Tests

- 110 unit tests intra-crate (no `tests/` integration dir, no `parity/` test port of Flutter corpus)
- Notable coverage: `tree.rs:538` (27 tests, the most), `owner.rs:407` (13 incl. `flush_when_disabled`), `role.rs:394` (13), `binding.rs:492,608` (10), `configuration.rs:1058` (9), `node.rs:328` (9 incl. `test_semantics_node_to_data`, `test_semantics_node_absorb`)

## Internal docs / specs

- `crates/flui-semantics/src/lib.rs:1-53` — module-level doc + 5-tree architecture diagram
- `crates/flui-foundation/README.md:61,64,99,261` — SemanticsId
- `docs/FOUNDATIONS.md:36,151,170,192,200` — semantics = L3, L1 architecture
- `docs/architecture.md:24` — Layer 3 = `flui-painting, flui-layer, flui-semantics`
- `docs/crates.md:32,40` — "flui-semantics ACTIVE: Accessibility tree"
- `docs/ROADMAP.md:23` — semantics 7.9k Dart ~70% covered
- `docs/ROADMAP.md:140` — references layer/semantics repair plan
- `docs/ROADMAP-TRACKER.md:M3` — Layer/semantics repair plan in-progress
- `docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md` — 22 atomic commits / 7 waves
- `docs/brainstorms/layer-semantics-repair-requirements.md`
- `docs/research/2026-05-22-flui-layer-semantics-audit.md` — **25 findings (3 CRITICAL / 5 HIGH / 10 MEDIUM / 7 LOW)**
- `docs/research/2026-05-22-flui-rendering-engine-audit.md:54,72,179,240-244,281-340,346-388` — 3 unimplemented!()s in semantics + 4 production-path gaps
- `docs/research/2026-05-22-flutter-flui-gap-matrix.md:154-164` — ~65% coverage
- `docs/STRATEGY.md:20` — 4 trees incl. Semantics
- `AGENTS.md:33,157` — flui-semantics = "Core: accessibility (ACTIVE)" + ID offset pattern
- `crates/flui-painting/README.md:900-927` — Semantics Integration section (doc only, no impl)
- `crates/flui-rendering/flutter-rendering-hierarchy.md:56,89-93,218,572-573,623-624,720-895` — Flutter reference: `RenderSemantics*`, `SemanticsAnnotationsMixin`, `_RenderObjectSemantics`
- `openspec/changes/core-0a-foundation-adversarial-reaudit/exploration.md:116` — SemanticsBinding as 5th BindingBase
- `openspec/changes/core-0a-foundation-adversarial-reaudit/proposal.md:10,68,69,80,82,105,137,225` — Cross-cutting impact
- `openspec/changes/core-0a-foundation-adversarial-reaudit/specs/foundation-flutter-parity/spec.md:147` — Variant "Semantics" column

## Critical stubs inventory (production-path blockers)

| # | File:line | Code | Impact |
|---|---|---|---|
| 1 | `crates/flui-rendering/src/pipeline/owner.rs:2413-2501` | `run_semantics` = `tracing::warn!` per node + clear dirty | Tree never flows to AT |
| 2 | `crates/flui-rendering/src/binding/mod.rs:382-396` | `perform_semantics_action` = warn-stub, action no-op | AT input never reaches framework |
| 3 | `crates/flui-rendering/src/delegates/custom_painter.rs:50-58` | `SemanticsBuilder` = inert shell with WARN_ONCE | Custom paint path can't contribute semantics |

## `SemanticsConfiguration::absorb` drift vs Flutter `:6790`

| Flutter behavior | flui behavior | Reference |
|---|---|---|
| Concat `label`/`value`/`hint` via `_concatAttributedString` | first-wins | `semantics.dart:6790` |
| Filter `_actions` through `_kUnblockedUserActions` if `isBlockingUserActions` | not implemented | `semantics.dart:6790` |
| Absorb `role` field | `role` field absent from `SemanticsConfiguration` | `semantics.dart:5196` |
| Merge `headingLevel` via `_mergeHeadingLevels` | not implemented | `semantics.dart:6790` |

## Symmetric / architectural gaps

- `crates/flui-semantics/src/tree.rs:57` — `SemanticsTree` does NOT implement `TreeRead<SemanticsId>+TreeNav<SemanticsId>` (asymmetric vs `LayerTree` per [[flui-tree-unified-interface-intent]])
- `crates/flui-semantics/src/node.rs:437` — `merge()` should be `absorb()` for Flutter naming consistency
- `crates/flui-semantics/src/binding.rs:147-170` — `SemanticsBinding` has 4 `RwLock`s where `disable_animations` reads single bool; pack into `AtomicU8`
- `crates/flui-semantics/src/owner.rs:131` — `updates_buffer` per-frame alloc (now amortized via clear+reuse; verify)
- `crates/flui-semantics/src/tree.rs:391` — `iter_mut` zero callers outside `owner`
- `crates/flui-semantics/src/node.rs:79` — `transform: Option<Matrix4>` now unified (post-U19); pre-U19 was `Option<[f32; 16]>`

## SemanticsAction enum drift

`crates/flui-semantics/src/action.rs:21-100` adds `Expand`/`Collapse`/`Unfocus` not in current Flutter — **wire-format risk** when serializing to embedder ABI.

## `.flutter` symlink

`C:\Users\vanya\RustroverProjects\flui\.flutter` is a **Cygwin symlink** (mtime 2026-05-24) → `C:\Users\vanya\RustroverProjects\.flutter\flutter-master\` (the actual Flutter 3.41.9 mirror, 184 MB, no git, mtime 2026-05-19). See [[2026-06-09-flutter-3-41-semantic-reference]].
