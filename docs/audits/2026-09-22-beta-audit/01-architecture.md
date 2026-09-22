# FLUI Architecture & Crate Structure Audit

Scope: `main` branch as checked out, read-only. All numbers gathered via `wc -l`, `rg`, `find` on 2026-09-22.

## 1. Crate DAG and sizing

`docs/workspace-layers.toml` is the authoritative, tool-enforced layering (`just inventory-check`); `docs/architecture.md` renders it as 11 ranks (0-10):

```
10 flui (facade)
 9 flui-app, flui-devtools, flui-cli
 8 flui-localizations
 7 flui-material, flui-cupertino
 6 flui-widgets, flui-testing, flui-hot-reload, flui-build
 5 flui-view
 4 flui-engine, flui-rendering, flui-objects
 3 flui-layer, flui-semantics, flui-animation
 2 flui-tree, flui-platform, flui-scheduler, flui-painting, flui-interaction, flui-assets
 1 flui-foundation, flui-macros
 0 flui-geometry, flui-types
```

This is a real, checked contract (ADR-0041, `scripts/check-workspace-inventory.sh`), which is unusually rigorous for a project this size — most Rust UI frameworks (iced, egui) rely on convention, not a lint. The cost is that the layering document itself is a 262.9 KB TOML file (`docs/workspace-layers.toml`, 18.0K on disk per the earlier listing — actually `runtime-contract.toml` is 262.9K; `workspace-layers.toml` is 18.0K) that a newcomer must read to understand why an edge is rejected.

### LOC per crate (`.rs` files, `find crates/<c> -name '*.rs' | xargs wc -l`)

| Crate | LOC | Layer | Note |
|---|---:|---:|---|
| flui-widgets | 103,984 | 6 | **by far the largest crate** — widget catalog + navigator + text editing + scroll + focus |
| flui-rendering | 80,916 | 4 | render pipeline, virtualization, layer ownership |
| flui-engine | 74,194 | 4 | GPU/wgpu, AA oracle, raster owner |
| flui-view | 66,880 | 5 | View/Element tree, build owner |
| flui-objects | 54,521 | 4 | RenderBox/RenderSliver catalog |
| flui-platform | 48,854 | 2 | per-OS backends (macOS/Windows/winit/headless) |
| flui-app | 46,023 | 9 | app runtime, `ui_realm.rs` alone is 11,525 lines |
| flui-interaction | 41,664 | 2 | gestures, focus, arenas |
| flui-material | 37,828 | 7 | Material catalog |
| flui-types | 28,279 | 0 | base types + units |
| flui-scheduler | 26,609 | 2 | frame scheduling |
| flui-geometry | 19,297 | 0 | |
| flui-animation | 16,871 | 3 | |
| flui-foundation | 12,248 | 1 | |
| flui-semantics | 11,188 | 3 | |
| flui-painting | 10,610 | 2 | |
| flui-cli | 9,547 | 9 | |
| flui-testing | 7,241 | 6 | |
| flui-tree | 6,889 | 2 | |
| flui-cupertino | 6,520 | 7 | |
| flui-assets | 5,550 | 2 | |
| flui-layer | 5,040 | 3 | |
| flui-build | 4,779 | 6 | |
| flui-log | 4,642 | ungated | |
| flui-hot-reload | 3,158 | 6 | |
| flui-devtools | 2,882 | 9 | "no tree inspector yet" per its own manifest comment |
| flui-macros | 822 | 1 | proc-macros — thin, per its own manifest comment ("derives land as a follow-up") |
| flui-localizations | 281 | 8 | **near-empty**; occupies a whole layer (8) of its own for 281 lines |

Total workspace: ~28 crates, ~737K lines counted just in the >2000-line-file scan below (partial); the four largest crates (widgets, rendering, engine, view) alone account for ~326K lines, roughly 45% of the counted codebase.

