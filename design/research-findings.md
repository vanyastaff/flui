# Research findings not yet acted on

The 2026-09-25 architecture review left a large body of raw research under
[`docs/research/2026-09-25-architecture-review/`][root]. The two final reports
([architecture][report-arch], [decisions][report-dec]) and the ADRs they produced
(ADR-0081 to ADR-0097) carry most of it. This page collects the rest:

- findings the final reports did not carry;
- places where the raw research and the reports disagree;
- market lessons worth keeping as design guidance;
- claims nobody verified, with how far each one was checked.

Each item links to its raw source, rates its importance, and names where it belongs. When a
finding is acted on, move it into its ADR, plan step or issue and delete it here. This page is
a queue, not a record.

**Importance.** **H** changes a decision, an exit criterion or correctness. **M** belongs in
an ADR or the plan. **L** is hygiene.

**Evidence.** A `path:line` citation was re-checked in the worktree at `cab06137d`. An item
marked *map claim* cites only the research map; the code was not re-checked for this page,
so confirm it before relying on it.

**Belongs in.** An ADR number, a step in the
[migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md), an issue, or an
entry in [open questions](open-questions.md). Decisions already taken are indexed in
[decisions](decisions.md). The dynamic linking study has its own page,
[dynamic-linking.md](dynamic-linking.md).

## 1. Claims the reports rely on, re-checked

