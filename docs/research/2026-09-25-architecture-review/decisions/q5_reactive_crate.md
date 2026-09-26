# Decision panel: q5_reactive_crate

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "The reactive core is one file, crates/flui-view/src/reactive/mod.rs (1159 lines, of which lines 840-1159 are tests). It imports only std, `flui_foundation::{ElementId, RebuildReason}` (:65), smallvec (:66), thiserror (derive at :85) and two items inside flui-view: `crate::owner::ExternalBuildScheduler` (:68) and `crate::BuildContext` (:711, :725, :739, :752). So the graph is almost free-standing. Its only real coupling to flui-view is (a) the scheduler sink and (b) the read context.",
    "`Signal::get/with/try_with/try_get` take `&dyn crate::BuildContext` (reactive/mod.rs:709-760). They call exactly two context methods, `cx.reactive()` and `cx.signal_read(slot)` (:715-717). Both are declared on BuildContext behind `#[cfg(feature = \"signals\")]` (crates/flui-view/src/context/build_context.rs:132,137). These two methods are the whole `ReadScope` surface: `trait ReadScope { fn reactive(&self) -> Reactive; fn signal_read(&self, slot: SignalSlot); }`.",
    "`ExternalBuildScheduler` is `pub(crate)` in flui-view (crates/flui-view/src/owner/build_owner.rs:80). It wraps `Arc<Mutex<HashMap<ElementId, RebuildReasons>>>` plus an optional frame-request closure. Reactive uses only `.schedule(ElementId, RebuildReason)` (reactive/mod.rs:639-641), so a one-method `RebuildSink` trait replaces it.",
    "Moving the graph to its own crate makes this element-lifecycle surface public, because flui-view calls it today as `pub(crate)`: `set_scheduler` (:217), `begin_element_build` (:373), `end_element_build` (:391), `register_element_reader` (:429), `release_element` (:448). The probe build warned about them (5 unused items), which confirms they are only reached from flui-view. In flui-reactive they would need `#[doc(hidden)] pub` or a sealed hook trait. This is a real cost of a split that D4 does not record.",
    "Readers today are `ElementId` only (e.g. `readers_of -> Vec<ElementId>`, :469; `SlotInfo.readers`, :190). There is no subscriber-kind enum yet. The Layout(RenderId)/Paint(RenderId) subscribers in D4 are new work. `RenderId`, `ElementId` and `RebuildReason` all already live in flui-foundation (crates/flui-foundation/src/id.rs:9,27; rebuild_reason.rs:18), so a crate below view and rendering can name every subscriber id without a new edge.",
    "`Signal<T>` is `!Send + !Sync` through `_local: PhantomData<*const ()>` (reactive/mod.rs:653). `CustomPainter: Send + Sync + Debug` (crates/flui-rendering/src/delegates/custom_painter.rs:105) and `Animation<T>: Listenable + Send + Sync` (crates/flui-animation/src/animation.rs:68). So a painter or an Animation cannot hold a `Signal<T>` directly. Render-phase subscription has to go through RenderObject state (`RenderObject<P>` has no Send bound, crates/flui-rendering/src/traits/render_object.rs:178), or those Send bounds must be relaxed. Whichever crate hosts the graph, this constraint stays.",
    "flui-foundation already hosts the observable layer: notifier.rs (1042 lines), notifier_generic.rs (447), listener_registry.rs (523), observe.rs (251), with re-exports at crates/flui-foundation/src/lib.rs:269-273. Rendering consumes it for render-phase invalidation today: the `CustomPainter::repaint -> Option<Arc<dyn Listenable>>` hook (custom_painter.rs:140) and `ScrollPosition` as a Listenable (crates/flui-rendering/src/view/scroll_position.rs:27,322,678). D4 turns Listenable into an Rc adapter over the graph. If the graph sits in flui-reactive above foundation, that adapter cannot live in foundation. Either the Listenable family moves up into flui-reactive, or foundation keeps a parallel Arc/Mutex notifier.",
    "flui-foundation's dependencies today are parking_lot, smallvec, tracing and web-time (`cargo tree -p flui-foundation -e normal --depth 1`). Folding reactive in would add thiserror, or hand-written Error impls, to foundation.",
    "Command: `cargo tree -i flui-foundation --workspace -e normal --depth 1 --prefix none --offline`. Result: 20 direct framework dependents (animation, app, cupertino, devtools, engine, hot-reload, interaction, layer, material, objects, painting, platform, rendering, scheduler, semantics, testing, tree, view, widgets, plus the flui facade). D4's rejected-alternative cell says 'фан-аут 17 крейтов'. That figure is stale or understated; the measured number is 20 direct and 26 transitive, counting example crates.",
    "Transitive reverse dependents, counted with `cargo tree -i <crate> --workspace -e normal --prefix none`, including self and example crates: flui-foundation 27, flui-scheduler 17, flui-rendering 14, flui-animation 14, flui-view 12. The union of the rendering, animation and view reverse sets (what a flui-reactive crate depended on by those three would invalidate) is 15. Relative to foundation, placing the core in flui-reactive stops a reactive edit from rebuilding flui-engine, flui-platform, flui-interaction, flui-scheduler, flui-semantics, flui-layer, flui-painting, flui-tree, flui-devtools, flui-web-demo and flui-painting-demo.",
    "Size of the crates flui-reactive would spare from a rebuild, counted in src lines (`find <crate>/src -name '*.rs' | xargs cat | wc -l`): engine 72,479; platform 46,302; interaction 40,264; scheduler 20,644; semantics 10,914; painting 7,019; tree 6,900; layer 5,027; devtools 2,487. Total about 212k lines. What a flui-reactive edit still rebuilds: animation 16,241; rendering 55,791; objects 38,329; view 52,706; widgets 82,112; app 52,092; testing 3,679; material 26,910; cupertino 4,330; localizations 281; hot-reload 2,856. Total about 335k lines. Estimate, not a measurement: with the core in foundation, each reactive-only edit rebuilds about 63% more source (547k vs 335k lines), and flui-engine (wgpu) and flui-platform join the critical path. Staying in flui-view rebuilds the least (view and above, about 12 crates) but cannot serve rendering or animation subscribers.",
    "Churn, from `git log`: crates/flui-view/src/reactive has 8 commits in its whole history, all between 2026-09-22 and 2026-09-23, so it is new code. Computed/Effect are deferred to ADR-0075 and Store/Writer/Bind are planned in W3, so heavy churn is expected. crates/flui-foundation already has 40 commits since 2026-07-27, so foundation edits already invalidate everything often. The marginal cost of folding reactive in comes only from reactive-only edits during W3.",
    "The only consumers of the reactive module outside its own file are inside flui-view (build_context.rs, element_build_context.rs, owner/build_owner.rs, owner/element_owner.rs, lib.rs). Found with `grep -rln 'reactive::\\|Signal<\\|flui_view::reactive' crates/*/src src`. flui-widgets touches signals only in tests and benches (crates/flui-widgets/tests/main.rs:125-130, benches/signals_rebuilds.rs). The facade feature `signals` is off by default (Cargo.toml:642-645). The root manifest note Cargo.toml:71-77 ('deliberately NO signals crate') would have to be rewritten if flui-reactive is created. D4 already lists this.",
    "Compile cost of the crate itself is negligible either way. Probe command: `cargo check -p probe-reactive --timings`, cold. flui-foundation took 0.32s and the moved reactive module 0.16s (scratchpad/probe-reactive/target/cargo-timings). Any build-time difference between the options therefore comes entirely from dependent invalidation, not from the reactive crate's own compile time. So D4's fold-back criterion ('if cargo build --timings shows no difference') should be defined as a warm-edit rebuild of the dependents (edit mod.rs, then time `cargo check -p flui-app`), not as a comparison of the crate's own unit time.",
    "The workspace pins toolchain 1.98.1 (rust-toolchain.toml; `rustc 1.98.1`). Trait-object upcasting coercion is stable since Rust 1.86, so `&dyn BuildContext -> &dyn ReadScope` coerces implicitly at call sites. The experiment confirmed this."
  ],
  "market_precedents": [
    {
      "who": "Leptos",
      "what": "The reactive core is its own crate, `reactive_graph` ('A fine-grained reactive graph for building user interfaces'). It depends only on generic crates (slotmap, rustc-hash, thiserror, tracing, any_spawner, or_poisoned, ...) and on nothing from leptos. The renderer `tachys` consumes it as an optional dependency: `reactive_graph = [\"dep:reactive_graph\", \"dep:any_spawner\"]`.",
      "outcome_or_lesson": "This is the closest precedent for D4. The graph and Signal types sit below both the view layer and the rendering layer, so the renderer subscribes without an upward edge. Leptos kept it separate so that renderers and other frameworks can reuse it. That is the 'second consumer' P8 asks for, and here the second consumer is flui-rendering/animation.",
      "source": "https://raw.githubusercontent.com/leptos-rs/leptos/main/reactive_graph/Cargo.toml ; https://raw.githubusercontent.com/leptos-rs/leptos/main/tachys/Cargo.toml"
    },
    {
      "who": "Dioxus",
      "what": "The split runs in the opposite direction. `generational-box` (the Copy-handle arena) is its own bottom crate. `dioxus-signals` depends on both `dioxus-core` and `generational-box`. The subscriber context (`ReactiveContext`: a thread-local current-subscriber stack, `mark_dirty` into the VirtualDom scheduler) lives in dioxus-core.",
      "outcome_or_lesson": "This corresponds to option 3 ('stay above core, core exposes a subscription trait'). It works for Dioxus because there is no separate retained render tree that must subscribe. The only subscriber is a component scope. FLUI needs Layout/Paint subscribers in flui-rendering, which cannot depend on flui-view, so the Dioxus shape does not transfer unless render-phase reads go through type-erased adapters. The reusable idea is the storage/handle split (generational-box) as its own tiny crate.",
      "source": "https://raw.githubusercontent.com/DioxusLabs/dioxus/main/packages/signals/Cargo.toml ; https://docs.rs/dioxus-core ; https://deepwiki.com/DioxusLabs/dioxus/3-reactivity-and-state-management"
    },
    {
      "who": "Sycamore",
      "what": "`sycamore-reactive` is a separate crate ('Reactive primitives for Sycamore') whose dependencies are only paste, slotmap and smallvec, plus optional serde and wasm-bindgen.",
      "outcome_or_lesson": "A signals graph can be a leaf crate with almost no dependencies. FLUI's module already fits that shape: std + foundation ids + smallvec + thiserror.",
      "source": "https://raw.githubusercontent.com/sycamore-rs/sycamore/main/packages/sycamore-reactive/Cargo.toml"
    },
    {
      "who": "Floem (Lapce)",
      "what": "The workspace member `reactive` is published as `floem_reactive` (`floem_reactive = { path = \"reactive\", version = \"0.2.0\" }`) and sits separately from the renderer crates (vello, skia, tiny_skia, vger).",
      "outcome_or_lesson": "A Rust native desktop UI with retained widgets also chose a standalone reactive crate below the UI crate, with signals read directly in widget update and paint paths.",
      "source": "https://raw.githubusercontent.com/lapce/floem/main/Cargo.toml"
    },
    {
      "who": "Xilem / Masonry",
      "what": "The workspace has 23 members (xilem, xilem_core, xilem_masonry, masonry, masonry_core, masonry_imaging, masonry_testing, masonry_winit, tree_arena, ...). There is no signals or reactive crate: state flows through the Elm-style app-state rebuild in xilem_core. They do extract small substrate crates (tree_arena) and split a *_core from the backend.",
      "outcome_or_lesson": "Xilem is evidence that splits follow a genuine seam (core vs backend, arena vs framework), not per-feature crates. It has no precedent for render-phase signal subscribers.",
      "source": "https://raw.githubusercontent.com/linebender/xilem/main/Cargo.toml"
    },
    {
      "who": "Bevy",
      "what": "Change detection (Ref/Mut ticks) is built into `bevy_ecs`, whose internal dependencies are bevy_ptr, bevy_reflect, bevy_tasks, bevy_utils, bevy_ecs_macros and bevy_platform. There is no separate reactive crate.",
      "outcome_or_lesson": "Bevy is the fold-into-the-core option: change tracking sits in the widely depended-on core crate, which every ECS edit rebuilds. Bevy accepts that because change ticks are part of the ECS data model itself. In FLUI, flui-foundation is a grab-bag of values and ids, not the graph owner. That argues against folding the young, high-churn graph into it before it stabilises.",
      "source": "https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ecs/Cargo.toml"
    },
    {
      "who": "Slint, GPUI (hypothesis, from memory, not re-checked this run; .gpui clone absent)",
      "what": "Slint keeps its reactive Property/binding machinery as a module inside i-slint-core. GPUI keeps Entity/observe/notify inside the single `gpui` crate.",
      "outcome_or_lesson": "Both are monolithic-core precedents: reactivity as a module of the core crate. They support 'module in foundation' as a legitimate end state once the API is stable, which is D4's stated fold-back.",
      "source": "not fetched; verify at https://github.com/slint-ui/slint/tree/master/internal/core and https://github.com/zed-industries/zed/tree/main/crates/gpui"
    }
  ],
  "constraints": [
    "Inherent-method rule: `count.get(cx)` must stay an inherent method on `Signal<T>`, and Rust allows inherent impls only in the crate that defines the type. So the crate hosting the graph must also host the `Signal<T>` handle, `SignalSlot`, `SignalError`, `SignalSender` and the `ReadScope` trait. Moving only the core, with the handle left in flui-view, leaves flui-rendering unable to name `Signal<T>`. This confirms D4 §4.6 and the finding the doc records as fixed ('Исправлено').",
    "Option 3 (graph stays in flui-view, rendering subscribes through a trait in foundation) fails the render-phase requirement in its typed form. flui-rendering cannot depend on flui-view (layering), so it cannot hold or name `Signal<T>`. It could only consume a type-erased adapter such as an `Rc<dyn Readable<T>>` trait object defined in foundation, which is exactly today's Listenable path. That keeps two observable systems (graph plus Listenable) instead of D4's single graph with a Listenable adapter.",
    "Option 2 (module inside flui-foundation) works technically: every subscriber id (ElementId, RenderId) and RebuildReason is already there. Costs: (a) each reactive edit rebuilds about 27 crates instead of about 15, including flui-engine (72k lines) and flui-platform (46k), roughly 63% more source rebuilt per edit, estimated from line counts and not timed; (b) foundation gains thiserror, or hand-written Error impls; (c) the pub(crate) lifecycle hooks must still become public, so option 2 does not avoid that cost either.",
    "Option 1 (flui-reactive, tier V) must make public the element lifecycle hooks that are pub(crate) today: set_scheduler, begin_element_build, end_element_build, register_element_reader, release_element (reactive/mod.rs:217,373,391,429,448). They should be `#[doc(hidden)]` or sealed. Otherwise any crate could forge build bracketing, which weakens the ADR-0074 guard's authority. Option 2 carries the same cost. Only option 3 keeps them private.",
    "`Signal<T>` is !Send (reactive/mod.rs:653), while `CustomPainter` and `Animation<T>` require Send + Sync (custom_painter.rs:105, animation.rs:68). Render-phase subscribers must therefore register from RenderObject-side contexts (layout/paint cx implementing ReadScope), not by storing Signals inside painters or animations, unless those Send bounds are relaxed. This constraint is independent of which crate hosts the graph, and D4 does not mention it.",
    "D4 makes Listenable an Rc adapter over the graph, but the Listenable/ChangeNotifier/ValueNotifier family lives in flui-foundation (lib.rs:269-273, about 2.2k lines). With the graph in flui-reactive, the adapter must live in flui-reactive or above, and foundation's Arc/Mutex notifier either moves up or stays as a second system. This is a migration item missing from D4 and W3.",
    "P8 (a crate costs money) needs a named second consumer or seam. The strongest honest justification is the compile seam: 15 vs 27 invalidated crates during the W3 churn window. The render-phase consumer is still a hypothesis in D4 itself ('гипотеза: нужны для скролла и drag'). However, render-side invalidation consumers already exist today through Listenable (CustomPainter::repaint, ScrollPosition), so the second consumer is concrete if D4 commits to migrating those.",
    "The fold-back criterion must be measured as warm incremental invalidation (edit reactive, then time `cargo check -p flui-app` or `-p flui-widgets`), not as `--timings` unit time. The reactive crate's own check time is about 0.16s (probe), so a unit-time comparison will always show 'no difference' and would wrongly trigger folding it into foundation.",
    "D4's fan-out figure is wrong: foundation has 20 direct framework dependents, not 17. The D4 row in the decisions table should be corrected.",
    "Hypothesis, not verified in this run: Cargo has no early cutoff, so any rebuild of a dependency's rmeta forces its dependents to recheck even when only private code changed. This is what makes foundation placement expensive. Verify with a warm-edit experiment on a worktree before final sign-off."
  ],
  "experiments_run": [
    "Probe workspace at C:\\Users\\vanya\\AppData\\Local\\Temp\\claude\\D--flui\\bbb28042-f972-4a10-8e94-731819e26161\\scratchpad\\probe-reactive with 3 crates. `probe-reactive` holds a verbatim copy of reactive/mod.rs lines 1-839, with `use crate::owner::ExternalBuildScheduler` replaced by `trait RebuildSink` + `type ExternalBuildScheduler = Rc<dyn RebuildSink>` and `&dyn crate::BuildContext` replaced by `&dyn ReadScope`. Its only dependencies are flui-foundation (path), smallvec, thiserror and tracing. `probe-view` defines `trait BuildContext: ReadScope`. `probe-rendering` has a PaintCx implementing ReadScope with a RenderId subscriber. `CARGO_TARGET_DIR=<probe>/target cargo test --offline` passed: `test tests::get_records_read_through_upcast ... ok`. This proves (1) the module compiles outside flui-view with only foundation + smallvec + thiserror + tracing; (2) `count.get(cx)` compiles unchanged for `cx: &dyn BuildContext`, through implicit trait-upcasting coercion on rustc 1.98.1, and for `cx: &impl BuildContext`; (3) a rendering-layer context can implement ReadScope and call `Signal::get` without depending on view. The only compiler diagnostics were 2 warnings: `previous_reads` never read, and 5 hook methods never used, which shows the pub(crate) lifecycle hooks need a public but hidden surface after the split. Not done: I did not port flui-view's real call sites, and I did not include the module's own tests (lines 840-1159).",
    "`cargo check -p probe-reactive --offline --timings` (cold for foundation and the probe): flui-foundation took 0.32s and probe-reactive 0.16s. The crate's own compile time is negligible, so any gain comes from dependent invalidation only.",
    "`cargo tree -i {flui-foundation,flui-view,flui-rendering,flui-animation,flui-scheduler} --workspace -e normal --prefix none --offline`, counting flui* crates (self and examples included): foundation 27, scheduler 17, rendering 14, animation 14, view 12. The union of rendering, animation and view is 15. The same command with --depth 1 gave 20 direct dependents for foundation (excluding self).",
    "Line count per crate (`find crates/<c>/src -name '*.rs' | xargs cat | wc -l`) to estimate rebuild weight: about 212k lines spared by flui-reactive vs foundation placement (engine, platform, interaction, scheduler, semantics, painting, tree, layer, devtools), against about 335k lines rebuilt either way.",
    "`git log` churn: reactive/mod.rs 8 commits in total, all dated 2026-09-22 and 2026-09-23. crates/flui-foundation has 40 commits since 2026-07-27.",
    "Not run: a warm-edit rebuild timing on the real workspace (edit reactive, time `cargo check -p flui-app`). It would require modifying repo files or a worktree plus a near-full build on the shared, memory-limited host. The 63% figure is therefore an estimate from line counts. This is the recommended acceptance measurement for D4's W0 prototype."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "flui-reactive crate in tier V now (D4 as written, with its gaps fixed)",
      "description": "Extract crates/flui-view/src/reactive/mod.rs into a new internal crate `flui-reactive` (deps: flui-foundation, smallvec, thiserror, tracing), done upfront in W3. The crate hosts the graph, `Signal<T>`, `SignalSlot`, `SignalError`, `SignalSender`, `ReadScope { reactive(), signal_read(slot) }` and a one-method `RebuildSink` that replaces `pub(crate) ExternalBuildScheduler` (build_owner.rs:80, used only at reactive/mod.rs:639-641). flui-view declares `trait BuildContext: ReadScope`. flui-rendering and flui-animation depend on flui-reactive for Layout(RenderId) and Paint(RenderId) subscribers. The probe at scratchpad/probe-reactive shows it compiles and that `count.get(cx)` works through trait upcasting on rustc 1.98.1. D4 needs these amendments: fan-out is 20 direct / 27 transitive, not 17; the lifecycle hooks (reactive/mod.rs:217,373,391,429,448) need a non-forgeable public surface; the Listenable adapter's home has to be named; the fold-back metric has to be defined.",
      "pros": [
        "The inherent `Signal::get(cx)` lives in the crate that defines `Signal<T>`, so the orphan and inherent-impl rule is satisfied, and flui-rendering and flui-animation can name `Signal<T>` without an upward edge to flui-view.",
        "Smallest invalidation set that still serves render-phase subscribers. Measured with `cargo tree -i`: a reactive edit rebuilds the union of the view, rendering and animation reverse sets, 15 crates (about 335k src lines). With the graph in foundation it is 27 crates (about 547k lines). flui-engine (72k lines, wgpu), flui-platform (46k) and flui-interaction (40k) stay off the critical path during the high-churn W3 window.",
        "Direct precedent: Leptos `reactive_graph` sits below both its view layer and its renderer (tachys consumes it optionally). Sycamore `sycamore-reactive` and Floem `floem_reactive` are also leaf crates with few dependencies.",
        "A future merge back into foundation is a mechanical move plus a re-export. The facade keeps the public path stable."
      ],
      "cons": [
        "P8: a crate costs money, and until a render-phase subscriber actually lands, flui-view is its only consumer. D4 itself marks render subscribers as 'гипотеза'. Shipping the crate before its second consumer exists is the repository's 'unwired surface' defect at crate level.",
        "The element lifecycle hooks, `pub(crate)` today, become cross-crate API. The probe flagged 5 unused items. Plain `#[doc(hidden)] pub` lets any crate forge build bracketing, which weakens the ADR-0074 guard.",
        "Listenable/ChangeNotifier/ValueNotifier (about 2.2k lines in foundation, lib.rs:269-273) cannot become an adapter over a graph that lives above foundation. The adapter has to live in flui-reactive, and until the `!Send` flip in W5 foundation keeps the Arc/Mutex notifier as a second system for the Send+Sync users (CustomPainter custom_painter.rs:105, Animation animation.rs:68).",
        "Costs one more manifest, layer metadata, a rewrite of the Cargo.toml:71-77 'deliberately NO signals crate' note, and ARCHITECTURE.md."
      ],
      "cost_now": "About 1-2 days: move the file and its tests (lines 840-1159), add the RebuildSink trait, the ReadScope supertrait, a hidden or driver hook surface, the manifest and layer entry, rewrite the Cargo.toml note, fix the D4 fan-out figure. No behavior change.",
      "cost_later": "Low. Folding back into foundation is a move plus a re-export if a warm-edit measurement ever shows no gain. The real later cost is the Listenable unification, which every option has to pay.",
      "reversibility": "High. The public path goes through the facade (`flui::...`), and the crate is internal (unpublished), so merging or moving it is mechanical.",
      "fits_plan": "Matches D4 and W3 as written. Needs the four amendments listed in the description. Its weak spot is P8 when the crate lands before any rendering or animation consumer."
    },
    {
      "id": "B",
      "name": "Reactive core as a module inside flui-foundation now",
      "description": "Move reactive/mod.rs into flui-foundation. Every subscriber id is already there (ElementId and RenderId at id.rs:9,27, RebuildReason at rebuild_reason.rs:18), so nothing new needs naming. `ReadScope`, `Signal<T>` and `RebuildSink` go in foundation. Listenable becomes an in-crate adapter over the graph. Precedent: Bevy (change ticks in bevy_ecs), Slint and GPUI (reactivity as a core module; hypothesis, not re-checked).",
      "pros": [
        "No new crate, so P8 is satisfied trivially.",
        "The Listenable adapter sits next to the graph in one crate, and one observable system becomes reachable without moving the notifier family.",
        "Every current and future subscriber crate already depends on foundation, so no manifest edges change."
      ],
      "cons": [
        "Each reactive edit rebuilds all 27 foundation dependents instead of 15, including flui-engine (wgpu), flui-platform, flui-interaction, flui-scheduler, flui-semantics, flui-layer, flui-painting and flui-tree. That is about 63% more source per edit. This is an estimate from line counts, not a timing, and it lands in exactly the W3 window where Computed, Effect, Store, Writer and Bind make the graph churn (8 commits in 2 days already).",
        "Foundation gains thiserror, or hand-written Error impls, and a young, high-churn subsystem, which makes the grab-bag of values and ids worse.",
        "It does not avoid the public lifecycle-hook problem: those hooks still cross the foundation-to-view boundary.",
        "It puts element-lifecycle concepts (build bracketing, rebuild reasons) in the bottom layer, which every crate including platform and engine then 'sees'. That is a layering smell, and `forbid-reach` gates would have to police it."
      ],
      "cost_now": "About 1-2 days, similar to A, plus adding thiserror to foundation.",
      "cost_later": "Recurring: a slower edit-compile loop for every reactive change during W3-W5. Extracting it later is harder once foundation code (Listenable) depends on it internally.",
      "reversibility": "Medium. Moving out later means untangling the in-crate Listenable adapter.",
      "fits_plan": "D4 explicitly rejected this. The measured fan-out (20 direct, not 17) makes the rejection stronger. Acceptable as the end state once the API stops churning, which is the Bevy/Slint precedent."
    },
    {
      "id": "C",
      "name": "Stay in flui-view; rendering and animation subscribe through a type-erased trait in foundation",
      "description": "Keep the graph and `Signal<T>` in flui-view. Foundation exposes a trait such as `Readable<T>` or keeps using Listenable. flui-view adapts signals to it, and render objects and animations hold `Rc<dyn Readable<T>>` or `Arc<dyn Listenable>`. This is the Dioxus shape: the subscriber context lives in core and signals sit above it.",
      "pros": [
        "Zero crate cost, and the lifecycle hooks stay `pub(crate)`, so the ADR-0074 guard keeps full authority. This is the only option where that holds.",
        "Smallest rebuild set for reactive edits: flui-view and above, about 12 crates.",
        "No immediate migration work, because the current code already is this shape."
      ],
      "cons": [
        "It fails D4's typed render-phase requirement. flui-rendering and flui-animation cannot name `Signal<T>` and cannot call `Signal::get(cx)` from a layout or paint context, so they only get a type-erased callback.",
        "It keeps two observable systems permanently: the graph plus Listenable, which is exactly what D4 set out to unify. Every render-side consumer (CustomPainter::repaint at custom_painter.rs:140, ScrollPosition at scroll_position.rs:27) keeps using the Arc/Mutex notifier.",
        "Dioxus gets away with this only because it has no retained render tree that must subscribe. FLUI has one."
      ],
      "cost_now": "Near zero.",
      "cost_later": "High if render-phase subscribers are ever wanted. The move then happens anyway, after more code has been built on the split-brain model.",
      "reversibility": "High technically. The architectural debt of two systems grows with every new consumer.",
      "fits_plan": "Contradicts D4's outcome ('Render/animation могут подписываться; Listenable становится адаптером'). Viable only if the owner drops render-phase subscribers."
    },
    {
      "id": "D",
      "name": "Staged hybrid: build the seam inside flui-view first, extract flui-reactive in the PR that lands its first non-view consumer",
      "description": "Target end state is A, but the crate becomes gated on a consumer. Step 1 (W0/W3 start, zero crate cost): inside flui-view, replace `&dyn BuildContext` with `&dyn ReadScope` in Signal::get/with/try_* (reactive/mod.rs:709-760), make `BuildContext: ReadScope`, replace `ExternalBuildScheduler` with a `RebuildSink` trait, and split the handle into a cloneable `Reactive` (read and write) plus a non-Clone `ReactiveDriver` created once with the graph and owned by BuildOwner, which carries begin/end_element_build, register_element_reader, release_element and set_scheduler. The module then has no `crate::` imports, which a module-DAG gate can pin. Step 2 (in W3): a single PR moves the module verbatim into `flui-reactive` (tier V) *together with* the first render-phase subscriber. That subscriber is a ReadScope implemented on a layout or paint context with a Layout(RenderId)/Paint(RenderId) reader, migrating one concrete Listenable consumer (ScrollPosition or CustomPainter::repaint), and the PR carries a test that fails without it. The Listenable adapter lives in flui-reactive. Foundation's Arc notifier stays for Send+Sync users until the W5 `!Send` flip, then foundation's copy is deleted. Fold-back criterion: a warm-edit measurement (touch reactive, time `cargo check -p flui-app`, compare against the same edit in foundation), never `--timings` unit time. The crate's own check time is 0.16s (probe), so a unit-time comparison would always say 'no difference' and wrongly trigger folding it into foundation.",
      "pros": [
        "Satisfies P8 honestly: the crate appears in the same PR as its second consumer, so there is no unwired crate.",
        "The public hook surface becomes a type rule, not a review note ('make rules types'). Only the holder of `ReactiveDriver` (BuildOwner/realm) can bracket builds, and a `cx.reactive()` clone cannot. That preserves ADR-0074's guard across the crate boundary for both A and B.",
        "Step 1 is a pure refactor inside one crate. It de-risks the D4 prototype (ReadScope ergonomics, upcasting) with real call sites, which the probe did not port.",
        "The extraction in step 2 is then mechanical: the probe already showed the module compiles with only foundation, smallvec, thiserror and tracing.",
        "It also covers the gaps D4 does not record: the Send constraint (render subscribers register from RenderObject-side contexts, not from inside Send+Sync painters or animations) and the Listenable migration."
      ],
      "cons": [
        "Two PRs instead of one, and the crate lands somewhat later in W3.",
        "Until step 2, rendering and animation still cannot subscribe. That is acceptable, since no consumer exists yet.",
        "The ReactiveDriver split is a small API design task that D4 does not currently include."
      ],
      "cost_now": "About 1 day for step 1, a refactor in flui-view only with no manifest changes.",
      "cost_later": "About 1-2 days for step 2, including the first render subscriber and its test. The Listenable dedup waits until after W5, as in every option.",
      "reversibility": "Highest. After step 1 nothing new is committed publicly. After step 2 it is the same as A.",
      "fits_plan": "Keeps D4's decision (flui-reactive, tier V, BuildContext: ReadScope, render-phase subscribers) and W3 placement. It only changes the order and adds the entry conditions the architecture doc is missing: fan-out 20/27, hook sealing, Listenable home, Send constraint, fold-back metric."
    }
  ],
  "recommended": "D",
  "rationale": "Two constraints settle where the code lives. First, `count.get(cx)` has to stay an inherent method, so `Signal<T>`, `ReadScope` and the graph must sit in one crate. Second, flui-rendering and flui-animation must be able to name that crate. That rules out C: rendering would get only a type-erased Listenable-style callback, and two observable systems would stay forever. It leaves foundation (B) or a new crate below view (A/D).\n\n**A/D versus B is a compile-seam question.**\n- A reactive-only edit rebuilds 15 crates, about 335k lines (measured with `cargo tree -i`).\n- With the core in foundation it rebuilds 27, about 547k lines. That adds flui-engine, flui-platform and flui-interaction.\n- The rebuilt-line comparison is an estimate from line counts, not a timed rebuild. A warm-edit timing is still needed to confirm it.\n- The difference matters most in W3, when the graph will churn (8 commits in 2 days already; Computed, Effect, Store, Writer and Bind still to come).\n- Leptos `reactive_graph`, Sycamore and Floem all use a leaf reactive crate. Bevy, Slint and GPUI fold reactivity into the core, which suits a stable API, i.e. the fold-back end state.\n\n**D is preferred over A because of P8 and the unwired-surface rule.** Today flui-view is the only consumer. D4 still calls render subscribers a hypothesis, although the Listenable consumers they would replace (CustomPainter::repaint, ScrollPosition) are concrete.\n\nD first builds the seam inside flui-view at no crate cost:\n- `ReadScope` as a supertrait of `BuildContext`\n- a `RebuildSink` trait in place of the scheduler\n- a non-Clone `ReactiveDriver` that holds the lifecycle hooks, so build bracketing is a type rule and not a review item across the crate boundary\n\nThe crate is then extracted in the same PR that migrates one real render-phase consumer, with a test that fails without it.\n\n**D4 amendments this answer records:**\n- Fan-out is 20 direct / 27 transitive dependents, not 17.\n- The hooks at reactive/mod.rs:217,373,391,429,448 need a non-forgeable surface. Any option that moves the graph out of flui-view pays this, B included.\n- The Listenable adapter lives in flui-reactive. Foundation's Arc notifier stays until W5, because CustomPainter and Animation are Send+Sync and `Signal` is !Send.\n- The fold-back test is a warm-edit `cargo check -p flui-app` timing, never `--timings` unit time. The crate itself checks in 0.16s, so unit time would always show no difference.\n\n**Not verified:** the warm-edit timing on the real workspace, and a port of flui-view's actual call sites (the probe covered only the module and synthetic contexts)."
}
```

## judge_q5_reactive_crate_engineer

```json
{
  "choice": "D",
  "confidence": 0.78,
  "reasons": [
    "I spot-checked the research against the code and it holds. reactive/mod.rs:65-68 imports only flui_foundation::{ElementId, RebuildReason}, smallvec and crate::owner::ExternalBuildScheduler. The lifecycle hooks are pub(crate) (for example set_scheduler at :217 and begin_element_build at :373). Signal<T> is !Send through PhantomData<*const ()> (:653). try_with takes &dyn crate::BuildContext and calls only cx.reactive() and cx.signal_read() (:709-717). CustomPainter is Send + Sync (custom_painter.rs:105).",
    "Rust allows inherent impls only in the crate that defines the type. So Signal<T>, ReadScope and the graph have to share one crate, and flui-rendering/flui-animation must be able to name that crate. That rules out C: it gives rendering only a type-erased Listenable-style callback and keeps two observable systems for good, which is the outcome D4 was written to remove.",
    "B (a module in flui-foundation) is technically sound because every subscriber id already lives there. Its cost is 27 invalidated crates per reactive edit against 15 for a crate below view, measured with cargo tree -i. The extra crates include engine, platform and interaction, about 212k lines. That lands exactly in the W3 churn window. B also still has to make the hooks public, so it saves nothing on that front.",
    "A and D reach the same end state. D is better than A on P8 and on the repository's most common defect, unwired surface: the crate appears only in the PR that also adds its first non-view consumer. Today flui-view is the only consumer (checked with grep), and D4 itself calls render subscribers a hypothesis.",
    "D's step 1 turns the hook-forgery risk into a type rule (a non-Clone ReactiveDriver owned by BuildOwner) before any crate boundary exists. That follows 'make rules types, not reviews' and keeps the ADR-0074 guard authoritative after extraction. Step 1 is a refactor inside one crate, and it exercises the real call sites, which the probe did not port.",
    "Reversibility is highest. Step 1 commits nothing new publicly. Step 2 is a mechanical move, already shown to compile with only foundation, smallvec, thiserror and tracing (probe-reactive), and a later fold into foundation stays a move plus a re-export."
  ],
  "conditions": [
    "Step 1 guardrail: after the refactor the reactive module has zero `crate::` imports. Only ReadScope, RebuildSink and ReactiveDriver cross its edge. Pin this with a test or check that fails if a crate:: import comes back, or with a compile probe. A review note does not count.",
    "ReactiveDriver is created once, together with the graph. It is not Clone and is not reachable from BuildContext or ReadScope. Add a compile_fail doctest showing that `cx.reactive()` cannot call begin_element_build or register_element_reader. If the hooks must be `pub` after extraction, they live only on ReactiveDriver. Do not use a bare `#[doc(hidden)] pub` on Reactive.",
    "Step 2 is a single PR that creates flui-reactive (tier V, `[package.metadata.flui] layer`, deps foundation + smallvec + thiserror + tracing only) and migrates one concrete Listenable consumer (ScrollPosition or CustomPainter::repaint) to a Layout(RenderId) or Paint(RenderId) subscriber. It must include a test that fails with that migration reverted, and it must show the repaint or relayout actually happening, not just that the subscription exists. If no such consumer is ready, the crate is not created.",
    "Render-phase subscribers register from RenderObject-side layout and paint contexts that implement ReadScope. Do not relax the Send + Sync bounds on CustomPainter or Animation just to hold a Signal. If relaxing them turns out to be necessary, that needs its own ADR, tied to the W5 !Send flip.",
    "Update D4 and its ADR in the same PR as step 2. Correct the fan-out to 20 direct / 27 transitive (measured with `cargo tree -i flui-foundation --workspace -e normal`). Name flui-reactive as the home of the Listenable adapter, with foundation's Arc/Mutex notifier kept until W5 and then deleted. Rewrite the Cargo.toml:71-77 'deliberately NO signals crate' note, including its reference to contract C1 (setState/Inherited model), which may itself need an explicit supersede.",
    "The fold-back criterion is a warm-edit measurement: touch reactive, time `cargo check -p flui-app`, and compare with the same edit placed in foundation. Never use `--timings` unit time (the crate checks in about 0.16s, so unit time always shows no difference). Record this measurement in the step 2 PR, because the 63% figure is still a line-count estimate.",
    "Hypothesis to confirm during step 2: Cargo recompiles dependents whenever a dependency's rmeta changes, even for a private-only edit (no early cutoff). If the warm-edit experiment shows otherwise, re-evaluate B as the end state."
  ]
}
```

## judge_q5_reactive_crate_ecosystem_author

```json
{
  "choice": "D",
  "confidence": 0.8,
  "reasons": [
    "The inherent-method rule settles the crate question. `count.get(cx)` has to be an inherent method on `Signal<T>`, so the graph, `Signal<T>` and `ReadScope` must all live in one crate. That crate must also be one flui-rendering and flui-animation can name. This rules out C: rendering would be left with a type-erased Listenable-style callback, and the codebase would keep two observable systems for good. I checked the evidence: `_local: PhantomData<*const ()>` is at reactive/mod.rs:653, and the five pub(crate) lifecycle hooks are at lines 217, 373, 391, 429 and 448.",
    "For a third-party package author (H3-H4), the public surface counts more than where the crate sits. D turns the non-forgeable hook surface into a type rule: a non-Clone `ReactiveDriver` owned by `BuildOwner`. A plugin that holds `cx.reactive()` then cannot forge build bracketing, so the ADR-0074 guard holds even after the code leaves flui-view. A (a plain `#[doc(hidden)] pub`) and B both lose this.",
    "For app authors, `count.get(cx)` and the facade path `flui::...` stay unchanged across step 1, step 2, and any later fold back into foundation. The probe showed the trait upcast `&dyn BuildContext -> &dyn ReadScope` works on rustc 1.98.1, so choosing D costs no ergonomics.",
    "A and D beat B on compile time for the W3 churn window: a reactive-only edit invalidates 15 crates versus 27 (from `cargo tree -i`), and B would put flui-engine and flui-platform on the critical path. Both are estimates, not timings. Churn is already high: 8 commits in 2 days.",
    "D beats A on P8 and the unwired-surface rule. The crate lands in the same PR as its second real consumer, which migrates ScrollPosition or `CustomPainter::repaint` from Listenable. A crate with no second consumer does not ship.",
    "D is the cheapest to reverse. Step 1 is a refactor inside one crate with no manifest or public commitment. Step 2 is a mechanical move that the probe has already exercised."
  ],
  "conditions": [
    "Step 1 is not done until reactive/mod.rs has no `crate::` imports and `Signal::get/with/try_*` take `&dyn ReadScope`. A module-level check or a compile test must pin this, so step 2 stays a verbatim move.",
    "`ReactiveDriver` must be non-Clone and created once alongside the graph. A compile_fail doctest must show that a `Reactive` handle obtained from `cx.reactive()` cannot call begin/end_element_build, register_element_reader, release_element or set_scheduler.",
    "The extraction PR must include one real render-phase subscriber: a layout or paint context implementing `ReadScope` with a Layout(RenderId) or Paint(RenderId) reader. It must migrate ScrollPosition or `CustomPainter::repaint`, with a test that fails without the change. Without that subscriber there is no crate.",
    "Record the Send constraint in D4: `Signal` is !Send, while `CustomPainter` and `Animation` require Send + Sync. Render subscribers therefore register through RenderObject-side contexts, not by storing a Signal in a painter or animation. Foundation keeps its Arc notifier until the W5 !Send flip. The Listenable adapter over the graph lives in flui-reactive.",
    "Correct D4's fan-out figure to 20 direct / 27 transitive dependents. Define the fold-back criterion as a warm-edit timing: touch reactive, then time `cargo check -p flui-app`, and compare against the same edit made in foundation. Never use `--timings` unit time for this.",
    "Before sign-off, run the warm-edit experiment in a worktree to confirm the 15-versus-27 invalidation gap in real seconds. If the gap is negligible, B, as the end state after W5, becomes the default.",
    "In the same PR as the crate, rewrite the Cargo.toml:71-77 'deliberately NO signals crate' note, add the `[package.metadata.flui]` layer, and write ARCHITECTURE.md for flui-reactive. The facade stays the only public path; flui-reactive is not published on its own before 1.0."
  ]
}
```

## judge_q5_reactive_crate_owner

```json
{
  "choice": "D",
  "confidence": 0.78,
  "reasons": [
    "The inherent-method rule decides where the code lives. `count.get(cx)` must be an inherent method on `Signal<T>`, so the graph, `Signal<T>`, `SignalSlot`, `SignalError`, `SignalSender` and `ReadScope` belong in one crate. flui-rendering and flui-animation must be able to name that crate without depending on flui-view. That rules out C, which leaves two observable systems in place for good and does not deliver D4's typed render-phase subscribers.",
    "Placing the core in foundation (B) is the wrong trade during W3 churn. Measured with `cargo tree -i`, a reactive edit invalidates 27 crates in foundation versus 15 in a dedicated crate, and the extra ones include flui-engine, flui-platform and flui-interaction (about 212k extra source lines, estimated from line counts, not timed). The reactive module had 8 commits in 2 days, and Computed, Effect, Store, Writer and Bind are still to come. B also puts element-lifecycle concepts into the bottom layer that engine and platform see. B still has to make the lifecycle hooks public, so it does not even save that cost.",
    "D is preferred over A because of P8 and the unwired-surface rule. Today flui-view is the only consumer, and D4 still calls render subscribers a hypothesis. Under A, the crate would land before its second consumer exists. Under D, the crate appears in the same PR as a concrete migrated consumer (ScrollPosition or CustomPainter::repaint), with a test that fails without it.",
    "Step 1 of D turns review rules into type rules, which the project prefers. A non-Clone `ReactiveDriver`, owned by BuildOwner, is the only holder of begin/end_element_build, register_element_reader, release_element and set_scheduler. That keeps the ADR-0074 guard intact across a crate boundary. Plain `#[doc(hidden)] pub` hooks would let any crate forge build bracketing.",
    "Fit for a sole owner: step 1 is about a day of refactoring inside one crate with no manifest changes and no new crate to maintain. It is fully reversible, and it tests ReadScope and upcasting against flui-view's real call sites, which the probe did not do. Step 2 is then mechanical: the probe showed the module compiles with only foundation, smallvec, thiserror and tracing, and that `get(cx)` works through `&dyn BuildContext` upcasting on rustc 1.98.1.",
    "Precedents support a leaf reactive crate while the API churns: Leptos reactive_graph below tachys, sycamore-reactive, floem_reactive. The Bevy, Slint and GPUI fold-into-core shape suits a stable API, so it is a legitimate later end state and not a reason to fold now."
  ],
  "conditions": [
    "Step 1 must leave reactive/mod.rs with no `crate::` imports. Signal::get/with/try_* take `&dyn ReadScope`, `BuildContext: ReadScope`, and `ExternalBuildScheduler` is replaced by a one-method `RebuildSink` trait. Pin this with a test or a module-dependency check, not a comment.",
    "The lifecycle hooks move onto a non-Clone `ReactiveDriver` that is created together with the graph and owned by BuildOwner or the realm. A `cx.reactive()` clone must not be able to call them. This is required before any extraction.",
    "Extraction into `flui-reactive` (tier V, internal, `publish = false`, layer metadata set) happens only in the PR that migrates one real render-phase consumer. That consumer registers a Layout(RenderId) or Paint(RenderId) reader from a RenderObject-side layout or paint context, and the PR includes a test that fails with the production hunk reverted. If no render consumer is scheduled by the end of W3, the graph stays in flui-view and D4 is revised to say so.",
    "Record the Send constraint in D4: `Signal<T>` is !Send, while CustomPainter and Animation<T> are Send + Sync. Render subscribers therefore register through contexts and never store Signals inside painters or animations, unless the W5 !Send flip lands first.",
    "Record where the Listenable adapter lives: in flui-reactive. Foundation's Arc/Mutex notifier family stays for Send + Sync users until W5, and is then deleted rather than kept as a permanent second system. Put this deletion item into W5.",
    "Correct D4's fan-out figure to 20 direct and 27 transitive dependents (from 17). In the extraction PR, rewrite the Cargo.toml:71-77 'deliberately NO signals crate' note and add flui-reactive/ARCHITECTURE.md.",
    "Measure the fold-back criterion as a warm-edit rebuild: touch reactive, then time `cargo check -p flui-app`, and compare against the same edit placed in foundation. Never use `--timings` unit time, because the crate itself checks in about 0.16s and a unit-time comparison would always show no difference. Before sign-off, run this warm-edit timing once in a worktree to confirm the unverified estimate of about 63% more rebuilt source."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "Step 2 is described as a verbatim move of the probed module, but it is not. The graph is hard-typed to element readers. Adding Layout(RenderId)/Paint(RenderId) subscribers changes the graph's internals, its public diagnostic API and its guard semantics. The probe only proved that the element-only graph compiles standalone. The step-2 PR therefore bundles three non-trivial changes: crate extraction, generalizing the graph's readers and guards, and migrating a Listenable consumer.",
      "evidence": "crates/flui-view/src/reactive/mod.rs:136 (`element_readers: SmallVec<[ElementId; 4]>`), :143 (`element_reads: HashMap<ElementId, ..>`), :150 (`building: Option<ElementId>`), :191 (pub `SlotInfo.readers: Vec<ElementId>`), :469 (pub `readers_of -> Vec<ElementId>`). The write refusal (SignalError::WrittenDuringBuild, :101-105) is armed only by begin_element_build (:373-383). No layout or paint phase exists in the graph.",
      "severity": "major",
      "fix": "Split step 2. (2a) Inside flui-view, generalize the reader to an enum (Element/Layout/Paint) and add phase guards (write-during-layout/paint refusal or deferral), with tests. (2b) Extract the crate together with the first render consumer. Drop the 'mechanical, probe-proven' claim for anything beyond the element-only graph."
    },
    {
      "problem": "The hook-sealing condition is incompatible with render-phase subscribers. It says ReactiveDriver is non-Clone, 'created once together with the graph' and 'owned by BuildOwner'. But a render-phase ReadScope implemented in flui-rendering must register Layout/Paint readers, a privileged hook, and flui-rendering sits below flui-view and cannot reach BuildOwner. Its PipelineOwner is a separate object. So either a second privileged capability exists, or the hook becomes public again, which is exactly what the condition forbids.",
      "evidence": "Today registration is done by the context via the owner: crates/flui-view/src/context/element_build_context.rs:230-235 (`self.owner.read().reactive().register_element_reader(..)`). flui-view depends on rendering, not the reverse: element_build_context.rs:707 (`pipeline_owner: Option<flui_rendering::pipeline::PipelineCell>`). PipelineCell is a separate `Rc<RefCell<PipelineOwner>>` (crates/flui-rendering/src/pipeline/owner/cell.rs:51).",
      "severity": "major",
      "fix": "Design the capability per phase in step 1: an ElementDriver held by BuildOwner, plus a RenderDriver (layout/paint registration and bracketing) minted from the same graph by the realm and handed to PipelineOwner. Both are non-Clone, and the compile_fail doctest covers both. Alternatively, state that the realm mints exactly two drivers."
    },
    {
      "problem": "Both named first consumers are poor or blocked migration targets. ScrollPosition is written from inside perform_layout (the content-dimension feedback), so as a signal it would write during layout. Neither the current guards nor any proposed ones define that. CustomPainter::repaint lives on a Send+Sync trait and returns Arc<dyn Listenable>. The paint path has no ReadScope parameter, so a Paint(RenderId) reader needs a CustomPainter API change (also FlowDelegate). That is more than a small consumer migration.",
      "evidence": "crates/flui-objects/src/sliver/viewport.rs:1057,1104,1827-1828 call offset.apply_viewport_dimension/apply_content_dimensions during layout. crates/flui-rendering/src/view/scroll_position.rs:24-28,347-349 (Arc<Inner> + parking_lot Mutex + ChangeNotifier). crates/flui-rendering/src/delegates/custom_painter.rs:105 (`CustomPainter: Send + Sync`) and :140 (`fn repaint(&self) -> Option<Arc<dyn Listenable>>`). The same shape appears at flow_delegate.rs:148, flui-objects/src/layout/flow.rs:508 and proxy/custom_paint.rs:553.",
      "severity": "major",
      "fix": "Name a first consumer that only reads during layout or paint and is written outside the frame phases. An example is a RenderObject field read in paint through a PaintContext: ReadScope (RenderObject has no Send bound, render_object.rs:178, and PipelineOwner is !Send, cell.rs:213). Keep ScrollPosition out until a write-during-layout policy exists. For CustomPainter, plan the API change explicitly, for example storing a Send `SignalSender`/`SignalSlot` rather than `Signal`."
    },
    {
      "problem": "D ignores that signals are an off-by-default cargo feature. Step 1 makes `BuildContext: ReadScope` unconditional, or needs cfg-duplicated trait declarations, because a supertrait bound cannot be cfg-gated. Step 2 has flui-rendering replace a Listenable consumer with a signal, which only works if signals are always compiled in rendering. So the D4 decision to stop signals being a feature must land before, or together with, step 2. D does not record this ordering, and the facade says the feature stays opt-in until a separate registry lands.",
      "evidence": "crates/flui-view/Cargo.toml:110 (`default = []`), :119-123 (`signals = []`, 'Off by default until the go/no-go measurement'). crates/flui-view/src/lib.rs:96,223 and context/build_context.rs:131,136,579 are all `#[cfg(feature = \"signals\")]`. Root Cargo.toml:642-645: 'stays an opt-in until the #1090 field-mask registry lands'. The architecture doc (line 188) says signals stop being a feature, but gives no ordering relative to the crate extraction.",
      "severity": "major",
      "fix": "Add an explicit entry condition. Step 1 makes the reactive module and ReadScope unconditional, which removes the `signals` feature per D4. Alternatively, step 2 is blocked until the feature is removed. Record the ADR-0074 go/no-go dependency."
    },
    {
      "problem": "The proposed ReadScope surface `{ reactive(), signal_read(slot) }` contradicts D4. D4 says `BuildContext::reactive()` is deleted. With `BuildContext: ReadScope`, every build context keeps a `reactive()` that returns a cloneable read/write graph handle. That is the handle D4 removes, and it is the one used to write or create signals outside the narrowed Writer path.",
      "evidence": "flui-global-architecture.md:596 (D4: '`BuildContext::reactive()` удалён'). The current surface is crates/flui-view/src/context/build_context.rs:132 (`fn reactive(&self) -> crate::reactive::Reactive`). Option A/D define `ReadScope { reactive(), signal_read(slot) }`.",
      "severity": "minor",
      "fix": "Give ReadScope only read-registration plus a graph-identity check, for example `fn scope(&self) -> ScopeRef<'_>`. Do not return the owned Reactive handle. Keep write and create access on LifecycleContext/Writer."
    },
    {
      "problem": "The fan-out figures are slightly miscounted. `cargo tree -i` output includes the crate itself. Under A/D a reactive edit also rebuilds flui-reactive, and the union of dependents is 15 including view, rendering and animation. The claimed 15 vs 27 comparison happens to hold because both sides count self (27 = foundation plus 26 dependents). But '20 direct / 27 transitive dependents' overstates the dependents by one if 27 includes foundation.",
      "evidence": "`cargo tree -i <c> --workspace -e normal --prefix none` with unique flui names counted: flui-foundation 27, flui-view 12, flui-rendering 14, flui-animation 14, union(view, rendering, animation) 15. All counts include the queried crate.",
      "severity": "minor",
      "fix": "Record the numbers as 'crates rebuilt per edit: 27 (foundation) vs 16 (flui-reactive plus the 15-crate union)'. State the counting rule next to the command."
    }
  ]
}
```