**Sizing problems:**
- `flui-widgets` at 104K LOC is 4x the next crate and spans unrelated concerns (navigator/routing, text editing, scrolling, focus, image decode cache) that in Flutter are separate packages. It is a strong split candidate.
- `flui-localizations` (281 LOC) occupies its own DAG layer (8) between `flui-material`/`flui-cupertino` (7) and `flui-app`/`flui-cli` (9). A single-purpose 281-line crate justifying a whole layer is a sign the layer model has more ceremony than the crate has content — every consumer must reason about a layer that holds one nearly-empty package.
- `flui-macros` (822 LOC) is explicitly a "skeleton" per `Cargo.toml`'s own comment (`# Proc-macros (skeleton — derives land as a follow-up)`), i.e. shipped as a workspace member ahead of its content.

## 2. Public API surface (`flui` facade, `src/lib.rs`, 199 lines)

- Root `src/lib.rs` re-exports each crate as a same-named module (`pub use flui_widgets as widgets`, etc.) plus a curated `prelude` module and `run_app`. `#![deny(missing_docs)]` is enforced on the facade.
- Feature flags: `material` (default on), `cupertino`, `localizations`, `hot-reload`, `gpu-readback-tests`, `serde`. Turning `material` off removes the Material glob from `prelude` at the *use site* (unresolved import), by design — documented as intentional ("absent, not empty").
- `prelude` is two-tier: a design-system-neutral base (`flui_widgets::prelude::*` + `run_app`) and an opt-in Material half. Notably `TextField` is **excluded** from the Material half of the glob because both `flui-widgets::TextField` and `flui-material::TextField` exist and would collide — the doc comment in `src/lib.rs:176-182` calls this out explicitly. This is an honest but real ergonomic wart: two types with the identical name in the same conceptual "everyday widget" tier, disambiguated only by which one didn't make the glob.
- Lower layers (rendering, painting, engine, platform) are **not** re-exported through `flui`; an app author is expected to stay inside `flui::widgets`/`flui::app`. This mirrors Flutter's public/`dart:ui` split and is more disciplined than iced (which re-exports its renderer types more freely) — good for API stability, at the cost of examples like `examples/hello_world.rs` reaching directly into `flui_platform`/`flui_types` (see §6) instead of the facade, which undercuts the "go through `flui`" story for the very first thing a newcomer opens.
- Compared to egui (single crate, immediate mode, no facade needed) and Dioxus/Slint (each ships one main crate plus a macro crate, with the macro crate doing most of the ergonomic work), FLUI's facade-over-28-crates model is structurally closer to Flutter's own package layout than to any of its direct Rust competitors — appropriate for its ambitions but heavier for someone evaluating "should I try this."

## 3. Contributor understandability

| Artifact | Size |
|---|---:|
| `AGENTS.md` (root) | 133 lines |
| `docs/adr/` | 68 ADR files |
| `docs/research/` | 83 files |
| `docs/plans/` | 23 files |
| `docs/brainstorms/` | 13 files |
| `docs/designs/` | 10 files |
| `docs/audits/` | 4 files |
| `docs/ROADMAP.md` | 399 lines / 115.6 KB |
| `docs/ROADMAP-TRACKER.md` | 459 lines / 148.0 KB |
| `docs/PORT.md` | 1,233 lines / 131.8 KB |
| `docs/FOUNDATIONS.md` | 328 lines / 39.1 KB |
| `docs/runtime-contract.toml` | 262.9 KB |
| `docs/testing.md` | 32.9 KB |
| `docs/architecture.md` | 179 lines |
| `docs/crates.md` | 152 lines |

Only one `AGENTS.md` exists (root, no per-crate copies), which is good — no fragmented/contradictory sub-rules to reconcile. But the total non-ADR, non-research documentation load is heavy: 68 ADRs plus 83 research docs plus a 148KB tracker plus a 262KB runtime-contract TOML plus a 132KB port doc, all treated as load-bearing (workspace-layers.toml and runtime-contract.toml are checked by CI, not just descriptive).

