# Architecture decisions

- **Status:** Index of proposed decisions; every linked ADR is Proposed. The owner answered the
  open questions on 2026-09-25; see [Owner decisions of 2026-09-25](#owner-decisions-of-2026-09-25)
- **Date:** 2026-09-25
- **Snapshot:** `main` at `cab06137d`
- **Sources:** [architecture report](../docs/research/2026-09-25-architecture-review/report-architecture.ru.md)
  (§10, decisions D1–D17) and [owner-decisions report](../docs/research/2026-09-25-architecture-review/report-decisions.ru.md)
  (the eight questions of the architecture report's §14, each answered by three judges and a verifier).
  Both are in Russian. Where they disagree, the owner-decisions report wins: it was written after
  verification and amends the architecture.

This page is the index from each decision to the record that carries it. The target shape these
decisions add up to is in [architecture.md](architecture.md); what still needs the owner is in
[open-questions.md](open-questions.md); the order of work is in the
[migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md).

## How to read the status column

- **Verified** — the verification pass (four lenses: topology, invariants and contracts,
  concurrency and performance, plan reality and DX) or a probe confirmed the facts the decision
  rests on, and the decision stands as first written.
- **Changed by verification** — a verifier finding changed the decision. The entry says what
  changed and why.
- **Re-checked** — every `path:line` below was re-read in the worktree at `cab06137d` for this
  page. A claim that could not be re-checked is labelled a hypothesis.
- **Owner, 2026-09-25** — the owner confirmed or changed the decision in the interview of that
  date. "Confirmed" means the decision stands as written; "changed" means the entry and its ADR
  now say what the owner decided.

None of these is an accepted decision, except the part of D8 that ADR-0081 accepted and D1, D2,
D16 and G in part (ADR-0082, ADR-0083, ADR-0095 and ADR-0097; below). D1's per-backend `Send` removal is
still Proposed, though Win32 has done its first step. The panel's answers are recommendations and
the owner's answers settle the open questions; acceptance still happens ADR by ADR.

## Summary

| # | Decision | Status | ADR |
|---|---|---|---|
| D1 | `flui-platform-api` is the contract crate; OS backends stay in `flui-platform` | Changed by verification (split into a mechanical move and a per-backend `Send` removal); **accepted in part** (2026-09-26): the capability traits and window/input vocabulary moved; `PlatformWindow`'s move, with `accessibility()` on the host-side `HostWindow` subtrait, is accepted on merge pending the evidence ADR-0082's Verification lists; the per-backend `Send` removal (§4, still Proposed) was revised to require refusing off-owner registration, and Win32 has done its first step | [ADR-0082](../docs/adr/ADR-0082-platform-api-contract-crate.md) |
| D2 | One frame transaction in `flui-runtime`, above `flui-widgets` | Changed by verification (test modules move; transaction defined by type); owner confirmed it in B0; **accepted in part** (2026-09-26): the crate exists in tier K above `flui-widgets` with the presentation lanes, the realm core follows in later moves | [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) |
| D3 | An open, typed capability set registered by plugins | Verified (seam shape); registration specified by verification; **changed by the owner** (two classes, core-required and optional, behind one door); a prototype (2026-09-26) confirmed the seam and corrected the provider signature, the registry's lifetime (per realm) and the conflict rules; cursor, text input and accessibility are not widget capabilities | [ADR-0084](../docs/adr/ADR-0084-open-capability-seam-and-plugins.md) |
| D4 | The reactive graph is realm-owned and read through `ReadScope` | Changed by verification and by owner decision O5; owner confirmed removing the `signals` feature; a prototype (2026-09-26) placed the read contract in `flui-foundation` and withdrew the `flui-reactive` extraction | [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md), [ADR-0086](../docs/adr/ADR-0086-signal-writes-through-event-context.md) |
| D5 | One raster contract in `flui-layer`; wgpu and CPU backends | Changed by verification (`RasterBackend` moves first; `RasterOwner` stays) | [ADR-0087](../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) |
| D6 | Retained layer identity drives damage | Verified; retained target made conditional | [ADR-0087](../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) |
| D7 | Where official packages live | Changed by owner decision O1 (one workspace, not a nested one) | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) |
| D8 | Workspace tiers, stability kinds, "three crates, N items" | Changed by verification and by O2/O3; owner confirmed deleting `flui-tree` and `flui-localizations`; ADR-0081 accepted in part on 2026-09-26 (tiers, order, direction rule, `edge-exceptions`, `tier-kind` declarations), checked beside the layers | [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md), [ADR-0089](../docs/adr/ADR-0089-upstream-types-in-stable-signatures.md) |
| D9 | One owner thread hosts isolated realms | Verified; the real bar to parallel layout named; **changed by the owner** (`!Send` flip before the first crates.io publication) | [ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) |
| D10 | IME talks to a pull text-store contract | Changed by owner decision O8 (read + edit + asynchronous lock) | [ADR-0090](../docs/adr/ADR-0090-ime-pull-text-store-contract.md) |
| D11 | `runtime-internals` becomes `#[doc(hidden)] __runtime` | Re-checked; not challenged by verification; scheduled as its own step in the migration plan | [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) |
| D12 | Text shapes per realm over Parley | Verified; gate 1 met by a prototype (2026-09-26): swash rasterizes, glifo not adopted; still Proposed | [ADR-0092](../docs/adr/ADR-0092-per-realm-text-over-parley.md) |
| D13 | One raster thread per `GpuContext` (revision of ADR-0045) | Changed by verification | [ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) |
| D14 | Router is the primary navigation API | Changed by verification (handle from `init_state`); **changed by the owner** (every push URL-addressable, dialogs and overlays excluded) | [ADR-0093](../docs/adr/ADR-0093-router-is-the-primary-navigation-api.md) |
| D15 | Hot reload through Subsecond behind a runtime hook | Changed by verification (no facade `hot-reload` feature); the Windows spike (2026-09-26) failed (stock dx cannot patch; app-crate statics and thread-locals break); the dlopen path stays until a later spike | [ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md) |
| D16 | `flui-protocol` is the typed schema shared by tests, devtools and agents | Changed by verification (amends ADR-0080 by settling its in-process transport; does not reverse it); accepted in part on 2026-09-26 (the crate, the lifted wire vocabulary and the moved semantics enums) | [ADR-0095](../docs/adr/ADR-0095-agent-protocol-schema-crate.md) |
| D17 | Packages build on `flui-sdk`; the facade names none of them | Changed by owner decision O6 (the reason is semver, not a cycle) | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) |
| G | Process-global state is gated; one trampoline cell | Changed by verification (scan every `static`, seed by scan); **accepted in part** 2026-09-26 (the gate and its seeded allowlist) | [ADR-0097](../docs/adr/ADR-0097-no-process-global-state-gate.md) |
| L | Dynamic linking for development builds | Studied after the review; measured on Windows; owner deferred it and asked for a build-footprint study | [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md) |
| O1 | Official packages are members of this workspace | Changed by verification | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) |
| O2 | `flui-sdk` is a separate Evolving crate, tied to the release train | Changed by verification | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md), [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) |
| O3 | raw-window-handle is the one named upstream exception | Changed by verification | [ADR-0089](../docs/adr/ADR-0089-upstream-types-in-stable-signatures.md) |
| O4 | Exit B0 is gates, structure and ratchets, not a crate count | Changed by verification | [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) §5; the one-transaction definition lives in [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) |
| O5 | The reactive core moves in three steps | Changed by verification; step 3's extraction withdrawn after a prototype | [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md) |
| O6 | The facade has no default design system | Changed by verification | [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) |
| O7 | Signal writes go through `EventCx` opened by a `WriterSource` | Changed by verification (verdict `holds: false`); owner chose the typed form, pilot widened to `counter` and `todo` | [ADR-0086](../docs/adr/ADR-0086-signal-writes-through-event-context.md) |
| O8 | Windows IME + Narrator is not an H0 gate; a text-store conformance kit is | Changed by verification |
| S1–S4, A1–A15, P1–P5 | The owner's strategy, architecture and process decisions of 2026-09-25 | Owner, 2026-09-25 | [below](#owner-decisions-of-2026-09-25) | [ADR-0090](../docs/adr/ADR-0090-ime-pull-text-store-contract.md) |