| Claim | Evidence | Status |
|---|---|---|
| One import pulls `flui-platform` into `flui-interaction` | `crates/flui-interaction/src/text_input.rs:27` (`use flui_platform::traits::PlatformTextInput;`) | Confirmed |
| Signals stay opt-in "until the #1090 field-mask registry lands" | The comment is at `Cargo.toml:642-645`. #1090 landed as `588251a1c`, and `depend_on_fields` ships at `crates/flui-material/src/theme.rs:93` | **Stale.** The blocker the manifest names is gone; `crates/flui-view/Cargo.toml:119-123` names the ADR-0074 go/no-go measurement instead |
| `realm_dispatch.rs` (7,149 lines) is a production smear | `crates/flui-app/src/app/runner/realm_dispatch.rs`: 7,149 lines, but the test module starts at `:1691-1692` and holds 77 `#[test]`s | **Mostly tests.** About 1,690 production lines |
| Damage is always full | `crates/flui-app/src/app/raster_lane.rs:354`; `DamageRegion` has only `Full` (`crates/flui-layer/src/scene_snapshot.rs:18-20`) | Confirmed |
| Font state is process-global | `crates/flui-painting/src/text_layout/layout.rs:124` | Confirmed |
| `Layer` has 15 variants ([engine map][m-engine]) or 19 ([rendering map][m-render]) | `pub enum Layer` at `crates/flui-layer/src/layer/mod.rs:78` has 19 variants | 19. The engine map is wrong |
| The runtime-contract ratchet "vanished" | `cf46dfe20` (#1283) deleted `docs/runtime-contract.toml` and `docs/panic-policy-allowlist.txt` on purpose: "those rules are types and clippy lints now (ADR-0078)" | **Framing is stale.** It was a deliberate deletion |
| `flui-hot-reload` links the `windows` crate directly | `crates/flui-hot-reload/Cargo.toml:47` | Confirmed |
| Android uses NativeActivity | `crates/flui-platform/Cargo.toml:162`, `crates/flui-app/Cargo.toml:145` | Confirmed |
| The AccessKit `Blur` mapping is wrong ([interaction map][m-inter]) | `crates/flui-semantics/src/accesskit_translation.rs:272-273` maps it, and `:296-297` documents the mapping as deliberate | **Contested.** See section 3 |
| wgpu has no fallback backend | One backend per OS (`crates/flui-engine/Cargo.toml:105-112`); `gles` is an opt-in feature only (`:30`) | Confirmed |

## 2. Findings the final reports did not carry

### 2.1 Runtime and view spine

| Imp | Finding | Evidence | Source | Belongs in |
|---|---|---|---|---|
| H | A `GlobalKey` lookup during a frame can deadlock. `draw_frame_impl` holds the binding's write lock (`crates/flui-view/src/binding.rs:1241`) across `build_scope` (`:1297`). The registry closures take `inner.read()` (`:698-705`) on a non-reentrant `parking_lot::RwLock` (`:67`), and the file itself names the hazard (`:505`). The deadlock has not been executed. Fix with an owner-local binding or a `Busy` error, and a test that fails today | Code shape confirmed | [view_element][m-view] | Issue, with the failing test first |
| H | `ElementCore`'s `depth` field stores the sibling slot, not the depth (`crates/flui-view/src/element/generic.rs:76-78`, set at `:151`). Code that reads it as a tree depth orders wrongly | Confirmed | [view_element][m-view] | Issue: rename the field and audit its readers |
| M | ADR-0075 effects run only in the headless harness, not in the production frame. That is a second, test-only frame path, which the "one implementation per contract" rule does not list | *Map claim* | [xcut_performance][m-perf] | [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md): name the effects slot in the phase order |
| M | Mobile hosts are not yet "runners over the same runtime". On iOS, `run_direct` and the window and exit policies are compiled out (`crates/flui-app/src/lib.rs:55,88`). The map also reports that Android and web install no exit-policy hook | Partly confirmed | [app_runtime_scheduler][m-app] | Migration plan: the runtime extraction step names each host's gap |
| M | `BuildContext::get_inherited` is a public read that records no dependency (`crates/flui-view/src/context/build_context.rs:189-199`). It contradicts the view layer's dependency-tracking invariant | Confirmed | [view_element][m-view] | [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md), or restrict it to `LifecycleContext` |
| M | `GlobalKey::with_current_state` gives only `&T`, so Router, Form and Scaffold handles need interior mutability. The controller pattern is undecided | *Map claim* | [view_element][m-view] | [ADR-0093](../docs/adr/ADR-0093-router-is-the-primary-navigation-api.md) |
| M | `ViewState` has no reassemble or migration hook and no state-layout fingerprint. Subsecond's "a state edit restarts the realm" rule needs the fingerprint to detect such an edit | *Map claim* | [view_element][m-view], [rust_ui_architectures][mk-rust] | [ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md) |
| M | A catalog derive that emits a `const NAME` could replace `TypeId` as the identity in tree observation (`crates/flui-foundation/src/observe.rs`). The final report keeps the derive but drops this role | *Map claim* | [synthesis §11][synth], [view_element][m-view] | [ADR-0095](../docs/adr/ADR-0095-agent-protocol-schema-crate.md) |
| L | `Element<V, A, B>` has more than 130 monomorphs, about 13.7% of `flui-widgets`' LLVM lines. It costs compile time, and it inflates the export table of a development dylib ([dynamic-linking.md](dynamic-linking.md)) | *Map claim* | [view_element][m-view] | Issue, measured with `cargo llvm-lines` |
| L | The predictive-back surface carries its own `REMOVE_BY: 2026-12-22` note (`crates/flui-view/src/binding.rs:537`) | Confirmed | [view_element][m-view] | Issue dated for the removal |

### 2.2 Rendering, engine and text

| Imp | Finding | Evidence | Source | Belongs in |
|---|---|---|---|---|
| H | `clip_path` clips to the path's bounding-box scissor, and `ClipOp::Difference` is refused (`crates/flui-engine/ARCHITECTURE.md:291,317`). Rounded and custom Material and Cupertino shapes render wrong with no error | Confirmed in the crate doc | [engine_painting_text][m-engine] | Issue with a readback test; a correctness gate for the beta |
| H | A paint or layer-update poison drops the whole frame until the node recovers, so one bad third-party render object freezes the UI. [synthesis §10][synth] proposed an error-box picture after repeated poison; the final report keeps `guarded_call` but dropped it | *Map claim* | [rendering_objects_layer][m-render], [synthesis][synth] | [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) (containment), with a test |
| M | Sliver intrinsic and dry-layout queries return zero in release, and there is no sliver layout cache (#1199). AGENTS.md counts intrinsics left at defaults as a defect | *Map claim* | [xcut_performance][m-perf] | Issue under #1199 |
| M | The GPU image cache is keyed by the `Arc` data pointer (`TextureKey::Pointer`, `crates/flui-engine/src/texture_cache.rs:65-73`), and the cached entry holds only a `TextureView` (`:101-106`), so a freed and reused allocation can hit a stale texture. ABA not reproduced | Code shape confirmed | [engine_painting_text][m-engine] | Issue; `flui_types::Image` needs a stable id |
| M | `register_font` does not invalidate laid-out text. The final report fixes only the asynchronous system-font scan | *Map claim* | [engine_painting_text][m-engine] | [ADR-0092](../docs/adr/ADR-0092-per-realm-text-over-parley.md) |
| M | Device-loss recovery once one `GpuContext` is shared by every window was in [synthesis §5.4][synth] and was dropped | Design gap | [synthesis][synth], [engine_painting_text][m-engine] | [ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) |
| M | With one wgpu backend per OS and no GLES fallback, Android devices without Vulkan and Linux VMs get no renderer. That makes the CPU backend a production path, not only a test oracle | Backend selection confirmed (section 1) | [rendering_text_platform][mk-render] | [ADR-0087](../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) |
| M | [ADR-0066](../docs/adr/ADR-0066-display-list-command-representation.md) keeps serde off the display list, which conflicts with record and replay and with devtools frame inspection. Not reconciled | ADR text | [engine_painting_text][m-engine] | ADR-0087 or ADR-0095: say where a serialized form lives |
| M | `RenderObject`, `RenderBox` and the blanket impl repeat about 30 hooks, with input hooks welded to layout. Freezing a ~60-method trait at H3 forecloses Masonry's shape, where the framework passes over the arena | *Map claim* | [rendering_objects_layer][m-render], [rust_ui_architectures][mk-rust] | [Open questions](open-questions.md), before any H3 freeze |
| M | `Protocol` is sealed to Box and Sliver (`crates/flui-rendering/src/protocol/protocol.rs:54`). Whether that stays sealed after H3 is not recorded | Confirmed | [rendering_objects_layer][m-render], [ecosystem_evolution_first §11][d-eco] | [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) stability kinds |
| M | Parley acceptance gaps: Parley drops glyph `byte_index`/`byte_length` (caret and selection depend on them); vello_cpu panics on complex filter graphs and has no filters in multi-threaded mode; "OS hinting in production" should read "one shaper, only rasterizer and hinting vary" | Secondary sources | [rust_ui_architectures][mk-rust], [rendering_text_platform][mk-render] | ADR-0092 acceptance criteria |
| M | Damage tracking needs an off switch that really removes its cost (Flutter #95681) | Market lesson | [rendering_text_platform][mk-render] | ADR-0087 |
| L | The root repaint boundary is never retained, and boundary placement is only a catalog convention. Automatic promotion was not considered | *Map claim* | [xcut_performance][m-perf] | ADR-0087 follow-up |
| L | KeepAlive has no widget; the roadmap row contradicts ADR-0056; excluding stamps makes the a11y tree depend on history | *Map claim* | [rendering_objects_layer][m-render] | Issue |
| L | `crates/flui-rendering/ARCHITECTURE.md`, the ADR-0017 context and ADR-0056 ("the facade does not re-export rendering") contradict the code | *Map claim* | [rendering_objects_layer][m-render] | Doc fix |

### 2.3 Platform, input and accessibility

| Imp | Finding | Evidence | Source | Belongs in |
|---|---|---|---|---|
| H | The web backend has no IME and no accessibility (a canvas only), yet the H0 exit includes the Notes app on the web. No plan step covers a hidden-input IME bridge or a DOM/ARIA mirror | *Map claim*, confirmed by the mining pass | [platform_layer][m-plat] | Resolved by the owner ([item 17](open-questions.md#17-web-ime-and-accessibility-for-the-h0-exit)): web in H0 is rendering and pointer input; the IME bridge and DOM/ARIA mirror are H1 items |
| M | `flui-hot-reload` depends on `windows` directly (`crates/flui-hot-reload/Cargo.toml:47`), against "platform types stay inside `flui-platform`". The package reach gate would fail on it, and no exemption is listed | Confirmed | [xcut_safety_health][m-safety] | ADR-0081 allowlist, or it retires with the dlopen path under ADR-0094 |
| M | Intents are keyed by `TypeId` (`pub trait Intent: Any {}`, `crates/flui-widgets/src/interaction/actions.rs:59`), and EditableText handles keys itself. There is no named command registry for native menus or for agents | Confirmed | [interaction_semantics][m-inter] | ADR (commands as data), before the B2 menus exit |
| M | No backend has menu code. The B2 exit (native menus and dialogs) has no contract, and the "menu as data" ADR was never written | *Map claim* | [platform_layer][m-plat] | New ADR, with the command registry above |
| M | The AccessKit action vocabulary drops Expand, Collapse, numeric SetValue and ScrollToPoint, while ADR-0080 advertises expand, collapse and set_value. A pin that checks a mapping exists does not check ADR-0080 coverage | *Map claim* | [interaction_semantics][m-inter] | ADR-0095, with a coverage test |
| M | `SemanticsHost::announce` (`crates/flui-app/src/app/semantics_host.rs:267`) has no caller outside its tests, so live regions (snackbars, validation errors) never reach the OS | Confirmed by grep | [interaction_semantics][m-inter] | Issue |
| M | Android `native-activity` may not support soft-keyboard IME, which may need GameActivity (hypothesis). The text-store contract should also follow winit 0.31's IME vocabulary (DeleteSurrounding, purpose and hints) and the coordinated `ui-events-winit` bump | NativeActivity confirmed; the rest unverified | [platform_layer][m-plat], [rendering_text_platform][mk-render] | [ADR-0090](../docs/adr/ADR-0090-ime-pull-text-store-contract.md) |
| M | `FLUI_HEADLESS` selects the backend through an environment variable, which a static scan for globals cannot see | *Map claim* | [platform_layer][m-plat] | [ADR-0097](../docs/adr/ADR-0097-no-process-global-state-gate.md) allowlist |
| M | The web backend justifies `unsafe impl`s with "SAFETY: WASM is single-threaded" (`crates/flui-platform/src/platforms/web/platform.rs:47`, also `clipboard.rs:24`, `executor.rs:13`). That is unsound once wasm threads are enabled | Confirmed | [platform_layer][m-plat] | Issue; forbid `target_feature = "atomics"` or fix the types |
| L | `crates/flui-platform/src/lib.rs:296,299` rates Windows "Production 10/10" and Android "Stub 2/10", while `docs/BETA.md` calls Windows experimental | Confirmed | [platform_layer][m-plat] | Doc fix |
| L | `GestureSettings` does not read the OS double-click time or drag slop | *Map claim* | [interaction_semantics][m-inter] | Issue |
| L | `examples/android_*` are outside the workspace and CI does not build them | *Map claim* | [workspace_topology][m-ws] | Issue |

### 2.4 Workspace, tooling and process

| Imp | Finding | Evidence | Source | Belongs in |
|---|---|---|---|---|
| H | The new gates (process globals, release check, a `BUG:`-prefix lint) bring back rules that `cf46dfe20` (#1283) deleted because they "are types and clippy lints now (ADR-0078)". That commit also removed the panic allowlist and the publish dry-run. Each new gate must say why no type or lint covers it | Commit confirmed | [workspace_topology][m-ws] | ADR-0097, and an amendment to [ADR-0078](../docs/adr/ADR-0078-rules-live-in-types-and-lints.md) |
| H | A development `dynamic-linking` facade feature appears in the architecture report and in three designs, but nobody had designed or measured it. That is now done; on Windows the dylib exports sit close to the 65,535 limit | Measured, see [dynamic-linking.md](dynamic-linking.md) | [workspace_ecosystem_structure][mk-ws], [dx_first][d-dx], [performance_first][d-perf] | [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md) |
| M | Two hot-reload worker hazards the crate does not document. (1) `WorkerReloadDriver::poll` unloads the old image (`crates/flui-hot-reload/src/worker.rs:458`) before the realm reassembles (`crates/flui-app/src/app/hot_reload.rs:226-227`), while the element tree may still hold views and closures from it. (2) The worker is a `cdylib` with its own copy of `REQUEST_REBUILD` (`crates/flui-hot-reload/src/dispatch.rs:24,78-85`), so a rebuild request from worker code finds no host hook. Both are hypotheses; neither has been run | Code shape confirmed | [dynamic-linking.md](dynamic-linking.md) | Windows repro, then `flui-hot-reload` crate docs until ADR-0094 deletes the path |
| M | CLI `--json` emits NDJSON events with no schema or version, and `cargo xtask device` already consumes the stream (`tools/xtask/src/device.rs:204`) | Consumer confirmed | [tooling_testing_docs][m-tool] | ADR-0095: version the events as protocol |
| M | External dependencies bypass `[workspace.dependencies]`: `raw-window-handle` is declared directly in `crates/flui-platform/Cargo.toml:74` and `crates/flui-engine/Cargo.toml:60`, and `crates/flui-platform/Cargo.toml:77` pins `bitflags = "2.6"` against the workspace's `2.10` (`Cargo.toml:305`). No gate catches this | Confirmed | [workspace_topology][m-ws] | `cargo xtask workspace` check (manifests inherit shared dependencies) |
| M | The living plan is a private claude.ai artifact (`docs/ROADMAP.md:5`). That blocks the H3 "external contributor" exit, and it is the reason this `design/` folder exists | Confirmed | [tooling_testing_docs][m-tool], [plan_alignment][m-plan] | Resolved by the owner ([item 18](open-questions.md#18-where-the-living-plan-lives)): the repository is canonical |
| M | All 15 Rust fences in the book are `rust,ignore`, the docs workflow runs no `mdbook test` (`.github/workflows/docs.yml:10`), and `book/src/concepts/state.md:45` says signals do not exist | Confirmed | [tooling_testing_docs][m-tool], [xcut_api_dx][m-api] | Issue |
| M | Windows was dropped from the CI test matrix "temporarily" (`.github/workflows/ci.yml:638-643`). The Windows evidence gate adds only the a11y probe, not the test suite | Confirmed | [tooling_testing_docs][m-tool] | An input to the [CI redesign](open-questions.md#ci-redesign) |
| M | The `unwrap` target is already met: no bare `unwrap()` in production and no file above 2,000 production lines. The owner roadmap's "1143 unwrap" and "46 files over 2000" figures are out of date | *Map claim* | [xcut_safety_health][m-safety] | Roadmap update |
| M | Owner-roadmap lines that neither report edits still describe removed tooling or missing features: `docs/workspace-layers.toml` and `docs/runtime-contract.toml` (both deleted in #1283), a `flui-state` crate, "68 ADR" (there are 61), and nested scrolling (no `NestedScroll` in the code) | Checked against the tree | [plan_alignment][m-plan] | Roadmap update |
| L | `tools/text-spike` is a nested workspace with its own lockfile, outside every gate. `tools/` has no layer, so its dependencies (rmcp, uiautomation, xcap) sit in the root lock | *Map claim* | [tooling_testing_docs][m-tool] | ADR-0081 (tool tier) |

### 2.5 API and developer experience

| Imp | Finding | Evidence | Source | Belongs in |
|---|---|---|---|---|
| H | Copy and paste are table stakes for the "form in ten minutes" target, but the runtime's clipboard accessor has no production caller (`crates/flui-app/src/app/runtime.rs:1632-1641`), and EditableText documents copy, paste and cut as "not wired" (`crates/flui-widgets/src/text/editable_text.rs:322`). Both reports schedule clipboard only at B2, as a client of the capability seam | Confirmed | [xcut_api_dx][m-api] | Migration plan: move clipboard ahead of the seam, or pull the seam earlier; see [ADR-0084](../docs/adr/ADR-0084-open-capability-seam-and-plugins.md) |
| M | Signal batching (N writes, one notification) and one reader registry (#1249, #1254) should land before Stable. [ADR-0074](../docs/adr/ADR-0074-realm-scoped-signals.md) (line 10) routes a value "read by hundreds of cells" to `InheritedView`, which turns an implementation cost into API guidance | ADR text | [reactivity_state][mk-react], [xcut_performance][m-perf] | ADR-0085 |
| M | "Every push produces a URL-addressable entry; no pageless routes" was in [synthesis §8][synth] and was dropped from D14 without a reason. It is Flutter's Navigator 1/2 lesson | Design gap | [synthesis][synth], [flutter_compose_swiftui][mk-flutter] | ADR-0093 |
| M | There is no `Icons` table (`examples/todo.rs:32` builds `IconData::new(0xE872)`), and `ListView` lays out at a fixed `item_extent` (`crates/flui-widgets/src/scroll/list_view.rs:68`). There is no rule for when a widget is a builder and when a struct | Confirmed | [xcut_api_dx][m-api] | Issues; the builder rule goes in the widget guide |
| M | ADR-0042 ("no universal ThemeData") conflicts with `flui_sdk::tokens` and `ThemeData::from_tokens` in the architecture report. No amendment is listed | ADR text | [plan_alignment][m-plan] | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) |
| M | ADR-0028 conformance defects: InkWell and the surface substrate live in Material, not below both design systems, and EditableText branches on the compile-time platform (`word_jump_modifier(TargetPlatform)` at `crates/flui-widgets/src/text/editable_text.rs:1500`, `TargetPlatform::current()` at `:1583`) | Confirmed | [widgets_catalogs_facade][m-widgets] | ADR-0088 (the substrate moves down); issue for the platform branch |
| L | `CustomPainter::as_any` can go now that trait upcasting is stable. `Theme::of` clones the whole `ThemeData` at 29 call sites | *Map claim* | [xcut_api_dx][m-api], [widgets_catalogs_facade][m-widgets] | Issues |
| L | The published `flui` archive includes the engine and platform probe examples | *Map claim* | [widgets_catalogs_facade][m-widgets] | ADR-0088 (what the facade ships) |

### 2.6 Agent-facing surface

All of [synthesis §11][synth] was dropped from the final report. These items belong in
[ADR-0095](../docs/adr/ADR-0095-agent-protocol-schema-crate.md) or its follow-up.

| Imp | Finding | Source |
|---|---|---|
| M | Bounded reads (scope, depth, maximum nodes, concise or detailed) for the element, render and layer trees, diagnostics and traces. `DiagnosticsNode::to_string_deep` is unbounded | [ai_native_tooling][mk-ai], [synthesis][synth] |
| M | Every action returns the outline or diff after the action. The outline doubles as the golden file: one node per line, deterministic | [ai_native_tooling][mk-ai] |
| M | MCP 2026-07-28 is stateless: the handle table belongs to the backend, and logging goes through OpenTelemetry, not MCP Logging or Sampling | [ai_native_tooling][mk-ai] |
| M | An evaluation harness for the agent-success metric; a headless preview registry; app-as-tool (in the style of AppFunctions) as a separate role; the Chrome-trace exporter as a protocol method | [ai_native_tooling][mk-ai] |

## 3. Where the raw research and the reports disagree

`report-decisions.ru.md` wins over `report-architecture.ru.md` where they disagree. These
are cases where the raw research, or the code, disagrees with the reports.

1. **`realm_dispatch.rs`.** The architecture report and owner decision 4 treat its 7,149
   lines as runtime logic smeared into one file. About 1,690 are production code (section 1).
   [app_runtime_scheduler][m-app] ("mostly tests") is right; [plan_alignment][m-plan]
   ("production code with no inline tests") is wrong. Moving the tests out fixes the
   file-length problem without splitting the file.
2. **Rebuild weight.** Owner decision 5 and the architecture report count engine and widgets
   lines including tests and comments. [engine_painting_text][m-engine] counts about 18.8k
   executable lines in the engine, and the widgets crate carries 14.3k lines in test files
   alone. The warm-edit gain estimated for extracting a reactive crate is inflated by the same
   amount. [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md)
   should rest on a measurement, not the line estimate. Settled: ADR-0085 now rests on a
   measured warm edit and keeps the graph in `flui-view`, with no reactive crate.
3. **The signals gate.** Owner decision 5 makes its first step wait for a go/no-go "before
   #1090", but #1090 has landed (section 1). What remains is the ADR-0074 measurement, and
   possibly unifying the two reader registries (#1254). ADR-0085 must say which.
4. **Version-pin count.** The architecture report says 145, the decisions report 172. Both
   are right: 145 in the member manifests, 27 in the root manifest, 14 of them in
   `flui-material`. Any ADR that uses the number must state its scope.
5. **`Layer` variants.** 19, not 15 (section 1).
6. **AccessKit on iOS.** [interaction_semantics][m-inter] says there is no iOS adapter.
   [rendering_text_platform][mk-render] reports `accesskit_ios` 0.2.1 and `accesskit_android`
   0.9.0, released 2026-09-25. The reports follow the market survey. The survey means the
   adapters exist upstream; FLUI still uses only the desktop adapters.
7. **The ratchet file.** The architecture report says it vanished; it was deleted on purpose
   in #1283, together with the publish dry-run and the panic allowlist (section 1). That also
   answers [workspace_topology][m-ws]'s question about the missing #1236 dry-run.
8. **SDK version cadence.** The verifier of [q2_sdk_stability][q2] recommended bumping the
   SDK's `0.N` only on release trains that change Evolving items. The final decision bumps on
   every train unconditionally, and defers only third-party lag to the owner. The reason for
   rejecting the mitigation is not recorded.
9. **Dropped from [synthesis][synth] without a recorded reason.** No pageless routes;
   the agent-facing surface (§11); device loss per `GpuContext`; the paint-poison error box;
   the "route push" frame budget; the benchmark suite that mirrors Flutter's.
10. **AccessKit `Blur`.** [interaction_semantics][m-inter] calls the mapping wrong. The code
    documents it as deliberate (`crates/flui-semantics/src/accesskit_translation.rs:296-297`).
    Unresolved; it needs a screen-reader run, not a reading.
11. **Rebuild fan-out.** The architecture report says fan-out was not measured.
    [workspace_topology][m-ws] did measure dependent counts with `cargo tree` (16 crates
    depend on `flui-platform`, 26 on `flui-foundation`). Only wall time is unmeasured.
12. **Counts that differ by grep pattern.** `&dyn LifecycleContext` sites: 136 in the
    architecture report, 122 in [judges-and-verification][judges]. `on_*` setters: 92 against
    91. `flui-platform` size: 50.4k lines against 46,302 re-measured. An ADR that quotes one
    of these numbers must quote the command that produced it.

## 4. Market lessons as design guidance

These are not decisions. They are the lessons from the market surveys that should shape a
decision when it comes up, and they name the ADR where they apply.

| Lesson | What it means for FLUI | Source | Applies to |
|---|---|---|---|
| Navigation state is a value | No second, non-addressable route kind; dialogs and overlays are either part of that state or explicitly excluded (Flutter's pageless routes, SwiftUI's `NavigationStack`) | [flutter_compose_swiftui][mk-flutter] | ADR-0093 |
| Field-granular inherited reads | Field reads are the default; a whole-provider `depend_on` is the flagged exception (the `MediaQuery.of` trap). Lint whole-provider dependencies on high-fan-out providers | [flutter_compose_swiftui][mk-flutter] | ADR-0085 |
| One saveable-state contract | Keep-alive eviction, the Router back stack and Subsecond share one contract (Compose `rememberSaveable`), next to the existing RAII keep-alive lease | [flutter_compose_swiftui][mk-flutter] | ADR-0093, ADR-0094 |
| Batch signal writes | N writes, one notification, rather than MVCC. Settle synchronous versus deferred visibility and the effect API before Stable; Solid needed a major version to change exactly these | [reactivity_state][mk-react] | ADR-0085, ADR-0086 |
| Collection key is observation identity | One concept. Pin it with a test: moving a row keeps its element and does not rebuild its siblings (SwiftUI `ForEach` against Observation) | [reactivity_state][mk-react] | ADR-0085 |
| Headless runtime | Reconciliation and signals stay usable without rendering (Compose's runtime under Molecule and Redwood) | [reactivity_state][mk-react] | ADR-0083, ADR-0085 |
| Type-erase at the user boundary | Keep arity and generic machinery out of Stable signatures (the Xilem critique). This is in tension with a Stable `RenderBox` surface that includes `Arity` | [rust_ui_architectures][mk-rust] | ADR-0081, ADR-0088 |
| Cross-cutting behavior as framework passes | Focus, cursor, semantics and hit-testing as passes over the arena, not per-widget trait hooks (Masonry) | [rust_ui_architectures][mk-rust] | [Open questions](open-questions.md) (render trait shape) |
| Settle internals before the freeze | Masonry lost a year by postponing this behind features | [rust_ui_architectures][mk-rust] | [Migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md) ordering |
| Record at the input seam | Record and replay capture input at the platform-to-interaction seam, as Iced's time travel does | [rust_ui_architectures][mk-rust] | ADR-0095 |
| Measure the agent surface | Measure agent success for the typed catalog plus A2UI against a DSL (Makepad, Slint) instead of asserting it. An in-process `flui mcp` is table stakes by 2027; the single catalog is the differentiator | [ai_native_tooling][mk-ai] | ADR-0095 |
| Version the protocol apart from AccessKit | An AccessKit bump must not silently change the agent schema | [rendering_text_platform][mk-render] | ADR-0095 |
| Catalog from a derive | Generate the catalog from a derive on stable Rust; rustdoc JSON is nightly enrichment only | [ai_native_tooling][mk-ai] | ADR-0095 |
| Passive agent context | Put a compressed catalog index into the `AGENTS.md` that `flui create` writes (Vercel's evaluation: passive context 100% against 79% for skills) | [ai_native_tooling][mk-ai] | `flui create` template issue |
| A slower surface for extension authors | A stable-subset crate on a slower cadence than the catalog (the `bevy_platform` proposal), plus plugin-author guidance: a naming prefix, minimal facade features, a compatibility table | [workspace_ecosystem_structure][mk-ws] | ADR-0088 |
| Split crates only where the graph is wide | Measure with `cargo build --timings` before splitting (matklad). `cargo publish --workspace` is not atomic, which argues for fewer publish units | [workspace_ecosystem_structure][mk-ws] | ADR-0081 |
| The hero app uses only the published facade | Otherwise the framework ends up trapped in its app, as GPUI did in Zed | [workspace_ecosystem_structure][mk-ws] | ADR-0088 |
| One shaper everywhere | Only rasterization and hinting vary by platform (GPUI's per-OS text regret, zed#13951) | [rendering_text_platform][mk-render] | ADR-0092 |
| A closed, pre-warmable effect set | Labelled GPU resources and an effect set that can be compiled ahead (Impeller, Graphite); damage with a true off switch | [rendering_text_platform][mk-render] | ADR-0087 |
| Extract a design system in order | Raw primitives first, then freeze, re-release, compatibility bridge and a `flui migrate` step, as Flutter did with `MaterialUiCompatibilityBridge` | [flutter_compose_swiftui][mk-flutter] | ADR-0088 |

## 5. Claims nobody verified

Nothing below may be stated as fact in an ADR or a doc until it is checked. Status:
**open** (not checked), **partial** (the code shape or a probe supports it, the behavior has
not been run), **resolved** (checked since the research; the result is given).

| Claim | Status | How to check | Source | Belongs in |
|---|---|---|---|---|
| The names `flui-sdk`, `flui-reactive`, `flui-protocol`, `flui-platform-api`, `flui-runtime`, `flui-engine-cpu` are free on crates.io | Open (the crates.io tool failed in every run) | Look each one up before the ADR that names it is accepted | [q2_sdk_stability][q2], [report-decisions][report-dec] | ADR-0082, ADR-0083, ADR-0087, ADR-0088, ADR-0095 |
| Subsecond works on Windows and Android, patches reach code through the vtables of `Box<dyn ElementBase>` created before the patch, and its thread-local reset copes with the remaining statics | Open | The Subsecond spike | [rust_ui_architectures][mk-rust], [judges-and-verification][judges] | ADR-0094 |
| A Bevy-style development dylib links on Windows and speeds up iteration | **Resolved.** It links with the default features, at 64,336 of 65,535 exports, and fails with `LNK1189` once every facade feature is on. An app-crate edit takes 1.0-1.3 s dynamic against 1.4-2.9 s static (0.3-1.5 s saved); a framework edit gets slower | See [dynamic-linking.md](dynamic-linking.md) | [workspace_ecosystem_structure][mk-ws] | ADR-0096 |
| `with_current_state` deadlocks under the frame's write lock | Partial (code shape, section 2.1) | A test that calls it from `build` | [view_element][m-view] | Issue |
| Rendering: stale pixels outside a swapchain scissor; blit cost of the retained target on tile GPUs; the cold-start split including the font scan; lazy-band passes per frame; the per-level view-clone cost; extra frames from the loop-wide `needs_redraw` | Open | Benchmarks and traces on real hardware | [xcut_performance][m-perf], [performance_first][d-perf] | ADR-0087, ADR-0091 |
| Build cost: duplicate upper-stack builds from per-crate `testing` features; "Cargo has no early cutoff"; the warm-edit gain of a reactive crate; the doubled compile cost of a nested workspace | Open, except the reactive crate: settled by ADR-0085's measured warm edit, which keeps the graph in `flui-view` (the nested-workspace figure rests on a two-crate probe) | `cargo build --timings` on the real workspace | [workspace_topology][m-ws], [q5_reactive_crate][q5] | ADR-0081, ADR-0085 |
| rustdoc JSON, cargo-public-api and cargo-semver-checks need nightly; cargo-public-api output includes the transitive closure; semver-checks allows per-module exemptions | Open | Run each tool on the pinned toolchain | [q2_sdk_stability][q2] | ADR-0089 |
| The resolver's error text for conflicting `=` pins through a real registry | Partial (only a directory-source probe) | A publish to a local registry | [q1_packages_location][q1], [judges-and-verification][judges] | ADR-0088 |
| `KEYEVENTF_UNICODE` bypasses the IME; `windows-a11y` and `windows-input` pass on hosted `windows-latest`; hosted runners support ja-JP | Open | One CI run on a hosted runner | [q8_windows_evidence_gate][q8] | ADR-0090, the Windows evidence gate |
| 12 to 20 Evolving SDK items; plugins depend on about 30 crates; a Win32 edit rebuilds 3 crates | Open (estimates) | Count after the SDK is drafted; `cargo tree -i` | [q2_sdk_stability][q2], [workspace_topology][m-ws] | ADR-0088 |
| Static id counters reach snapshots or the protocol; vello_cpu is bit-deterministic across CPUs with pinned SIMD; the normalized outline from UI Automation equals the in-process one | Open | Determinism tests on two machines | [safety_correctness_first][d-safety], [ai_native_first][d-ai] | ADR-0087, ADR-0095 |
| `accesskit_ios` 0.2 is mature enough; Android IME needs GameActivity; the scope of web a11y and IME for the H0 web target | Open | A device run for each | [rendering_text_platform][mk-render], [platform_layer][m-plat] | ADR-0090, [open questions](open-questions.md) |
| `panic = "abort"` on wasm32 makes lock poisoning inert; the image-cache ABA; FNV-1a key collisions silently reconcile the wrong child | Open (the image-cache code shape is confirmed, section 2.2) | Targeted tests | [rendering_objects_layer][m-render], [values_foundation_tree][m-values], [engine_painting_text][m-engine] | Issues |
| Secondary-source facts: Bevy's ecosystem lags 2 to 8 weeks behind a release; Flutter 3.47's `flutter create` adds `material_ui`; Parley release dates (the release page and the blog disagree); Slint and GPUI internals, recalled from memory | Open | Re-fetch the primary source before quoting | [workspace_ecosystem_structure][mk-ws], [flutter_compose_swiftui][mk-flutter], [rust_ui_architectures][mk-rust] | Wherever quoted |
| The cost of always-on semantics (`publish_cost` on the Notes app and on a 100k-row list) | Open | Run the bench | [interaction_semantics][m-inter], [ai_native_first][d-ai] | ADR-0095, the a11y default |

[root]: ../docs/research/2026-09-25-architecture-review/
[report-arch]: ../docs/research/2026-09-25-architecture-review/report-architecture.ru.md
[report-dec]: ../docs/research/2026-09-25-architecture-review/report-decisions.ru.md
[synth]: ../docs/research/2026-09-25-architecture-review/synthesis.md
[judges]: ../docs/research/2026-09-25-architecture-review/judges-and-verification.md
[m-app]: ../docs/research/2026-09-25-architecture-review/maps/app_runtime_scheduler.md
[m-engine]: ../docs/research/2026-09-25-architecture-review/maps/engine_painting_text.md
[m-inter]: ../docs/research/2026-09-25-architecture-review/maps/interaction_semantics.md
[m-plan]: ../docs/research/2026-09-25-architecture-review/maps/plan_alignment.md
[m-plat]: ../docs/research/2026-09-25-architecture-review/maps/platform_layer.md
[m-render]: ../docs/research/2026-09-25-architecture-review/maps/rendering_objects_layer.md
[m-tool]: ../docs/research/2026-09-25-architecture-review/maps/tooling_testing_docs.md
[m-values]: ../docs/research/2026-09-25-architecture-review/maps/values_foundation_tree.md
[m-view]: ../docs/research/2026-09-25-architecture-review/maps/view_element.md
[m-widgets]: ../docs/research/2026-09-25-architecture-review/maps/widgets_catalogs_facade.md
[m-ws]: ../docs/research/2026-09-25-architecture-review/maps/workspace_topology.md
[m-api]: ../docs/research/2026-09-25-architecture-review/maps/xcut_api_dx.md
[m-perf]: ../docs/research/2026-09-25-architecture-review/maps/xcut_performance.md
[m-safety]: ../docs/research/2026-09-25-architecture-review/maps/xcut_safety_health.md
[mk-ai]: ../docs/research/2026-09-25-architecture-review/market/ai_native_tooling.md
[mk-flutter]: ../docs/research/2026-09-25-architecture-review/market/flutter_compose_swiftui.md
[mk-react]: ../docs/research/2026-09-25-architecture-review/market/reactivity_state.md
[mk-render]: ../docs/research/2026-09-25-architecture-review/market/rendering_text_platform.md
[mk-rust]: ../docs/research/2026-09-25-architecture-review/market/rust_ui_architectures.md
[mk-ws]: ../docs/research/2026-09-25-architecture-review/market/workspace_ecosystem_structure.md
[d-ai]: ../docs/research/2026-09-25-architecture-review/designs/ai_native_first.md
[d-dx]: ../docs/research/2026-09-25-architecture-review/designs/dx_first.md
[d-eco]: ../docs/research/2026-09-25-architecture-review/designs/ecosystem_evolution_first.md
[d-perf]: ../docs/research/2026-09-25-architecture-review/designs/performance_first.md
[d-safety]: ../docs/research/2026-09-25-architecture-review/designs/safety_correctness_first.md
[q1]: ../docs/research/2026-09-25-architecture-review/decisions/q1_packages_location.md
[q2]: ../docs/research/2026-09-25-architecture-review/decisions/q2_sdk_stability.md
[q5]: ../docs/research/2026-09-25-architecture-review/decisions/q5_reactive_crate.md
[q8]: ../docs/research/2026-09-25-architecture-review/decisions/q8_windows_evidence_gate.md