**Top 5 sources of cognitive load for a newcomer:**
1. **68 ADRs with no reading order.** `docs/architecture.md` alone cites ADR-0027, ADR-0041, ADR-0002 by number with no index of "read these first." A newcomer must grep for which of 68 decisions is still current vs. superseded (ADR-0002 is explicitly superseded by ADR-0027, but nothing marks it dead at the file level besides prose in a different doc).
2. **`docs/runtime-contract.toml` at 262.9 KB and `docs/PORT.md` at 131.8 KB / 1,233 lines** are enforced or referenced artifacts, not optional reading — but their size alone (a quarter-megabyte TOML) is prohibitive to skim.
3. **`flui-widgets` at 104K LOC** means "read the widgets crate" is not a bounded task; the crate mixes navigator, text editing, scrolling, and focus, so there's no natural entry point smaller than the whole crate.
4. **Two competing "getting started" surfaces**: the facade's own doc-tested quick start in `src/lib.rs` (uses `flui::prelude::*`, `run_app`) vs. `examples/hello_world.rs`, which bypasses the facade entirely and talks to `flui_platform`/`current_platform()` directly with no widgets at all — a newcomer opening the example literally named `hello_world.rs` sees a different, lower-level API than the one documented in the crate root.
5. **The layering contract's own weight**: `docs/workspace-layers.toml` documents 6+ kinds of edges (`same_layer_edge`, `allowed_dependents`, `forbidden_edge`, `projected_edge`, `standalone_root`, `disposition` states like `keep`/`rename`/`narrow`/`optionalize`/`deferred-extraction`) — powerful but itself a small DSL to learn before a contributor can add a dependency with confidence.

## 4. Tech-debt signals (`rg`, workspace-wide, excluding `target/`)

| Signal | Count |
|---|---:|
| `#[allow(...)]` attributes | 24 |
| `.unwrap()` outside `tests/`/`*test*.rs` paths | 1,143 |
| `TODO`/`FIXME` in `.rs` files | 36 |
| Files > 2000 lines | 46 (see below) |
| `unsafe` occurrences by crate | see table |

Unsafe-site counts (`rg -c "unsafe "` per crate, includes test-only unsafe):

| Crate | unsafe occurrences |
|---|---:|
| flui-platform | 340 |
| flui-rendering | 49 |
| flui-hot-reload | 40 |
| flui-engine | 19 |
| flui-scheduler | 13 |
| flui-log | 11 |
| flui-app | 9 |
| flui-interaction | 9 |
| flui-foundation | 8 |
| flui-types | 5 |
| flui-view | 5 |
| flui-cli | 2 |
| flui-geometry | 1 |
| flui-material | 1 |
| flui-widgets | 1 |

`flui-platform` concentrates 340 of ~513 total unsafe occurrences — expected for OS-binding code (AppKit/Win32/winit FFI), but it means the single highest-unsafe-density crate is also the one every window/input feature touches, and it sits at layer 2 (i.e., almost everything above it depends on it transitively).