Rows G and L are not in the architecture report's D-table. G is its §8 globals gate, which needed
its own record; L comes from the report's dev-only `dynamic-linking` facade feature (§3.5 item 6),
which the review cited but never studied.

---

## Architecture decisions D1–D17

### D1. `flui-platform-api` is the contract crate

**Context.** One import, `use flui_platform::traits::PlatformTextInput;`
(`crates/flui-interaction/src/text_input.rs:27`), is the only `flui_platform` use in
`flui-interaction`'s sources, and it makes winit, tokio and the `windows` crate reachable from
interaction, rendering, objects, view, widgets and testing. Platform callbacks also require
`Send` (`crates/flui-platform/src/traits/platform.rs:319` takes `Box<dyn Fn() -> bool + Send>`),
which is why the `!Send` realm still lives in owner thread-local storage.

**Decision.** Split the traits into `flui-platform-api` (tier C, Stable, no OS types); the
backends stay in `flui-platform` (tier H). Two steps: first move the traits mechanically with a
re-export; then remove `Send` from the callbacks one backend at a time, each with a live-run
record, because only the Linux/headless path executes in CI.

**Alternatives rejected.** Backends inside `flui-app` (about 90k lines of unsafe OS code mixed
into the runtime). A crate per backend (premature before the backend-minting seam, #560).

**Evidence.** `cargo tree -i flui-platform -e normal` in the architecture report §1;
verification confirmed the split touches one production import.

**Changed by verification.** The first draft did both steps at once; the concurrency lens showed
the `Send` removal is per-backend work that CI cannot execute, so it became the second step and
the runtime extraction does not wait for it.

### D2. One frame transaction in `flui-runtime`

**Context.** Tests drive a second frame implementation: `HeadlessBinding::pump_frame`
(`crates/flui-testing/src/lib.rs:955`) does not go through the production transaction. The
runtime is spread through `flui-app`, whose `ui_realm` imports widget-layer scopes.

**Decision.** Extract `flui-runtime` in tier K, **above** `flui-widgets`. `flui-app` drives it
with the platform clock and raster lane, and `flui-testing` with a virtual clock. The second
implementation is deleted. "One transaction" is defined by
type: the phase entry points (`drive_frame_with_lane`, `crates/flui-scheduler/src/scheduler.rs:1874`,
and the scheduler's begin/draw handlers) become unreachable outside `flui-runtime`.

**Alternatives rejected.** The runtime below widgets (needs moves first); the runtime inside
`flui-view`. This supersedes ADR-0041's "No `flui-runtime` without two consumers"
([ADR-0041](../docs/adr/ADR-0041-workspace-topology-contract.md)): the headless driver is the
second driver of one transaction. It also amends ADR-0037 §12, which asks for two *production*
consumers: the runtime has one (`flui-app`) plus the test driver.

**Evidence.** `crates/flui-testing/src/lib.rs:955`, `:1270` (`pump_presentation`), `:1301`
(`pump_all`). Verification confirmed the runtime's position above widgets.

**Changed by verification.** Moving `flui-testing` above widgets would link a second copy of
widgets into its in-crate unit tests, so the 22 unit-test modules that use `crate::testing` move
to `crates/flui-widgets/tests/`. Owner decision O4 replaced "no `pub fn pump_frame`" with the
type-based definition above, because the old check could be bypassed by a rename.

### D3. An open, typed capability set

**Context.** ADR-0078 says a new capability is a method on `LifecycleContext`
(`docs/adr/ADR-0078-rules-live-in-types-and-lints.md:65`), and the trait is sealed, so a plugin
outside the repository cannot add one. The H1 exit asks for plugins outside the repository.

**Decision.** `LifecycleContext` gains one hidden, object-safe `capability_erased(TypeId)`; a
blanket extension trait gives the typed `cx.capability::<C>()`; plugins register providers
explicitly with `Application::plugin`, and an application can override a provider. Capabilities are still
acquired only in `init_state`/`did_change_dependencies`.

**Alternatives rejected.** A generic method on the trait itself (breaks object safety for the 136
`&dyn LifecycleContext` sites the review counted). Link-time registration through
`inventory`/`linkme` (caveats on wasm and static libraries).

**Evidence.** `crates/flui-view/src/context/build_context.rs:377` (`LifecycleContext: BuildContext`);
the clipboard accessor has no production caller (`crates/flui-app/src/app/runtime.rs:1632-1638`,
`expect(dead_code)`).

**Verified.** The extension-trait shape was confirmed; verification added the explicit
registration and the override hook, which the first draft left unspecified.

**Changed by the owner (2026-09-25).** Platform capabilities fall into two classes behind the one
door `cx.capability::<C>()`: core-required backend methods (clipboard and data transfer, text
input, accessibility, cursor, window chrome basics) and optional plugins with a typed
`Unsupported` (haptics, camera, geolocation, notifications, file dialogs). ADR-0084 §5 records the
classification rule and how a capability moves between classes; see [A8](#a8-capability-model).

**Changed by a prototype (2026-09-26).** A prototype of the seam compiled against the 136 sites
unchanged and served a capability to a crate outside the workspace; ADR-0084's Context records
what it showed and what it did not. The revision keeps the seam and corrects the rest: a provider
takes `&Arc<dyn PlatformWindow>` so a handle can keep a `Weak` window; the registry is a
parameter of each realm's construction, sharing one table validated by `Application::run` before
the platform starts, never an application-wide cell; a plugin over a built-in, two plugins on one
capability and one plugin registering twice are conflicts, reported in installation order as
`AppRunError::CapabilityConflict`; core `text_input()` and `accessibility()` return inert objects
instead of `Option`, and `accessibility()` sits on the backend extension trait of ADR-0082 §3.
Cursor, text input and accessibility are core-required backend methods but not widget
capabilities: the framework keeps its one route to each.

### D4. The reactive graph is realm-owned

**Context.** ADR-0074 says the graph is realm-scoped. In the code the graph is a field of each
presentation's `BuildOwner` (`crates/flui-view/src/owner/build_owner.rs:444`, exposed at `:973`),
and `UiCommand::SignalWrite` applied to the primary presentation's graph, so a write from
window B reached window A's graph and failed with `SignalError::ForeignGraph`
(`crates/flui-view/src/reactive/mod.rs`). Fixed: the command now carries its slot and is routed
by `SignalSlot::graph` to the owning presentation (ADR-0085 §1).

**Decision.** Each presentation keeps its own graph, owned by the realm with the presentation;
a cross-thread write is routed to the graph whose id the slot carries, never to the primary
presentation's. Reads go through a read-only `ReadScope`
trait that `BuildContext` extends. Readers are typed by frame phase (element, layout, paint).
Signals stop being a feature. `BuildContext::reactive()` (`build_context.rs:132`) is removed.
Writes are narrowed to callbacks (D4's write half is O7), and the ADR-0074 runtime guard stays
authoritative.

**Alternatives rejected.** The graph in `flui-foundation` (a warm edit re-checks 15 crates in
5.74 s there against 3 crates in 3.07 s in `flui-view`, and it puts build scheduling in the
lowest tier). A new `create(cx)` hook (merges `create_state` and `init_state`). The graph stays
in `flui-view`; the read contract is in foundation, so render and animation code can name
`Signal<T>` even though `Signal::get` is an inherent method today (`reactive/mod.rs:752`).

**Evidence.** Lines above; `SignalSlot` carries its graph id (`reactive/mod.rs:78-82`), which is
what routing by owner needs.

**Changed by verification and by O5.** The conformance defect is fixed first and separately,
with a multi-window test that fails today, not "by construction" through typed writes. The crate
extraction became three steps (O5).

**Changed by a prototype (2026-09-26).** A prototype moved the read vocabulary into
`flui_foundation::read_scope` (427 lines, no new dependency) and left the graph in `flui-view`,
with no change in `flui-widgets`, `flui-app`, `flui-testing`, the facade or the examples. Reads
take `&S where S: ReadScope + ?Sized`, which accepts every context shape that compiles today;
subscription goes through sinks the drivers mint, so the graph handle cannot subscribe anyone;
a handle of the wrong type is `SignalError::TypeMismatch`, not a `BUG:` panic. The warm edit it
measured (`cargo check -p flui-app`, one run: 15 crates in 5.74 s for the contract, 3 crates in
3.07 s for the graph) withdrew the `flui-reactive` extraction: the contract already lets render
code name `Signal<T>`, and a crate below `flui-rendering` and `flui-animation` would move the
graph's frequent edits from 3 re-checked crates to about 7 (inferred from `cargo tree -i`, not
measured).
ADR-0085 records the numbers and the holes the prototype opened.

### D5. One raster contract in `flui-layer`

**Context.** `crates/flui-engine/ARCHITECTURE.md:8-13` states that no other rasteriser is
planned and nothing exists to make one pluggable. There is no CPU golden path and no fallback
when no GPU is available.

**Decision.** The GPU-free lowering (layer walk, clip/opacity discipline, effect decomposition,
`CommandRenderer`, `LayerStateStack`) moves to `flui-layer` with a conformance suite. wgpu and a
CPU backend (`flui-engine-cpu`, `publish = false` until the golden API exists) implement it.

**Alternatives rejected.** Replacing wgpu with Vello; a CPU mode inside `flui-engine`.

**Evidence.** `RasterBackend` is already a GPU-free trait (`crates/flui-engine/src/raster.rs:100`).

**Changed by verification.** Moving `RasterOwner` into the runtime would pull wgpu into tier K.
So `RasterBackend`, `PresentDisposition` and a wgpu-free `RasterError` move to `flui-layer`
first, and `RasterOwner` stays in `flui-engine` through H0.

### D6. Retained layer identity drives damage

**Context.** Every frame is a full repaint: the lane always sends `DamageRegion::Full`
(`crates/flui-app/src/app/raster_lane.rs:291`), and `Full` is the only variant
(`crates/flui-layer/src/scene_snapshot.rs:18-21`). Partial repaint is a B2 exit item.

**Decision.** Each repaint boundary is an `Arc` subtree keyed by `RenderId`; a differ over
retained trees yields `DamageRegion::Partial` with a `Full` fallback. The contract closes before
H3; the work follows D1 and D2.

**Alternatives rejected.** Damage as the first breaking change in H0.

**Evidence.** Layer nodes already carry `render_id: Option<RenderId>`
(`crates/flui-layer/src/tree/layer_tree.rs:38`), stamped during paint
(`crates/flui-rendering/src/pipeline/owner/paint.rs:1186`). This means ADR-0061's "pairing
layers across frames — does not exist" (`docs/adr/ADR-0061-damage-needs-layer-identity.md:80`)
is now half out of date.

**Verified.** `DamageRegion::Full` and the ADR-0061 baseline were confirmed. Verification made
the retained render target conditional (render straight to the swapchain on full damage),
because a blit on tile-based mobile GPUs costs bandwidth (hypothesis, to be measured).

### D7. Where official packages live

**Context.** The owner plan said separate repositories on one release train; ADR-0028 keeps
Material and Cupertino inside the workspace.

**Decision.** Superseded by owner decision [O1](#o1-official-packages-are-members-of-this-workspace).
The review's proposal (a nested `packages/` workspace built against the published train) was not
adopted.

### D8. Workspace tiers and stability kinds

**Context.** Eleven numbered layers (`Cargo.toml:91-103`) did not catch the D1 leak. The first
draft claimed the semver promise "drops from 28 crates to 3".

**Decision.** Layers become tiers (V, C, S, R, K, H, packages) with a declared order inside each
tier, `tier-kind = stable | evolving | internal | official | tool`, and forbid-reach facts that
generalize the three hot-reload `cargo tree` facts (implemented in part: tiers, `order` and
`tier-kind` in `cargo xtask workspace`, the forbid-reach facts in `cargo xtask reach`; the kind
rules are not). Stable crates: `flui`,
`flui-platform-api`, `flui-protocol`; Evolving: `flui-sdk`. The frozen surface is "three crates,
N items", where N is the transitive closure of public types in the Stable modules, measured before
it is promised.

**Alternatives rejected.** About eight Stable crates; one crate.

**Evidence.** The allowed-dependents mechanism the tier kinds generalize
(`tools/xtask/src/workspace.rs:9-13`).

**Changed by verification.** "28 to 3" was relabelling; it became "three crates, N items", the
SDK became a separate Evolving crate (O2), and the upstream-type rule was restated by major-version
cadence with named exceptions (O3). ADR-0041 is superseded in part: its layer table goes, its
manifest-as-source rule and `allowed-dependents` mechanism stay.

### D9. One owner thread hosts isolated realms

**Context.** ADR-0027 says "realms may execute concurrently"
(`docs/adr/ADR-0027-owner-affine-ui-realms.md:18`). One thread-local `APP_RUNTIME`
(`crates/flui-app/src/app/runner/host.rs:25-47`) hosts every realm.

**Decision.** From H0 to H2 one owner thread hosts N isolated realms. Parallel layout inside a
realm is not a goal. Per-realm owner threads on Win32/Linux are a spike in H2.

**Alternatives rejected.** Per-realm threads now.

**Verified.** The single thread-local host was confirmed. Verification corrected the reason
parallel layout is excluded: it is `PipelineCell = Rc<RefCell<PipelineOwner>>`, not the render
object traits, which today carry `Send + Sync` bounds (`crates/flui-view/src/view/render.rs:451`).

**Changed by the owner (2026-09-25).** The `!Send` flip that ADR-0091 schedules lands before the
first crates.io publication, together with the callback signature change, instead of "no later
than the H3 freeze"; see [A3](#a3-the-send-flip-before-the-first-publication).

### D10. IME talks to a pull text-store contract

**Context.** `PlatformTextInput` only enables IME and places the candidate window
(`crates/flui-platform/src/traits/text_input.rs:24-45`). Win32 has no IME code. A Japanese IME
session is part of exit B1.

**Decision.** Superseded in its details by owner decision
[O8](#o8-windows-ime--narrator-is-not-an-h0-gate): the contract is read + edit + asynchronous
lock, and Windows uses TSF with UI Automation text patterns from the first day.

### D11. `runtime-internals` becomes a hidden module

**Context.** `flui-app` enables `flui-view/runtime-internals`
(`crates/flui-app/Cargo.toml:90`), so feature unification turns it on in every application: the
internal boundary is a visibility switch that is always on.

**Decision.** Replace the feature with `#[doc(hidden)] pub mod __runtime`.

**Alternatives rejected.** An `unstable` feature (the same unification problem).

**Evidence.** `crates/flui-app/Cargo.toml:90`; `crates/flui-view/Cargo.toml:118`. Verification
did not challenge this decision; ADR-0081 §4 owns it, and the migration plan schedules the
replacement as its own step, which also removes the feature's allowlist entry.

### D12. Text shapes per realm over Parley

**Context.** Text layout goes through one process-global `static FONT_SYSTEM`
(`crates/flui-painting/src/text_layout/layout.rs:124`), and glyph keys are cosmic-text's
process-global keys (`crates/flui-painting/src/text_layout/glyphs.rs:23`).
[ADR-0077](../docs/adr/ADR-0077-migrate-to-parley.md) proposed Parley on shaping evidence alone.

**Decision.** One ADR joins the Parley migration and per-realm fonts: a shared, immutable-after-load
font collection; per-realm contexts without a lock; atlas on the `GpuContext`; rasterization on the
raster side; neutral shaped runs in the display list instead of `Arc<TextLayout>`.

**Alternatives rejected.** A `flui-text` crate before a post-Parley measurement; OS text in
production.

**Verified.** `FONT_SYSTEM` was confirmed. ADR-0092 supersedes ADR-0077 and must carry its
rasterization precondition as a gate.

**Spike (2026-09-26).** The rasterization gate is met by a prototype: swash, driven directly,
rasterizes Parley-shaped glyphs into the unmodified atlas with a key on font blob identity; glifo
is not adopted (slower, experimental, vertical-only hinting, its own atlas). Keys stay stable only
while a raster-side registry holds each blob, since fontique's shared source cache holds blobs
weakly. FLUI adds no lock, but fontique locks internally on a local cache miss, shared across
realms. Two items stay open for the migration: a neutral run that does not carry
`parley::FontData`, and a door for feeding new faces to the atlas-owned rasterizer. ADR-0092
stays Proposed (its Context).

### D13. One raster thread per `GpuContext`

**Context.** [ADR-0045](../docs/adr/ADR-0045-raster-lane.md) is Proposed and describes GPU
services per owner thread. Today each window creates its own `wgpu::Instance`
(`crates/flui-engine/src/renderer.rs:1140`) and its own lane.

**Decision.** A revised ADR-0045, not a status flip: one `GpuContext` per application, one raster
thread per `GpuContext` serving every presentation, single-owner glyph and image atlases, and an
acceptance criterion per platform (Win32 and Linux threaded; macOS and wasm inline, with the
reason recorded).

**Alternatives rejected.** Accepting ADR-0045 as written; collapsing the mailbox.

**Changed by verification.** A shared atlas behind per-window threaded lanes was an unaccounted
lock. Per-window lanes became the alternative measured in the H2 spike. ADR-0045 itself rejected
"Share `GpuServices` process-wide" (`docs/adr/ADR-0045-raster-lane.md:309`); ADR-0091 must scope
the `GpuContext` as app-owned and injected, never global.

### D14. Router is the primary navigation API

**Context.** `Navigator` exposes about thirty push/pop variants and pages without URLs; commands
reach navigators through a thread-local `NAVIGATOR_COMMAND_TARGETS`
(`crates/flui-widgets/src/navigator/navigator.rs:91`).

**Decision.** Router first: derived routes, the URL as the source of truth, a handle to the
nearest ancestor Router acquired in `init_state`, `Navigator` frozen.

**Alternatives rejected.** Builder routes; named routes in the prelude.

**Changed by verification.** `Router::of(w)` cannot be implemented, because a `Writer` has no
position in the tree; the handle comes from `init_state` (Flutter's `Navigator.of(context)`
contract). The research synthesis also had "every push produces a URL-addressable entry; no
pageless routes", which the final report dropped without a reason; see
[open-questions.md](open-questions.md#15-pageless-routes).

**Changed by the owner (2026-09-25).** Adopted: every push is URL-addressable; dialogs and
overlays are excluded. ADR-0093 §2 records it.

### D15. Hot reload through Subsecond

**Context.** The dlopen design documents residual undefined-behaviour risk
([docs/hot-reload.md](../docs/hot-reload.md)). `flui-app` has an optional edge to
`flui-hot-reload` (`crates/flui-app/Cargo.toml:65,108`) and the facade a `hot-reload` feature
(`Cargo.toml:630`).

**Decision.** Rewrite hot reload on Subsecond as an official package behind a runtime
`DevReloadHook`. Delete the dlopen path only after a Subsecond spike. No core crate names the
package.

**Alternatives rejected.** Keeping dlopen; a facade `hot-reload` feature.

**Changed by verification.** The review first argued that the facade feature would form a Cargo
cycle; the probe reproduces a cycle (`cyclic package dependency`, exit 101) only when the package
depends on the crate that holds the optional edge, and `flui-hot-reload` depends on neither
`flui-app` nor the facade. The reason that stands is ADR-0088's: core must not name an official
package. The feature, the facade edges and the `flui-app → flui-hot-reload` edge are deleted in
the change that moves hot reload into packages.

**Spike (2026-09-26, Windows only).** It failed: stock `dx` cannot patch a FLUI app on Windows
x64 (its missing-symbol stub clobbers `__chkstk`'s argument), and with a patched `dx` a logic
edit keeps state but app-crate statics are zeroed per patch and app-crate thread-locals crash.
Patches reach only calls the hook wraps, so the hook sits at the element seam, not per frame.
ADR-0094 §5 now keeps the dlopen path on a platform where the spike fails, until a later spike
passes; macOS and Android were not run.

### D16. `flui-protocol` is the typed schema

**Context.** [ADR-0080](../docs/adr/ADR-0080-agent-protocol-desktop-contract.md) fixes MCP as the
transport and AccessKit names as the vocabulary, and leaves the in-process transport undecided.
`tools/desktop-mcp` keeps its own copy of the role enum.

**Decision.** ADR-0080 stays in force. `flui-protocol` is the typed schema for what MCP lacks
(query, action, outline, catalog, journal), shared by `flui-testing`, `flui-devtools` and
`flui-mcp`. Both backends are compared through a normalized outline projection.

**Alternatives rejected.** Our own protocol first with MCP as a projection (a reversal of
ADR-0080); MCP inside the application.

**Changed by verification.** The first draft reversed ADR-0080; it now complements it, and
"identical outlines" became a normalized projection, because UI Automation cannot express every
role.

### D17. Packages build on `flui-sdk`; the facade names none of them

**Context.** The review's deduplicated `cargo tree` counted 191 unique crates under `flui` against 127 under
`flui-material`: a package depending on the facade pulls in `flui-app`, `flui-engine`, wgpu and
naga.

**Decision.** `flui-sdk` is the host-free surface for package authors; the facade has no feature
that names an official package.

**Alternatives rejected.** A facade `runtime` feature with `default-features = false` (fragile;
any package can turn it back on).

**Changed by owner decision O6.** The reason is not the Cargo cycle (a cycle appears only with a
package → facade edge, which D17 already forbids); it is that a Stable crate cannot publicly
depend on an Evolving package, and publishing `flui` must not wait for one.

### G. Process-global state is gated

**Context.** The review found process-global state across the stack: `FONT_SYSTEM`
(`layout.rs:124`), `TIME_DILATION` (`crates/flui-scheduler/src/config.rs:43`), `APP_RUNTIME`
(`crates/flui-app/src/app/runner/host.rs:46`), `REQUEST_REBUILD` (`crates/flui-hot-reload/src/dispatch.rs:24`), `REGISTRY_STACK`
(`crates/flui-view/src/key/registry.rs:204`), `NAVIGATOR_COMMAND_TARGETS` (`navigator.rs:91`) and
more. They break multi-window isolation, determinism and Subsecond.

**Decision.** `cargo xtask globals` scans every `static` and `thread_local!` outside
`#[cfg(test)]`; its allowlist is seeded by the scan in the same change and can only shrink. The
one permanent exception class: OS-callback trampolines reach exactly one host cell.

**Changed by verification.** The first gate missed atomics such as `TIME_DILATION` and seeded
its list from prose; it now scans every `static` and seeds from the scan. The gate reintroduces a
scanner after `cf46dfe20` (#1283) deleted the old runtime-contract ratchet on the grounds that the
rules "are types and clippy lints now (ADR-0078)"; see
[open-questions.md](open-questions.md#12-new-gates-against-the-cf46dfe20-deletions).

**Accepted in part (2026-09-26).** `cargo xtask globals` and its `--self-test` run in
`cargo xtask checks`; each crate's `[package.metadata.flui] globals` holds the seeded entries.
Permanent state is a `grant` under ADR-0097 with a checked `class` (`trampoline`, `counter`,
`immutable`, `process`, `diagnostic`), which ADR-0078 §4 now admits beside an exit ADR; the
counter exemption is exact (`fetch_add` only, never `pub`), so the `try_update` ID counters are
`counter` grants. Removing each debt entry stays with its exit ADR.

### L. Dynamic linking for development builds

**Context.** The architecture report lists a dev-only `dynamic-linking` facade feature, and the
CHANGELOG names "Bevy-style `dynamic_linking` (`flui-dylib`)" as the next lever
(`CHANGELOG.md:1098-1104`). No crate, measurement or record existed.

**Decision.** Recorded in [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md); the study
and the Windows measurements are in [dynamic-linking.md](dynamic-linking.md). In short: an
app-crate edit rebuilds about 0.3–1.5 s faster, a framework-crate edit gets slower, and the
dylib's export count sits within about 1,200 of the 65,535 PE export limit with the default
feature set (the all-features build fails with `LNK1189`). Not a B-milestone item.

**Alternatives rejected.** A facade feature pulling a `flui-dylib` that depends on the facade
(a Cargo cycle: the dylib depends on the crate that names it; Bevy's shape has the dylib depend on the internal crates instead).

**Evidence.** Measured on the development host with rustc 1.98.1 and MSVC `link.exe`; the dev
profile builds FLUI crates at `opt-level = 1` (`Cargo.toml:779-780`), the level at which rustc
shares and exports generic instances.

**Owner (2026-09-25).** Deferred; ADR-0096 stays Proposed. The owner's problem is test and build
time, disk and memory growth, which dynamic linking does not address; a build-footprint study
takes it up with the CI redesign ([A14](#a14-dynamic-linking-deferred-build-footprint-study)).

---

## Owner decisions O1–O8

O1–O8 number the answers in the order of the owner-decisions report. Each carries the panel's
confidence. All eight were changed by their verifier.

### O1. Official packages are members of this workspace

**Context.** D7 proposed a nested `packages/` workspace built against the last published train;
the owner plan said separate repositories.

**Decision.** `packages/` is a directory of members of the **root** workspace (one lock file).
Packages move there in the same change that ports them to `flui-sdk`, not as a separate move.
`tier-kind = "official"` in `[package.metadata.flui]`. `cargo xtask workspace` generalizes the
existing allowed-dependents mechanism: official crates depend only on `flui-sdk`,
`flui-platform-api`, `flui-protocol` and declared official edges (an allowlist that only shrinks
until `flui-sdk` exists); no core crate names an official crate in any form, with named, dated
exceptions. `cargo package` of the SDK and the official packages runs on every PR to reproduce an
outside author's build. A package leaves for its own repository only on a recorded trigger
(cadence diverges for two trains, an external maintainer, or low core coupling).

**Alternatives rejected.** A nested workspace before anything is published (double compilation,
exact prerelease pins, a second root that change classification does not see). Separate
repositories now (two-way pins and a roller bot; Flutter consolidated its repositories in 2023
and 2024).

**Evidence.** The panel counted 68 of 123 commits touching the design-system crates also touch other crates. The
named exceptions: `crates/flui-testing/Cargo.toml:106` (dev edge to `flui-devtools`),
`crates/flui-app/Cargo.toml:65,108` (optional edge to `flui-hot-reload`), and the facade's
optional and dev edges to `flui-hot-reload`, `flui-material` and `flui-cupertino`
(`Cargo.toml:519,525-526,554`). `flui-cli`'s dev edge to `flui-hot-reload`
(`crates/flui-cli/Cargo.toml:114`) needs none: `flui-cli` has kind `tool`.

**Changed by verification** (confidence 0.82). The scheduled "strip path dependencies and build
against the published train" job was cancelled: with lockstep pins, `main` pins a dev version
that does not exist on crates.io. It was replaced by `cargo package` on every PR, which gives the
outside-author proof before the first publish.

### O2. `flui-sdk` is a separate Evolving crate

**Context.** The architecture report both made the SDK host-free and re-exported it from the
Stable facade, which contradicts itself.

**Decision.** A separate tier-K crate, host-free, versioned `0.N` and bumped on every train,
published in the same run as the train. Two parts: whole-module re-exports of the Stable closure
at the facade's paths (type identity preserved), and named Evolving modules (`pipeline`, `hooks`,
`gpu`). The facade does not re-export it. A one-train guard, `links = "flui_train"` on a
low crate everything on the train depends on, makes the resolver pick one train. If the Evolving
surface exceeds about 30 items beyond the hooks, the decision is reviewed.

**Alternatives rejected.** Everything in the Stable facade (freezes a dozen unproven internals).
An Evolving module behind a Cargo feature (feature unification leaks it). No SDK, with packages
pinning internal crates (every topology change breaks every package manifest).

**Evidence.** 127 against 191 crates; 172 exact `=0.2.0-dev` pins in all manifests (145 in
`crates/*/Cargo.toml` plus 27 in the root manifest; 14 in `flui-material`). A probe without the
`links` guard resolved an application and a package to two train copies and failed with E0308.

**Changed by verification** (confidence 0.80). The `links` guard was added, and the lockstep cost
for third-party packages was written down (see
[open-questions.md](open-questions.md#3-third-party-package-lag-under-a-lockstep-sdk)).

### O3. raw-window-handle is the one named upstream exception

**Context.** The Stable crates must not freeze on upstream releases: accesskit shipped 13
breaking releases between 2024-01 and 2026-09, and its `Role`/`Action` are not
`#[non_exhaustive]`.

**Decision.** Stable signatures carry only FLUI types, except raw-window-handle 0.6 through
`HasWindowHandle`/`HasDisplayHandle`/`HandleError`. serde and cursor-icon are allowed as 1.x
crates, not exceptions. Accessibility uses FLUI's own `#[non_exhaustive]` `SemanticsRole` and
`SemanticsAction`, named after AccessKit or ARIA where the concept exists, not a 1:1 mirror of
AccessKit's 182 roles. Input types are FLUI's own, with `NamedKey`/`Code` generated from
keyboard-types. Evolving escape modules exist only with a consumer; today that is `flui_sdk::gpu`.

**Alternatives rejected.** A 1:1 AccessKit mirror (inherits its churn). Versioned re-exports
(incompatible with an H3 freeze). A strict own vocabulary with no escape mechanism (leaves custom
widgets with roles outside the 33 stuck).

**Evidence.** Leaks to remove before a freeze: `pub use ::wgpu` (`crates/flui-engine/src/lib.rs:229`),
`pub use accesskit::{…}` (`crates/flui-testing/src/a11y.rs:36`), `pub use android_activity`
(`crates/flui-app/src/lib.rs:116`). The legacy `Window::raw_window_handle`
(`crates/flui-platform/src/window.rs:196`) returns a crate-local enum and is deleted with the
legacy window family (ADR-0082 §5); `PlatformWindow` already uses the borrowed-handle traits.

**Changed by verification** (confidence 0.75). `#[non_exhaustive]` removes the compiler's
exhaustiveness pin on the outgoing mapping, so the pin is a test over a generated `ALL` list, and
an exhaustive match is kept only on the incoming `accesskit::Action`/`ActionData` side.

### O4. Exit B0 is gates, structure and ratchets

**Context.** The roadmap's exit B0 named a green `just ci` (there is no justfile since
`cf46dfe20`), a crate count of 26 and a file-length limit.

**Decision.** Four classes. Gates wired and able to fail, each with `--self-test`. Structural
invariants green: the tier gate; `cargo xtask reach` over the resolved graph for every facade
feature combination, with the tier-K forbid set `flui-platform`, `winit`, `android-activity`,
`ndk`, `windows`, `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine`, `flui-app`; a
module-DAG gate for `flui-widgets`; and, if the runtime extraction stays in B0, the one-transaction
definition from D2. Debt frozen by ratchets seeded by scans. Hygiene: `cargo xtask ci` green on
macOS, version `0.2.0`. The crate count is a reported fact, not a target.

**Alternatives rejected.** Only forbidding wgpu (already zero today, so it proves nothing);
driving every count to zero in B0; keeping the crate count.

**Evidence.** At review the K set was red only through two direct edges to `flui-platform`:
from `flui-interaction`, and from `flui-widgets` under its `testing` feature (turned on by the
facade's `testing` feature). D1's first step moved both to `flui-platform-api` before
`cargo xtask reach` landed, so the gate's first run needed no `flui-platform` entry. `jni` reaches tier K through `reqwest →
rustls-platform-verifier` under `network-images`, which is why generic FFI crates are allowlisted
with reasons rather than forbidden.

**Changed by verification** (confidence 0.80). The K forbid set was corrected (the first one
stayed red after D1), "one transaction" moved to a type-based definition, and reach is computed
on `cargo metadata` because `cargo tree -i` fails on absent packages and multiple versions.
The panel thought no ADR was needed; the criterion is recorded in
[ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) §5 anyway, because it
replaces the roadmap's exit text with gates that record defines, and the
[migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md) maps it to steps.

### O5. The reactive core moves in three steps

**Context.** The graph is one file, `crates/flui-view/src/reactive/mod.rs`, and depends only on
`flui_foundation` identifiers, smallvec, thiserror and two `flui-view` items. D4 placed it in a
new `flui-reactive` crate in tier V.

**Decision.** Step 1: `Signal::get` takes `&S where S: ReadScope + ?Sized`, read-only, with the
read contract in `flui_foundation::read_scope` and the graph in `flui-view`;
`BuildContext: ReadScope`; non-Clone element and render drivers; signals stop being a feature.
Step 2a: readers become `Element | Layout | Paint`, with guards against writes during layout or
paint. Step 2b: the first render subscriber, with a test that shows the repaint, and no move. If
no render consumer exists by then, the render-phase readers and `RenderDriver` are removed
rather than kept unwired.

**Alternatives rejected.** The crate now (no second consumer). A module in `flui-foundation`
(15 crates re-checked in 5.74 s per edit against 3 in 3.07 s in `flui-view`, measured). Staying
in `flui-view` with an erased render trait (two observation systems forever).

**Evidence.** Readers are hard-wired to `ElementId` today; `ScrollPosition` is not a first
consumer because it is written inside `perform_layout`.

**Changed by verification** (confidence 0.75). Two steps became three, one driver became two, the
first consumer changed, and the removal of the `signals` feature was ordered. Its stated
precondition, "the ADR-0074 go/no-go before the #1090 field-mask registry"
(`Cargo.toml:642-645`), is met: #1090 landed as `588251a1c`, `depend_on_field` ships
(`crates/flui-view/src/context/build_context.rs:648`), and ADR-0074 §8.1 records the measurement.
See [open-questions.md](open-questions.md#14-is-the-signals-precondition-already-met).

**Owner (2026-09-25).** Confirmed: the `signals` feature is removed, and ADR-0085 §5 states the
preconditions as met, with the evidence ([A12](#a12-the-signals-feature-is-removed)).

**Changed by a prototype (2026-09-26).** A prototype of step 1 put the read contract in
`flui-foundation` and measured the warm edit: 15 crates in 5.74 s for an edit to the contract,
3 crates in 3.07 s for one to the graph in `flui-view`. Since the contract already lets render
and animation code name `Signal<T>`, the move to `flui-reactive` is withdrawn; the graph stays in
`flui-view`, and ADR-0085 §6 records the reasoning and the trigger for reopening it.

**Changed by the implementation of step 1 (2026-09-26).** Step 1 shipped the contract,
`BuildContext: ReadScope`, the sealed `SignalWriteExt`, `TypeMismatch` and the feature removal.
The two drivers, `RebuildSink`, `ScopeRef::detached` and `SignalError::NoGraph` moved to step 2a,
because in step 1 no production caller would reach them; until then `make_build_ctx` mints the
element sink. ADR-0085 §2 and §6 say the same.

### O6. The facade has no default design system

**Context.** The facade has `default = ["material"]` (`Cargo.toml:598`) and a `material` feature
(`Cargo.toml:605`); the README promises Material-first.

**Decision.** `flui` gets `default = []` and no `material`, `cupertino`, `devtools` or
`hot-reload` features; `flui create` adds `flui-material` explicitly, and `flui-material` gains a `prelude`.
The facade loses the feature only once `flui-material` can be added directly from a project
outside the workspace, that is, after it builds on `flui-sdk`.

**Alternatives rejected.** Keeping the default (removing it after a Stable release breaks semver).
Renaming (gives the most visible name to a volatile crate). A `flui-kit` meta-crate (deferred
until two-line setup is shown to hurt).

**Evidence.** The workflow line that builds with the feature: `.github/workflows/ci.yml:924`
(`cargo build -p flui --features material --example sliver_demo --locked`).

**Changed by verification** (confidence 0.82). "Independent cadence" was withdrawn: with exact
pins a new `flui` and an old `flui-material` do not resolve together, so Material ships in the
same run as the train. The workflow line was added to the change list.

### O7. Signal writes go through `EventCx`

**Context.** `Signal::set(self, r: &Reactive, value)` (`crates/flui-view/src/reactive/mod.rs:774`)
accepts any `Reactive`, and any context hands one out; only the run-time guard rejects a write in
`build`. The review counted 92 public `on_*` setters in the catalogs.

**Decision.** Framework-issued event callbacks receive `&mut EventCx<'_>` (borrowed, per dispatch,
`Deref<Target = Writer>`). A `WriterSource` acquired from `LifecycleContext` is the single way to
open one, used by catalog widgets, third-party widgets and the internal continuation path. The
gesture arena is unchanged: widgets wrap its `Rc` callbacks. Listener, animation-status and
post-frame callbacks get `cx` only in the change that removes their `Send`. `StateCell` and
`RebuildHandle` stay runtime-tier under the guard. The guard stays authoritative; `Writer` narrows
it. The steps: fix the signal-write routing defect (D4); types and `WriterSource`; new `set`/`update`
signatures and removal of `reactive()`; a `flui migrate` codemod crate by crate, piloted on
`flui-cupertino`. Rollback to the guard-only option is triggered if the pilot's HRTB friction is
clearly worse than the probe.

**Alternatives rejected.** An ambient scope (same wiring, fails at run time, adds thread-local
state). Guard-only (cheap now, expensive after 1.0). A separate public escape hatch for foreign
`Fn()` (no compilable uses before the `!Send` flip).

**Evidence.** Today's `Send + Sync` callbacks: `ListenerCallback`
(`crates/flui-foundation/src/notifier.rs:46`), `Listenable: Send + Sync` (`notifier.rs:78`).

**Changed by verification** (confidence 0.62; verifier verdict `holds: false`). The arena is no
longer rewritten; the routing defect is fixed separately and earlier instead of "by construction";
the separate hatch was dropped; `StateCell` stays runtime-tier. Two of the four arguments for
option A did not survive; the remaining one is that a write in `build` becomes a compile error.

**Changed by the owner (2026-09-25).** The owner chose the typed form, with the pilot widened to
the `counter` and `todo` examples beside `flui-cupertino` and the rollback trigger kept
([A1](#a1-typed-signal-writes)).

### O8. Windows IME + Narrator is not an H0 gate

**Context.** A grep for `WM_IME`, `ITextStore`, `ITfThreadMgr` and `Imm` under `crates/` finds
nothing; `docs/BETA.md:122` lists Windows as experimental with no IME.

**Decision.** The H0 gate is a public, headless text-store conformance kit that the built-in text
field and third-party widgets run in CI. The contract is **read + edit + lock**: UTF-16 offsets,
write verbs, and a lock that may be granted later (`TS_S_ASYNC`). An automated Windows lane runs
the UI Automation device check with a pre-check that separates host limits from regressions. The
live Japanese IME + Narrator session stays in exit B1, recorded in `docs/evidence/windows.toml`;
a record is fresh when its commit is an ancestor of the release and no trigger path changed in
between.

**Alternatives rejected.** A hard H0 gate (impossible today, and a human session cannot be a merge
gate). Exit B1 unchanged (the contract stays unchecked). Freshness under 30 days (about twelve
manual sessions a year).

**Changed by verification** (confidence 0.80). The kit would have tested a read-only contract
that TSF cannot use; it was extended to edit and asynchronous lock. The freshness rule was made
non-circular.

---

## Owner decisions of 2026-09-25

The owner answered every item of [open-questions.md](open-questions.md) in an interview on
2026-09-25 and set the product strategy. `#N` refers to the item of that page. "Confirmed" means
the recommended default stands; "changed" means the owner decided otherwise, and the entry says
how.

### Strategy

#### S1. Positioning

FLUI is a UI runtime trusted by people and agents: deterministic frames, realms without process
globals, one protocol shared by tests, devtools and agents
([ADR-0095](../docs/adr/ADR-0095-agent-protocol-schema-crate.md)), and generative UI through
A2UI. The Flutter model stays as the familiar shape, not the headline promise. Recorded in
[architecture.md](architecture.md).

#### S2. Notes is the hero application

Notes is the beta reference application, as the plan has it: signals, Router and Form, built from
crates.io by a clean consumer.

#### S3. Platform order

Live evidence comes platform by platform in the order Windows, macOS, Linux, web. The platform slot
of the migration plan follows it.

#### S4. Web in the H0 exit

Rendering and pointer input only, stated explicitly in the exit. Web text entry and IME, and the
DOM/ARIA mirror, move to H1 as items of their own (#17, confirmed).

### Architecture

#### A1. Typed signal writes

`&mut EventCx<'_>` on framework event callbacks through `WriterSource`, with the pilot and the
recorded rollback trigger to guard-only (#4). The pilot covers `flui-cupertino` and the `counter`
and `todo` examples. ADR-0086 records the choice and the lineage: the removed `flui-reactivity`
crate had a context-free `set` on a process-global runtime, ADR-0074 moved to realm-scoped `Copy`
handles, and typed writes are the next step on that line. Confirmed, pilot widened.

#### A2. Runtime extraction in B0

`flui-runtime` is extracted in B0 (#1), and the GlobalKey-under-lock and `ElementCore.depth` fixes
ship with it. [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md).
Confirmed.

#### A3. The `!Send` flip before the first publication

The UI callback surface loses `Send + Sync` before the first crates.io publication, together with
the callback signature change of ADR-0086 (#8).
[ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §1. **Changed**:
the default was "no later than the H3 freeze".

#### A4. `flui-sdk` cadence

`0.N` is bumped on every train (#3), as
[ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) is written. Confirmed.

#### A5. `flui-tree` and `flui-localizations` are deleted

Both crates are deleted (#10); [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)
records it in its tier table. Confirmed.

#### A6. Every push is URL-addressable

Every push is URL-addressable; dialogs and overlays are excluded (#15).
[ADR-0093](../docs/adr/ADR-0093-router-is-the-primary-navigation-api.md) §2. Confirmed.

#### A7. Copy and paste with Form in B1

Copy and paste in text fields land in B1 together with Form, not in B2 (#16). The GlobalKey and
`depth` fixes stay with the runtime extraction (B0); path clips stay with the raster contract.
**Changed**: the default put copy and paste in B2.

#### A8. Capability model

Two classes of platform capability behind one door, `cx.capability::<C>()` (#11):

- **core-required** — methods of the backend traits; a backend without them does not compile:
  clipboard and data transfer, text input and IME, accessibility, cursor, window chrome basics;
- **optional** — plugin capabilities through the registry with a typed `Unsupported`: haptics,
  camera, geolocation, notifications, and file dialogs unless they become core.

[ADR-0084](../docs/adr/ADR-0084-open-capability-seam-and-plugins.md) §5 records the rule for which
class a capability belongs to and how one moves between classes. **Changed**: the default only kept
clipboard core-required; the owner generalised it into a model.

Core-required is a backend obligation; clipboard and data transfer are the only core capabilities
reachable through `cx.capability::<C>()`. Text input, accessibility and the cursor stay on the
framework's own routes (ADR-0084 §5).

#### A9. Escape modules on demand

Versioned upstream escape modules exist only with a consumer, except `flui_sdk::gpu` (#2).
[ADR-0089](../docs/adr/ADR-0089-upstream-types-in-stable-signatures.md). Confirmed.

#### A10. `LayoutCallbackScope` after a spike

A three-day spike first, then a decision (#9);
[ADR-0017](../docs/adr/ADR-0017-build-during-layout-callback-seam.md) stays in force until then.
Confirmed. Spike ran 2026-09-26; ADR-0017 stays (its Revisited section names the conditions for a
follow-up spike).

#### A11. New scanner gates amend ADR-0078

The first new gate's change amends
[ADR-0078](../docs/adr/ADR-0078-rules-live-in-types-and-lints.md) explicitly (#12). Confirmed.

#### A12. The `signals` feature is removed

The feature goes; [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md)
§5 states the go/no-go preconditions as met, with the evidence (#1090 landed as `588251a1c` in
PR #1260; `depend_on_field`; ADR-0074 §8.1) (#14). Confirmed.

#### A13. ADR back-links at acceptance, no symmetry gate

Back-links on the older ADRs are added only in the PR that accepts each new ADR, and no gate checks
their symmetry (#13). The already-missing lines on ADR-0065 and ADR-0016 may be fixed at acceptance
time too; no older ADR is edited now. **Changed**: the default added the missing lines at once.

#### A14. Dynamic linking deferred, build-footprint study

Not now; [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md) stays Proposed as deferred
(#20). The owner's actual problem is test and build time, disk use and memory growth, which dynamic
linking does not address. A build-footprint study, done with the CI redesign, takes it up
([open-questions.md](open-questions.md#build-footprint-study)). Confirmed, study added.

#### A15. crates.io names

Each name is checked before its crate is created; nothing is reserved now (#7). Confirmed.

### Process

#### P1. CI redesign instead of piecemeal workflow edits

No workflow change lands piecemeal (#5, #19). CI is rebuilt as one design: a fast PR lane, full
runs only when needed, no platform-specific heavy runs during active work. The individual needs
(macOS `cargo xtask ci`, the perf job, the Windows UI Automation step, the `windows-latest`
matrix, the facade build without Material) are inputs to it
([open-questions.md](open-questions.md#ci-redesign)). **Changed**: the default was two `full-ci`
PRs.

#### P2. ja-JP on the development host

The owner enables ja-JP before the IME work (#6). Confirmed.

#### P3. dlopen hot-reload hazards

Run the Windows repro once and document both hazards in `flui-hot-reload`'s crate docs until
Subsecond replaces the path (#21). Confirmed.

#### P4. The repository is the living plan

`design/` and `docs/plans/` are the source of truth; the owner's own document is a mirror and
journal that points to the repository (#18). `docs/ROADMAP.md` says so. Confirmed.

#### P5. Unverified claims

Unchanged: each claim in open-questions item 22 is checked before its ADR is accepted (#22).

---

## Corrections to the reports found after the panel

Re-checking the reports against the worktree found these; the ADRs should state the corrected
form.

- `crates/flui-app/src/app/runner/realm_dispatch.rs` is 7,149 lines, but the test module starts at
  line 1692 (`mod realm_dispatch_tests`); production code is about 1,690 lines. The file-length
  gate counts production lines only, so the file is already within the limit and needs no move;
  it does not need to be "dissolved" either. The one file over the limit is
  `crates/flui-scheduler/src/scheduler.rs`; its current count is its entry in
  `tools/xtask/allowlists/file-length.toml`.
- The pin count is 145 in `crates/*/Cargo.toml` and 172 including the root manifest. Both reports
  are right for their scope; a record must state which scope it counts.
- The runtime-contract ratchet did not "vanish": `cf46dfe20` (#1283) deleted it on purpose,
  together with the publish dry-run and the panic allowlist.
- The rebuild-weight figures (engine 74.0k lines, widgets 82.7k) include test code; `flui-engine`
  carries at least 19k lines in test files alone. The reactive warm-edit cost is measured, not
  estimated: 15 crates in 5.74 s for an edit to a foundation module against 3 crates in 3.07 s
  for one to the graph in `flui-view` (`cargo check -p flui-app`, one run; ADR-0085, Context).
  That is cargo's re-check set for one build, not the `cargo tree -i` count of all dependents
  behind the earlier "27 against 16".