The 1,143-count `.unwrap()` figure is a blunt filter (it still counts unwraps inside `#[cfg(test)]` inline modules, since those aren't excluded by path), so treat it as an upper bound, but it is large enough that a grep for panic-prone paths in non-test code is a non-trivial exercise — consistent with the fact the project maintains a dedicated `docs/PANIC-POLICY.md` and a `docs/panic-policy-allowlist.txt` (6.3 KB) specifically to track allowed panics.

**Largest files** (>2000 lines; top 10 of 46):

| Lines | File |
|---:|---|
| 15,859 | `crates/flui-objects/tests/render_object_harness.rs` |
| 11,525 | `crates/flui-app/src/app/ui_realm.rs` |
| 6,364 | `crates/flui-app/src/app/runner/realm_dispatch.rs` |
| 5,842 | `crates/flui-view/src/owner/build_owner.rs` |
| 5,757 | `crates/flui-view/src/tree/element_tree.rs` |
| 5,642 | `crates/flui-scheduler/src/scheduler.rs` |
| 4,898 | `crates/flui-engine/src/renderer.rs` |
| 4,794 | `crates/flui-engine/src/aa_oracle_tests.rs` |
| 4,786 | `crates/flui-rendering/tests/retained_boundary_layers.rs` |
| 4,365 | `crates/flui-engine/src/raster_owner.rs` |

`ui_realm.rs` at 11,525 lines is the single largest non-test source file in the workspace and sits at the heart of the app runtime (`flui-app`) — the crate's own ADR-0027 threading model is implemented there.

## 5. Architectural risks for scaling / multi-window / parallelism

- **Global font system mutex**: `crates/flui-painting/src/text_layout/layout.rs` — `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`, accessed via `font_system_arc()`. Every text layout call in every window/thread contends on one global lock. This is the textbook single-point-of-contention that blocks parallel layout across multiple windows/realms.
- **Global asset registry and interner**: `crates/flui-assets/src/registry/mod.rs` (`static REGISTRY: LazyLock<AssetRegistry>`) and `crates/flui-assets/src/types/key.rs` (`static INTERNER: LazyLock<ThreadedRodeo>`) — process-wide singletons; `ThreadedRodeo` is at least designed for concurrent access, but the registry's concurrency story isn't visible from this scan.
- **Hot-reload globals**: `crates/flui-hot-reload/src/worker.rs` has three module-level statics (`WORKER_BUILDS: OnceLock<Mutex<HashMap<...>>>`, `REGISTRATION_SESSION: Mutex<Option<Vec<...>>>`, plus a test-only lock) — global mutable registration state for dlopen'd plugins, consistent with the memory notes on macOS dlclose/hot-reload gotchas.
- **Thread-affinity by design, not accident**: the project's own `ADR-0027-owner-affine-ui-realms.md` (cited in `docs/architecture.md`) makes single-writer, `!Send + !Sync` owner realms the *intended* model — each `UiRealm` is deliberately not thread-mobile, with bounded typed mailboxes and commit-at-Idle semantics. This is a considered design (superseding an earlier ADR-0002 "engine-wide threading architecture"), not an oversight, but it does mean multi-window scaling depends on realm-per-window fan-out rather than shared-nothing parallel layout within a realm — the ceiling is "N realms in parallel," not "layout of one tree in parallel."
- **`flui-platform` unsafe concentration (340 sites)** at layer 2 means platform-affinity bugs (main-thread-only AppKit calls, the documented "owner-lane" routing pattern from project memory) have the widest blast radius of any crate — consistent with the multiple owner-lane/window-sweep ADRs already tracked in project memory (issues #949, #1194, #1147).
- No evidence in this scan of a data-parallel layout pass (e.g., rayon-based subtree layout); `crates/flui-rendering/src/virtualization/sumtree.rs` (2,022 lines) suggests virtualization/windowing is the chosen scaling strategy for large lists rather than parallel tree walks.

## 6. Naming / ergonomics

- Widget construction is builder-style method chaining (`Container::new().color(...).padding(...).child(...)`), matching Flutter's constructor-with-named-args flavor as closely as Rust allows. `column![...]`/`row![...]` macros provide tuple-like children lists (seen in `examples/widgets_gallery.rs`), similar in spirit to Dioxus's `rsx!`/Slint's markup but staying inside plain Rust macros rather than a DSL.
- `#[derive(Clone, StatelessView)]` + `impl StatelessView for X { fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView { ... } }` is the idiomatic widget shape, shown in the facade's own doctest (`src/lib.rs:14-23`).
- **The bundled CLI counter template does not demonstrate state.** `crates/flui-cli/src/templates/counter.rs` (`flui new`'s generated `main.rs`) produces:
  ```rust
  impl StatelessView for CounterView {
      fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
          Center::new().child(Column::new(column![
              Text::new("You have pushed the button this many times:"),
              SizedBox::height(16.0),
              Text::new("0"),
          ]))
      }
  }
  ```
  This is a "counter" with a hardcoded `"0"` and no button, no `StatefulView`, no `setState` — the single template a new user runs (`flui new` → `flui run`) does not show the state pattern the crate is named for. `StatefulView` usage does exist elsewhere (`examples/animated_box_app.rs`, `examples/material_demo/tree.rs`, `examples/cupertino_demo/tree.rs`), but not in the onboarding path.
- **`examples/hello_world.rs` does not use widgets or the facade at all** — it calls `flui_platform::current_platform()` directly, opens a raw window, and logs display info. For a project whose facade doc-tests a `Hello, FLUI!` widget in 15 lines (`src/lib.rs`), the example literally named `hello_world.rs` teaches a completely different, lower-level API. This is the most concrete "two onboarding paths" artifact found.

---

# Executive Summary (top 10 findings, ranked for beta-release importance)

1. **The example named `hello_world.rs` doesn't use widgets, `StatefulView`, or even the `flui` facade** — it calls `flui_platform::current_platform()` directly and opens a raw window. It contradicts the facade's own 15-line doctest quick start (`src/lib.rs:11-28`) and is the single most damaging first impression for a new user.
2. **The `flui new` CLI counter template ships zero state.** `crates/flui-cli/src/templates/counter.rs` generates a `StatelessView` with a hardcoded `Text::new("0")` and no button — the one project every beginner runs never shows `StatefulView`/`setState`, framework's core interaction pattern.
3. **Global `FontSystem` mutex** (`crates/flui-painting/src/text_layout/layout.rs`, `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`) serializes text layout across every window/thread — a concrete ceiling on multi-window/parallel-layout scaling, confirmed present as of this checkout.
4. **`flui-widgets` is 103,984 LOC, ~4x the next-largest crate**, and mixes navigator/routing, text editing, scrolling, focus, and image caching — no natural sub-boundary for a contributor to scope a change or for the compiler to isolate a rebuild.
5. **`flui-localizations` is 281 LOC but occupies its own DAG layer (8)** between the catalogs (7) and app/CLI (9) — a whole layer of ceremony for a near-empty crate, and `flui-macros` (822 LOC) is shipped as a workspace member while its own `Cargo.toml` comment calls it a "skeleton."
6. **`flui-platform` concentrates 340 of ~513 unsafe occurrences workspace-wide** at a low DAG layer (2) that nearly everything depends on transitively — the highest-unsafe crate has the widest blast radius, and project memory already tracks multiple owner-lane/thread-affinity incident classes there (#949, #1194, #1147).
7. **68 ADRs with no reading order or supersession index** — ADR-0002 is superseded by ADR-0027 but that's stated only in prose in `docs/architecture.md`, not marked in the ADR files themselves; a newcomer has no map of which of 68 decisions is current.
8. **Two multi-hundred-KB "contract" documents are load-bearing, not optional**: `docs/runtime-contract.toml` (262.9 KB) and `docs/PORT.md` (131.8 KB / 1,233 lines), plus a 148 KB `ROADMAP-TRACKER.md` — these are referenced by CI/tooling, so "skim later" isn't a real option for a contributor who needs to add a dependency or a widget correctly.
9. **1,143 `.unwrap()` call sites outside obvious test paths** (upper-bound grep) and 46 files over 2,000 lines, topped by `crates/flui-app/src/app/ui_realm.rs` at 11,525 lines — the core app-runtime file implementing ADR-0027's realm model is a single file bigger than most whole crates.
10. **The threading model is intentionally single-writer, `!Send + !Sync` per-`UiRealm`** (ADR-0027) — a deliberate, documented design, but it means the scaling story for multi-window/parallel work is "more realms," not "parallel layout within a realm"; no evidence of a data-parallel layout pass was found (list virtualization via `sumtree.rs` is the chosen scaling strategy instead).

Full detail, tables, and file citations: see the written report at `/private/tmp/claude-501/-Users-vanyastafford-Develop-flui/698e857d-009a-4faf-b742-073d774be294/scratchpad/reports/01-architecture.md`.
