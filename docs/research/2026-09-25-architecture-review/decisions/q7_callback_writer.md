# Decision panel: q7_callback_writer

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Signal writes today already take a capability argument, but a weak one: `Signal::set(self, r: &Reactive, value)`, `update(self, r: &Reactive, ..)` and `set_if_changed(self, r: &Reactive, ..)` are at crates/flui-view/src/reactive/mod.rs:774,783,793. `Reactive` is `#[derive(Clone)]`, an `Rc<RefCell<Inner>>` (mod.rs:159-164), and every context hands it out (`BuildContext::reactive()`, crates/flui-view/src/context/build_context.rs:129-132). So `build` can obtain it. The only protection is the runtime guard `refuse_if_building` → `SignalError::WrittenDuringBuild` (mod.rs:566-580).",
    "`Signal<T>` is already `!Send + !Sync` through `PhantomData<*const ()>` (mod.rs:648-654). `SignalSender<T>` is the Send form, reattached on the owner thread (mod.rs:810-837). This means a Signal cannot be captured in any of the `+ Send + Sync` on_* setters today; W5's `!Send` flip is a prerequisite for shape A as well as for C.",
    "No production code uses signals. `grep -rln 'Signal<|\\.signal(' crates/*/src src examples` (excluding reactive/ and the unrelated interaction signal_resolver) finds only crates/flui-app/src/app/ui_realm/tests/mod.rs and a doc comment in build_context.rs. The feature is off by default (`signals = []` in crates/flui-view/Cargo.toml:123; root Cargo.toml:645). A Writer migration therefore migrates signatures, not existing signal writes.",
    "Real user state today is `StateCell`/`StateHandle` (`Rc<Cell>` plus `RebuildHandle`). `StateCell::set` has no capability and no build guard: it sets the value and schedules a rebuild (crates/flui-view/src/state_cell.rs:228-231). examples/counter.rs:55 uses `.on_pressed(move || count.update(|n| n + 1))`. The Writer design leaves this path unprotected unless StateCell also takes the Writer.",
    "The existing test crates/flui-widgets/tests/signals_legal_shapes.rs:71-78 already models shape A as `Box<dyn Fn(&Reactive)>` callbacks, while every real setter is `Fn()`-shaped.",
    "Setter census: `grep -rhoE 'pub fn on_[a-z_]+' crates/flui-widgets/src crates/flui-material/src crates/flui-cupertino/src | wc -l` → 92. By closure shape: 22 `impl Fn() + 'static`, 13 `impl Fn() + Send + Sync + 'static`, 8 `Fn(bool)`, 6 `Fn(PointerDispatch<'_>)`, 3 `Fn(usize)`, 3 `Fn(DeviceId, Offset)`, 3 `Fn(&str)`, and a long tail of detail structs. At least 23 carry `Send + Sync`. Two are routing hooks returning a value (`Fn(&RouteRequest) -> Option<GeneratedRoute>`), and some return `EventPropagation` or `bool` (drag-target will-accept). These are queries, and a Writer parameter would wrongly let them write.",
    "Call-site census: `grep -rEo '\\.on_[a-z_]+\\(' | wc -l` gives examples 43, flui-widgets/src 118, flui-material/src 86, flui-cupertino/src 9, flui-widgets/tests 83, flui-material/tests 91, flui-cli templates 2. That is about 432 sites for shape A to touch (upper bound: it includes some internal recognizer `.on_*` calls).",
    "Many writes in the catalog do not go through on_* setters at all. Counts over flui-widgets, flui-material, flui-cupertino and the facade: `add_listener(` 70, `add_status_listener(` 14, `post_frame` 79, `rebuild_handle()` 49, `schedule(RebuildReason::AsyncCompletion)` 2 (image/resolve.rs:246, navigator/navigator.rs:390). These are the listener, animation, timer, post-frame and async-continuation paths. Shape A must also give a Writer on each of them (Listenable listeners, AnimationStatus, post-frame, Spawner continuation, timers, `UiCommand::SignalWrite`), so the break is larger than the 92 setters.",
    "Interaction-layer callback aliases are `Rc<dyn Fn(Details)>` in flui-interaction, e.g. `TapCallback` (recognizers/tap.rs:91) and the drag, scale and long_press aliases (drag.rs:113-121). flui-interaction is a Substrate crate. For recognizers to pass `&mut Writer` themselves, Writer has to live at or below that layer: the planned `flui-reactive` (tier V, flui-global-architecture.md §4.6 line 262). The alternative is for widgets to wrap recognizer callbacks and create the Writer in flui-widgets, which needs a realm handle at dispatch.",
    "The cross-thread path already exists. `UiCommand::SignalWrite(Box<dyn FnOnce(&Reactive)>)` is applied in crates/flui-app/src/app/ui_realm/commands.rs:449-455 against `self.widgets().with_build_owner(|o| o.reactive().clone())`, i.e. the primary presentation's graph. This is the П4 conformance defect. A Writer created per event from the dispatching presentation's realm would fix it by construction; an ambient or TLS lookup has to reimplement the same routing.",
    "ADR-0078 (docs/adr/ADR-0078*.md, section 2 table) names the runtime guard as the authoritative enforcement of \"Signal written or created during build\". ADR-0075 requirement 4 calls a write inside a computation a typed error (`WrittenDuringCompute`). The architecture doc already downgrades Writer to \"an ergonomic narrowing, not a replacement for the guard\" (flui-global-architecture.md:266, 695)."
  ],
  "market_precedents": [
    {
      "who": "GPUI (Zed)",
      "what": "Every element listener receives the app context explicitly: `on_click(impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)`, `on_mouse_down(impl Fn(&MouseDownEvent, &mut Window, &mut App))`. `cx.listener(...)` binds the entity.",
      "outcome_or_lesson": "This is the closest Rust precedent for shape A, a context parameter on every callback, and it works in a large production codebase. The context is broad (window + app), so one parameter serves writes, focus, spawn and navigation. It is not a phase guard: `render` also gets `&mut Context`. The capability is about borrowck and ownership, not \"no writes in build\".",
      "source": "https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/elements/div.rs"
    },
    {
      "who": "Xilem (linebender)",
      "what": "Callbacks receive `&mut AppState` as their first argument, e.g. checkbox `move |data: &mut TaskList, checked| {..}` and textbox `|task_list: &mut TaskList, new_value| {..}`.",
      "outcome_or_lesson": "The write capability is an explicit callback parameter, type-checked, with a value argument following the state (the same shape as `Fn(&mut Writer, bool)`). Writes are impossible in view construction because the view fn gets `&mut State` only as the builder. This is strong typing at the cost of every callback carrying the parameter.",
      "source": "https://raw.githubusercontent.com/linebender/xilem/main/xilem/examples/to_do_mvc.rs"
    },
    {
      "who": "Dioxus (signals 0.5+)",
      "what": "Copy `Signal` with no context argument; `signal.set(v)` or `*signal.write()` works anywhere. At runtime, a debug warning `signal_write_in_component_body` flags writes during render, plus a 'Write on signal in reactive scope' warning, with an opt-out attribute being discussed.",
      "outcome_or_lesson": "Shape C plus a runtime guard. It is ergonomic, but the warnings produce false-positive complaints (issue #3852 'Unnecessary Write on Signal on ReactiveScope warning') and misses. It is the idiom LLMs most often reproduce for Rust UI.",
      "source": "https://github.com/DioxusLabs/dioxus/issues/3852 ; https://github.com/DioxusLabs/dioxus/blob/main/packages/signals/src/signal.rs"
    },
    {
      "who": "Leptos 0.7",
      "what": "`signal()` returns `(ReadSignal, WriteSignal)`, so capability is split by handle type, not by context; writes are allowed anywhere. The book officially discourages writing signals from effects (less efficient, risks loops and 'reactive spaghetti').",
      "outcome_or_lesson": "Read/write segregation by handle is a cheaper typed alternative: pass only a ReadSignal to a child and it cannot write. It says nothing about phase (writing in the component body compiles). The effects-writing concern maps to ADR-0075's WrittenDuringCompute.",
      "source": "https://book.leptos.dev/reactivity/working_with_signals.html"
    },
    {
      "who": "SolidJS",
      "what": "`createSignal` returns a [getter, setter] tuple; the setter can be called anywhere, with `batch` for coalescing.",
      "outcome_or_lesson": "Same split-handle idea as Leptos. There is no phase enforcement; convention alone keeps writes in handlers and effects.",
      "source": "https://docs.solidjs.com/reference/basic-reactivity/create-signal (general knowledge, not fetched this run)"
    },
    {
      "who": "Jetpack Compose",
      "what": "`mutableStateOf` is writable anywhere. A write after a read in the same composition (a 'backwards write') is documented as an anti-pattern that causes infinite recomposition. The guidance is to write only from event lambdas such as onClick.",
      "outcome_or_lesson": "No compile-time or hard runtime guard, only docs and lint-level advice. Even with Google's resources this bug class stays a recurring performance issue, which is evidence that convention alone is weak.",
      "source": "https://developer.android.com/develop/ui/compose/performance/backwards-write"
    },
    {
      "who": "SwiftUI",
      "what": "@State writes are allowed anywhere on the main actor. A write during `body` evaluation triggers the runtime purple warning 'Modifying state during view update, this will cause undefined behavior.'",
      "outcome_or_lesson": "Shape C plus a runtime warning, the same model as the ADR-0074 guard. Common fixes, such as deferring with DispatchQueue.main.async or `.task`, show that writes from async continuations are routine and must be first-class.",
      "source": "https://www.hackingwithswift.com/quick-start/swiftui/how-to-fix-modifying-state-during-view-update-this-will-cause-undefined-behavior ; https://forums.swift.org/t/runtime-warning-modifying-state-during-view-update-this-will-cause-undefined-behavior/77364"
    },
    {
      "who": "Flutter",
      "what": "`setState` is callable from any callback, listener, timer or async continuation (the `if (mounted) setState(..)` idiom). Calling it during build hits the assertion 'setState() or markNeedsBuild() called during build.'",
      "outcome_or_lesson": "This is the reference behaviour FLUI's guard already mirrors. Most real Flutter writes come from listeners, animation ticks and futures, not only onTap/onPressed setters, which supports widening the Writer surface beyond the 92 setters.",
      "source": "https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/framework.dart (setState assertion; general knowledge, reference clone .flutter not present locally)"
    },
    {
      "who": "Iced",
      "what": "Callbacks only produce messages (`on_press(Message::Increment)`); state is mutated exclusively in `update(&mut self, Message)`.",
      "outcome_or_lesson": "The strongest typing of all: writes during view are impossible because view takes `&self`. It costs a message enum and has no direct closure writes. It is not a fit for FLUI's Flutter-shaped widget API, but it shows the ecosystem accepts write-capability restrictions.",
      "source": "https://docs.rs/iced (general knowledge, not fetched this run)"
    }
  ],
  "constraints": [
    "Writer must be a borrowed, lifetime-carrying `&mut Writer<'_>`, never an owned token. Probe e1_owned_token shows an owned `Tok` passed by value infers cleanly in a `let` closure but can be stashed in a thread_local (compiles), so a later `build` could write with it. Probe a5 shows the borrowed form cannot be stored in `'static` state ('lifetime may not live long enough'). Only the borrowed form keeps the compile-time guarantee.",
    "The borrowed form has a known Rust ergonomics cost. A closure written inline in the call works (a1, a7, a9 compile). A closure bound in a `let` first fails with 'implementation of `Fn` is not general enough … for any lifetime' (a2). The fixes are an annotated param `|w: &mut Writer<'_>|` (a3 compiles) or a helper `fn callback<F: Fn(&mut Writer<'_>)+'static>(f: F) -> F` (a10 compiles). The helper must ship with shape A, and its name belongs in agent guidance, because the HRTB error text is opaque to humans and agents alike.",
    "Writes during `build` fail at compile time only if `build` has no route to a Writer. `BuildContext::reactive()` (build_context.rs:132) must be removed in the same PR, as the architecture doc already says (line 266). `StateCell::set` must also take the Writer, or be explicitly documented as the unguarded low tier, or the protection leaks through the path users actually use.",
    "The runtime guard cannot be removed under any shape. Probe test `a_nested_sync_callback_during_build_still_needs_guard` shows a catalog widget that holds a Writer and invokes a user callback synchronously during its own build: it compiles and is caught only by the guard. ADR-0075 write-in-compute is also runtime-only.",
    "Query-style callbacks (`Fn(&RouteRequest) -> Option<GeneratedRoute>`, `Fn(&DragTargetDetails<T>) -> bool` will-accept, `-> EventPropagation`) must not receive a Writer, because they run during routing or hit-testing decisions. Shape A has to classify the 92 setters into event (gets Writer) and query (does not), not rewrite them all blindly.",
    "Writer must also be delivered on non-setter paths: Listenable/animation listeners (70 + 14 sites), post-frame (79), timers, Spawner continuations and `UiCommand::SignalWrite`. Otherwise users need an escape hatch such as ambient `Writer::current()`, which reintroduces shape B.",
    "Writer (or the dispatcher that creates it) must live at or below flui-interaction's layer, i.e. in `flui-reactive` (tier V). Otherwise recognizer `Rc<dyn Fn(Details)>` aliases can't carry it, and every widget wraps callbacks.",
    "The `!Send` flip is a hard dependency. Signal is already `!Send` (mod.rs:651-653), so 23+ `Send + Sync` setters can't capture it today (probes a6 and c1 confirm the `*const ()` Send error). Doing W5's Writer migration and `!Send` together, as the doc plans (line 560), is correct.",
    "Signals have zero production users (no Signal in crates/*/src, src, examples outside tests). The cost of shape A is the signature change at about 432 on_* call sites plus listener paths, almost entirely a mechanical `move ||` → `move |_w|` / `move |v|` → `move |_w, v|` rewrite that `flui migrate` can do syntactically. The Send-bound removal is the harder half."
  ],
  "experiments_run": [
    "Throwaway crate C:\\Users\\vanya\\AppData\\Local\\Temp\\claude\\D--flui\\bbb28042-f972-4a10-8e94-731819e26161\\scratchpad\\probe-writer (own [workspace], CARGO_TARGET_DIR=probe-writer/target, rustc 1.98.1). It models a Graph, a Copy `!Send` Signal and the shapes: A `Fn(&mut Writer<'_>)`, B ambient scope installed by the dispatcher, C TLS realm with the guard only in build, D marker trait accepting `Fn()` or `Fn(&mut Writer)`.",
    "`cargo test --lib` → 5 passed. `b_press_writes_and_outside_scope_is_refused`: shape B refuses a write outside a dispatcher scope (timer or async continuation) with NoScope, so every non-event path needs its own scope. `b_write_during_build_is_runtime_only` and `c_write_anywhere_guard_only_in_build`: B and C catch a build write only when that path actually executes. `a_nested_sync_callback_during_build_still_needs_guard` (should_panic OK): shape A still depends on the runtime guard when a widget invokes a callback synchronously inside build.",
    "`cargo check --example` per case. a1_inline OK. a2_closure_in_let FAIL: \"implementation of `Fn` is not general enough … closure with signature `fn(&'2 mut Writer<'_>)` must implement `Fn<(&'1 mut Writer<'_>,)>` for any lifetime\". a3_closure_in_let_annotated OK. a10_callback_hint (helper fn with Fn bound) OK. a4_write_in_build FAIL E0061 \"this method takes 2 arguments but 1 argument was supplied / help: provide the argument\", a clear, agent-fixable compile error. a5_stash_writer (store `&mut Writer` in `'static` state) FAIL \"lifetime may not live long enough\", as desired. a6_writer_to_thread FAIL (`*const ()` not Send/Sync). a7_changed_bool `Fn(&mut Writer, bool)` OK. a8_async_then (`spawn_then(fut, |w, v| ..)`, `after_ms(.., |w| ..)`) OK. a9_helper_fn passing `w` down to a helper OK.",
    "c1_signal_to_thread: shape C with a `!Send` Signal moved into `std::thread::spawn` FAIL (`*const ()` cannot be sent). The `!Send` handle catches cross-thread writes regardless of shape.",
    "d1_both_untyped: marker-trait dual acceptance, `on_pressed(move || ..)` plus `on_pressed(move |w| s.set(w,1))`. FAIL: \"the trait bound `{closure}: IntoCallback<_>` is not satisfied\" on the untyped `|w|` closure. d2_typed with `|w: &mut a::Writer<'_>|` OK. So a gradual both-signatures transition forces an explicit type annotation on every new-style closure, which is worse than committing to shape A.",
    "e1_owned_token: a Writer passed by value (`Fn(Tok)`) infers in a `let`, but `STASH.with(|s| *s.borrow_mut() = Some(t))` compiles, so the token escapes. This confirms the borrowed-reference requirement.",
    "Repo greps (commands in code_facts): the setter count of 92 with per-signature histogram, about 432 call sites, listener/post_frame/rebuild_handle counts, the signal production-user search, and a read of reactive/mod.rs, state_cell.rs, commands.rs:440-456 and ADR-0078 section 2.",
    "Recommendation (my judgement, for the owner and architect). Choose shape A, framed as an event context in the GPUI style rather than a bare Writer: `Fn(&mut EventCx<'_>, ..)` where EventCx derefs to Writer and carries only realm-level capabilities (spawn, focus request, realm commands), with no tree position, consistent with doc line 438 (`Router::of(w)` rejected). Apply it to event setters only (queries excluded), and deliver the same cx on listeners, post-frame, timers and Spawner continuations. Ship a `callback(|cx| ..)` helper for let-bound closures. Make `StateCell::set/update` take the cx too. Remove `BuildContext::reactive()`. Keep the ADR-0074 guard as the backstop. Reasons: the break is mechanical and signals have no users yet (pre-1.0 cheapest moment); compile errors (E0061 'provide the argument') are far more reliable for agent-generated code than a runtime guard that only fires on executed paths; creating the cx per dispatch fixes the П4 wrong-realm routing by construction; and GPUI and Xilem prove the shape in Rust. Reject shape B: ambient scopes need the same wiring on every path, and failures surface as runtime NoScope. Reject D as a transition: it forces annotations. Shape C is the fallback only if the owner judges ergonomics above agent correctness; it matches Dioxus/SwiftUI/Flutter and needs only the guard, plus a handle-to-realm lookup to fix П4. All ergonomics claims above are measured in the probe; the claims about agent success rates are a hypothesis not measured here."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "opt_a_full_eventcx",
      "name": "A: typed event context on every write path, no escape hatch",
      "description": "Every event callback becomes `Fn(&mut EventCx<'_>, ..)`. EventCx is borrowed, is created per dispatch from the dispatching presentation's realm, and derefs to `Writer`. It carries realm-level capabilities only (spawn, focus request, realm commands) and no tree position. The same cx goes to Listenable and animation-status listeners, post-frame, timers, Spawner continuations and `UiCommand::SignalWrite`. `Signal::set/update/set_if_changed` and `StateCell::set/update` take `&mut Writer`. `BuildContext::reactive()` is removed in the same PR. Query callbacks (route generators, drag will-accept, `-> EventPropagation`, `-> bool`) keep their current shape. `Writer` and the `WriterSource` dispatcher live in `flui-reactive` (tier V), so recognizer aliases in flui-interaction can carry it. The ADR-0074 guard stays as the backstop.",
      "pros": [
        "The build-write mistake becomes a compile error (E0061 'provide the argument', probe a4). That is the most reliable signal for agent-generated code, where a runtime guard fires only on paths that actually execute.",
        "Creating the cx per dispatch fixes the П4 wrong-realm routing by construction (today commands.rs:449-455 writes into the primary presentation's graph). Every write resolves to the realm that dispatched it.",
        "This shape is proven in Rust: GPUI listeners take `(&Event, &mut Window, &mut App)` and Xilem callbacks take `&mut State`. Probes a1, a7, a8, a9 show inline closures, value parameters, async `then` and helper functions all compile.",
        "The borrowed `&mut Writer<'_>` cannot be stored in `'static` state (a5) or sent to another thread (a6). It is a real capability, not a token.",
        "Copy `!Send` signals are captured with plain `move`, so closures carry no clone boilerplate.",
        "Signals have no production users today, so this is the cheapest the change will ever be."
      ],
      "cons": [
        "Breaks about 432 on_* call sites and about 200 listener and post-frame sites. The rewrite is mechanical (`move ||` → `move |_cx|`), but it touches every example, test and template.",
        "HRTB ergonomics: a closure bound with `let` before being passed fails with the opaque 'Fn is not general enough' error (a2). This needs a shipped `callback(|cx| ..)` helper and an agent-guidance line.",
        "There is no escape hatch. Foreign-library callbacks (an OS hook, a third-party crate calling a plain `Fn()`) have to go through `SignalSender`/`UiCommand`, even on the owner thread. That costs a frame of latency and is awkward.",
        "The guard still cannot be removed (probe `a_nested_sync_callback_during_build_still_needs_guard`, and ADR-0075 WrittenDuringCompute), so the typing narrows the mistake space without closing it.",
        "Requires sorting the 92 setters into event and query, one by one."
      ],
      "cost_now": "High but mechanical. Signatures change on about 92 setters (roughly 70 event, the rest query) and on the Listenable, AnimationStatus, post-frame, Spawner and timer APIs. About 630 call sites are rewritten by `flui migrate` or a syntactic codemod. Writer/WriterSource go into flui-reactive (tier V, already planned). Done inside W5, together with the `!Send` flip that has to happen anyway (Signal is already `!Send`, so none of the 23+ `Send + Sync` setters can capture it today).",
      "cost_later": "Low for the core. Some friction later when integrating foreign callbacks, and pressure to add an ambient `Writer::current()`, which would quietly turn the design into shape B.",
      "reversibility": "Reversible in the cheap direction. Dropping the parameter or making `set` not require it later is a mechanical break (or additive, if a no-arg `set` is added). Going from C to A after 1.0 would be the expensive direction.",
      "fits_plan": "Fits W5 exactly (architecture doc lines 262-266, and 560 says to do it together with the `!Send` flip). Fits D-level decisions on realm ownership (П4), P1 and ADR-0078 (no new hooks), and the doc's downgrade of Writer to 'a narrowing, not a replacement for the guard' (line 266, 695)."
    },
    {
      "id": "opt_a_plus_lifecycle_handle",
      "name": "A+: typed event context plus a WriteHandle acquired in init_state",
      "description": "Everything in option A, plus one named escape hatch. `LifecycleContext::write_handle()` returns a same-thread `WriteHandle` (`!Send`, realm-bound). `handle.write(|w| ..)` opens a Writer scope on that handle's own realm. Because it lives on LifecycleContext (ADR-0078), `build` cannot acquire it. A handle stashed in state and called from build is caught by the existing runtime guard. `SignalSender` stays the cross-thread form. Documented use cases: foreign callbacks, platform hooks, and interop with crates that take a plain `Fn()`. The catalog's own widgets never use it, and a clippy `disallowed_methods` entry (or an xtask grep over crates/flui-widgets, flui-material, flui-cupertino) keeps it out of the catalog.",
      "pros": [
        "Keeps every compile-time benefit of A for the paths users and agents actually write (on_* setters, listeners, post-frame, continuations).",
        "Closes A's real gap (foreign `Fn()` callbacks on the owner thread) without an ambient lookup. The capability is still explicit, realm-bound (so П4 stays fixed), and acquired only where ADR-0078 already puts presentation capabilities (`rebuild_handle`, `post_frame_handle`, `async_driver`).",
        "Gives a single answer to 'how do I write from X' that is not NoScope at run time (the failure mode of shape B, probe b_press_writes_and_outside_scope_is_refused).",
        "The same mechanism is what the framework needs internally for `UiCommand::SignalWrite` and Spawner continuations, so it is not a new concept, just the public face of WriterSource.",
        "Matches Flutter's reference behaviour (setState from any callback, timer or future) while keeping the typing wherever the framework controls the callback."
      ],
      "cons": [
        "The same migration cost as A (about 630 sites, event/query classification, the `callback` helper).",
        "One more public type. It can be misused as a universal writer, so its reach has to be policed (a lint or disallowed-method entry in catalog crates, and a guidance line).",
        "A WriteHandle called during build is caught only at run time, the same as today's guard. This is acceptable because the path is opt-in and named."
      ],
      "cost_now": "A's cost plus roughly one small type (`WriteHandle` wrapping realm id + `Rc` to the graph, `write(|w| ..)` which asserts the guard) and one `disallowed_methods` entry for catalog crates. Adds perhaps 1-2 days to the W5 slice.",
      "cost_later": "Lowest of the options. There is no pressure to add an ambient Writer later, because the sanctioned hatch already exists. Its reach can be measured with a grep over dependents.",
      "reversibility": "High. The hatch can be removed or narrowed pre-1.0 if nobody needs it, or marked Evolving in the semver tiering. The typed core has the same reversibility as A.",
      "fits_plan": "Best fit. It honours W5 (`!Send` + signatures together), the realm-owned graph (§4.6, П4), ADR-0078 (capabilities come from LifecycleContext only), the doc line 438 correction (no tree position on Writer; navigation handles are resolved in init_state, as WriteHandle would be), and ADR-0074/0075 (the guard stays authoritative)."
    },
    {
      "id": "opt_b_ambient_scope",
      "name": "B: plain callbacks, Writer obtained implicitly from the dispatch scope",
      "description": "Keep `Fn()`-shaped setters. The dispatcher installs an ambient scope for the realm around each event, listener, post-frame run or continuation. `signal.set(v)` (or `Writer::current()`) resolves the realm from that scope and fails with a runtime NoScope error outside it. The guard stays.",
      "pros": [
        "A smaller signature break: on_* setters keep `Fn()`; only `Signal::set` loses its argument.",
        "The dispatcher's scope can pick the right realm, so П4 can be fixed without a parameter.",
        "Familiar from Dioxus/Leptos-style write-anywhere."
      ],
      "cons": [
        "Every non-event path needs the same wiring as in A (scopes around listeners, post-frame, timers, continuations). The framework-side work is not smaller, only hidden.",
        "Failures surface at run time as NoScope, and only on executed paths (probe b_*). This is the worst combination for agent-written code: it compiles, it looks right, and it fails in a timer at 3 a.m.",
        "The ambient scope is effectively a thread-local, which is the kind of process/TLS state the architecture is removing (§ globals gate, one TLS cell).",
        "It does not stop build writes at compile time (b_write_during_build_is_runtime_only)."
      ],
      "cost_now": "Medium. Scope installation at every dispatch site (the same count as A's wiring) plus a TLS cell. Fewer call-site edits.",
      "cost_later": "High. Runtime NoScope bugs in user code; pressure to make NoScope silently fall back to 'primary realm', which reintroduces П4.",
      "reversibility": "Poor. Moving to typed callbacks after users rely on write-anywhere is the full A break at post-1.0 prices.",
      "fits_plan": "Poor. It conflicts with the TLS-reduction and globals-gate direction and gives up the typed narrowing the doc keeps (line 266). Dominated by C, which is simpler at the same safety level."
    },
    {
      "id": "opt_c_guard_only",
      "name": "C: plain Fn() callbacks, handle-bound realm, runtime guard only",
      "description": "Keep `Fn()`/`Fn(T)` setters and remove the `&Reactive` argument from `Signal::set`. Each Signal handle carries its realm (or its graph `Weak`), so writes go to the owning realm, fixing П4 through the handle rather than the dispatch. Build writes are caught only by the ADR-0074 guard, and compute writes by ADR-0075. `BuildContext::reactive()` is removed anyway. This is Dioxus/SwiftUI/Flutter's model.",
      "pros": [
        "The least churn: only the signatures that must change for `!Send` change. Closures stay `move || count.set(n)`, which is the idiom LLMs reproduce most readily.",
        "No HRTB pitfalls and no helper function.",
        "Writes from foreign callbacks, timers and continuations just work, as in Flutter's setState.",
        "The realm-bound handle fixes П4 without a dispatcher-created context."
      ],
      "cons": [
        "The build-write mistake is caught only when the path executes. Compose and SwiftUI both show that this bug class stays common under convention plus runtime warnings (backwards-write docs, 'Modifying state during view update').",
        "A handle has to carry a realm pointer (a larger Copy handle, or a lookup by id per write).",
        "Gives up the typed narrowing the architecture doc and plan already committed to.",
        "StateCell stays unguarded, the same as today."
      ],
      "cost_now": "Low. Remove the argument from set/update, give Signal a realm id, and do the `!Send` flip on setters.",
      "cost_later": "Medium. If typed writes are wanted after 1.0, it is the full A break at a moment when consumers exist.",
      "reversibility": "Asymmetric. C → A later is the expensive direction; nothing about C is hard to undo pre-1.0, but the window closes at 1.0.",
      "fits_plan": "Partial. It fits `!Send`, П4 and ADR-0074 (the guard is already authoritative), but contradicts the doc's §4.6 Writer line and the stance 'make rules types, not reviews'. A valid fallback if the owner ranks closure ergonomics above compile-time checking."
    }
  ],
  "recommended": "opt_a_plus_lifecycle_handle",
  "rationale": "I recommend A+: a typed `&mut EventCx<'_>` (which derefs to Writer) on every event callback and every framework-driven write path, plus one explicit `WriteHandle` that can only be acquired from `LifecycleContext`. The main reasons:\n\n(1) **Now is the cheapest moment.** Signals have zero production users (grep shows only ui_realm tests), the feature is off by default, and W5 already has to break every `Send + Sync` setter for the `!Send` flip, since Signal is already `!Send` (reactive/mod.rs:648-654). The extra cost of adding a parameter to a signature that is changing anyway is a mechanical codemod over about 630 sites.\n\n(2) **Typing is worth more for agent-written code than for humans.** A build-time write becomes E0061 'provide the argument' (probe a4), which an agent fixes on the first try. Under B or C the same mistake is a runtime error on a path that may never run in tests. Compose's backwards-write docs and SwiftUI's 'Modifying state during view update' warning show that the runtime-only model leaves this bug class alive even with heavy tooling.\n\n(3) **Creating the cx per dispatch from the dispatching realm fixes П4 by construction.** Today commands.rs:449-455 writes into the primary presentation's graph. Doing this in the dispatcher is cleaner than giving every Copy handle a realm pointer (C) or keeping an ambient TLS scope (B, which runs against the globals-reduction direction).\n\n(4) **Pure A has one real hole: foreign `Fn()` callbacks on the owner thread.** Without a sanctioned hatch, pressure builds for `Writer::current()`, which is shape B by the back door. A `WriteHandle` from `LifecycleContext` fills that hole the ADR-0078 way: a capability acquired in init_state, realm-bound, unreachable from build, kept out of the catalog by a disallowed-methods lint. A misuse (stash it, call it in build) is still caught by the guard.\n\nConditions that must ship with it:\n\n- **Classify the 92 setters.** Query callbacks (route generators, drag will-accept, `-> EventPropagation`/`bool`) get no Writer.\n- **Deliver the cx on the non-setter paths too:** Listenable and animation-status listeners, post-frame, timers, Spawner `then` and `UiCommand::SignalWrite`. That is about 160+ sites.\n- **Place the types at tier V.** `Writer`/`WriterSource` live in `flui-reactive`, so flui-interaction recognizer aliases can carry them.\n- **Guard `StateCell` too.** `StateCell::set/update` take `&mut Writer`, or the unguarded path users actually use stays open.\n- **Remove `BuildContext::reactive()`** in the same PR.\n- **Ship a `callback(|cx| ..)` helper** and name it in agent guidance, because a closure bound with `let` hits the opaque HRTB 'Fn is not general enough' error (probe a2).\n- **Keep the ADR-0074 guard authoritative.** Nested synchronous callbacks during build and ADR-0075 compute writes are runtime-only whatever the shape (probe a_nested_sync_callback_during_build_still_needs_guard). The ADR text should call Writer 'a narrowing', consistent with flui-global-architecture.md:266,695.\n\nWhat to reject:\n\n- **Option D, accepting both `Fn()` and `Fn(&mut Writer)` during a transition** (probe d1): new-style closures need explicit type annotations, which is worse than committing to one shape.\n- **B:** it does the same wiring work as A but fails at run time.\n\nC is the documented fallback if the owner weighs closure ergonomics above compile-time checking. It is cheap now but expensive to reverse after 1.0.\n\nHypotheses I have not measured:\n\n- Agent success rates for the typed shape versus the runtime-guard shape.\n- Whether foreign-callback demand is large enough to justify WriteHandle. The fallback is to ship A without the hatch and add it on first need; that is an additive change."
}
```

## judge_q7_callback_writer_engineer

```json
{
  "choice": "opt_a_plus_lifecycle_handle",
  "confidence": 0.7,
  "reasons": [
    "The facts I checked hold. `Signal::set(self, r: &Reactive, ..)` is at crates/flui-view/src/reactive/mod.rs:774 and `set_if_changed` at :793. `StateCell::set`/`update` at crates/flui-view/src/state_cell.rs:228,245 have no capability parameter. So writes already take an argument today, but anyone can obtain that argument. Making it a borrowed, per-dispatch `&mut Writer<'_>` tightens a contract that already exists; it does not invent a new one.",
    "Pure A is less feasible than the architect costed, and that makes A+ necessary rather than optional. `Listenable::add_listener(ListenerCallback)` is declared in flui-foundation (crates/flui-foundation/src/notifier.rs:80), the lowest layer, and flui-rendering uses `Arc<dyn Fn() + Send + Sync>` listeners (crates/flui-rendering/src/view/viewport_offset.rs:117, scroll_position.rs:911). Focus listeners live in flui-interaction (focus.rs:440, focus_scope.rs:679). Threading an EventCx through every Listenable would push Writer into flui-foundation or force a second listener trait at every level. A realm-bound `WriteHandle` acquired in init_state covers the listener, controller and foreign-callback paths without that layering surgery. Without it, pure A either stalls on listeners or grows an ambient `Writer::current()`, which is shape B by the back door.",
    "Timing is right. Signals have no production users and are behind `signals = []`, and the `!Send` flip already breaks the `Send + Sync` setters in W5. Adding a parameter to signatures that are changing anyway costs a codemod, not a redesign. Going from C to A after 1.0 is the expensive, irreversible direction.",
    "For agent-written code, the probes show a build-time write surfacing as E0061 'provide the argument' (a4), which is deterministic. B and C fail only on paths that actually execute. Compose's backwards-write docs and SwiftUI's 'modifying state during view update' warning show that runtime-only enforcement leaves this bug class alive.",
    "Creating the cx per dispatch from the dispatching realm fixes the wrong-realm write in crates/flui-app/src/app/ui_realm/commands.rs:449-455 (writes land in the primary presentation's graph) by construction. WriteHandle is realm-bound in the same way, so the hatch does not reopen that defect.",
    "I reject B: it needs the same wiring as A but fails at run time with NoScope (probe b_*), and it adds TLS against the globals-reduction direction. I reject D: the probe d1 marker-trait transition forces a type annotation on every new-style closure. C is the only credible alternative, and it loses mainly on irreversibility and on 'make rules types, not reviews'."
  ],
  "conditions": [
    "Scope the typed cx to callbacks the framework dispatches from widget or interaction events: on_* event setters, recognizer callbacks, post-frame and Spawner/async `then` continuations. Do not thread EventCx through flui-foundation `Listenable`/`ChangeNotifier` or the flui-rendering `ViewportOffset` listeners in W5. Those paths use WriteHandle, or a widget-level adapter that opens a Writer on the realm's handle. Record this boundary in the ADR so the scope is not over-claimed.",
    "Keep EventCx minimal at first, meaning a Writer plus a realm id. Add spawn, focus and command capabilities only when a concrete setter needs them. Otherwise it duplicates LifecycleContext capabilities (ADR-0078) and grows into a second context type with unclear rules.",
    "Where Writer lives is decided before code: either the new tier-V `flui-reactive` exists first, or recognizer aliases stay `Fn(Details)` and flui-widgets wraps them and creates the Writer. `cargo xtask workspace` must pass either way.",
    "Classify the setters as event (gets a cx) or query (does not) explicitly, in the PR description. Query means route generators, drag will-accept, and anything returning `-> EventPropagation` or `-> bool`. A query callback must never receive a Writer.",
    "`StateCell::set`/`update` take `&mut Writer` in the same W5 change. Otherwise the unguarded path the examples actually use (examples/counter.rs) stays open and the typing protects nothing users touch. `BuildContext::reactive()` and the public `BuildOwner::reactive()` exposure (crates/flui-view/src/owner/build_owner.rs:973) go in the same PR.",
    "Ship the `callback(|cx| ..)` helper together with the signature change, and put one line about it in agent guidance, because let-bound closures hit the opaque HRTB 'Fn is not general enough' error (probe a2). Add a compile_fail doctest pinning that a Writer cannot escape into `'static` state (a5), and one pinning that `build` has no route to a Writer.",
    "WriteHandle is `!Send`, bound to one realm and obtainable only from LifecycleContext. Its `write(|w| ..)` still goes through the ADR-0074 guard, and a test must show a stashed handle called from build returns `WrittenDuringBuild`. Keep it out of catalog crates with a clippy `disallowed_methods` entry and mark it Evolving in the semver tiering.",
    "The ADR-0074/0075 runtime guard stays authoritative. The ADR text calls Writer a narrowing, not an enforcement, consistent with flui-global-architecture.md:266,695 and probe a_nested_sync_callback_during_build_still_needs_guard.",
    "Land it as staged PRs, not one sweep. First the types, the helper and WriteHandle behind the `signals` feature. Then a setter-signature codemod per crate with `cargo xtask check-changed` green each time. Then remove the old `&Reactive` API.",
    "Unverified: agent success rates for the typed shape, the exact call-site count (the ~630 figure is an upper-bound grep), and how many listener paths need WriteHandle versus an adapter. Re-measure after the classification pass. If WriteHandle turns out to be used in most user code, reconsider C."
  ]
}
```

## judge_q7_callback_writer_ecosystem_author

```json
{
  "choice": "opt_a_plus_lifecycle_handle",
  "confidence": 0.7,
  "reasons": [
    "App authors get a compile-time error at the most common mistake site: probe a4 shows a write in build fails with E0061 'provide the argument', which is a local, self-explaining fix. Under B and C the same mistake only shows up at run time, on a path a test may never execute. That matters most for agent-written app code, which is a large share of FLUI's expected H0-H2 users.",
    "Signals have no production users today, per the research grep (only crates/flui-app/src/app/ui_realm/tests/mod.rs). Signal is already !Send (crates/flui-view/src/reactive/mod.rs:648-654), so W5 has to break the 23+ Send + Sync setters anyway. Adding the cx parameter in the same break costs little extra per call site now. After 1.0, moving from C to A would break every third-party package at once.",
    "Package and plugin authors (H3-H4) are the users pure A leaves stranded. They integrate foreign plain-Fn() callbacks: OS hooks, third-party crates, platform channels. A named, realm-bound WriteHandle acquired from LifecycleContext follows the ADR-0078 pattern those authors already use for rebuild_handle, post_frame_handle and async_driver. It also removes the pressure for an ambient Writer::current(), which would bring shape B back in by the back door.",
    "Creating the cx per dispatch fixes the wrong-realm write in crates/flui-app/src/app/ui_realm/commands.rs:449-455 by construction. That matters for multi-window apps, and for plugins whose widgets can be mounted in a non-primary presentation.",
    "GPUI (`Fn(&Event, &mut Window, &mut App)`) and Xilem (`&mut State` first) show that Rust UI users accept a context parameter on every callback. It is idiomatic Rust, not an imposition. The main DX cost is the HRTB 'Fn is not general enough' error on let-bound closures (probe a2), and a helper fixes it (probe a10).",
    "B is dominated: it needs the same wiring as A, adds a TLS scope, and fails at run time with NoScope. C is the honest fallback, but it gives up the typed narrowing at exactly the moment it is cheapest to have."
  ],
  "conditions": [
    "Third-party widget authors need a public, documented way to create or forward an EventCx when their own widget invokes a user callback. For example, a recognizer callback receives the cx and passes it down, or the widget calls WriteHandle::write(|cx| user_cb(cx, v)). If only catalog-internal code can build a cx, packages cannot ship on_* setters with the same shape as flui-widgets. Pin this with an out-of-tree example or test widget that defines its own on_changed and does not rely on any pub(crate) item.",
    "flui-testing has to expose a way to run a callback with a test EventCx (for example tester.with_cx(|cx| ..)), so package authors can unit-test their callbacks without a full dispatch.",
    "Keep EventCx minimal and mark it Evolving in semver tiering. Start it as Writer plus realm-level spawn and focus-request only, with no tree position, per flui-global-architecture.md:438. Every capability added later is a breaking-surface decision for package authors.",
    "Ship the callback(|cx| ..) helper in the same PR. Add a rustc-diagnostic hint or doc alias that points at it from the HRTB error, and name it in AGENTS.md or the agent guidance.",
    "Add a `flui migrate` codemod (`move ||` → `move |_cx|`, `|v|` → `|_cx, v|`) covering examples, flui-cli templates and tests, and run it over all about 630 sites in the W5 PR. Call-site churn must not be hand-edited.",
    "Classify every one of the 92 setters as event or query in a table checked into the PR. Query callbacks (route generators, drag will-accept, `-> EventPropagation`/`bool`) get no cx.",
    "Deliver the cx on the non-setter write paths too: Listenable and animation-status listeners, post-frame, timers, Spawner continuations and UiCommand::SignalWrite. Otherwise app authors hit a missing Writer in timers and async continuations, which Flutter and SwiftUI practice shows are routine write sites.",
    "StateCell::set/update take the Writer too, or the guarded path is bypassed by the API users actually use (crates/flui-view/src/state_cell.rs:228-231). BuildContext::reactive() is removed in the same PR.",
    "WriteHandle stays !Send and realm-bound, and a disallowed_methods entry keeps it out of flui-widgets, flui-material and flui-cupertino. SignalSender remains the only cross-thread form.",
    "The ADR-0074 runtime guard stays authoritative, and the new ADR calls Writer 'a narrowing'. Nested synchronous callbacks during build, and ADR-0075 writes during compute, stay runtime-only.",
    "Fallback trigger: if a trial port of 2-3 examples plus one third-party-style package shows the cx parameter or the helper is routinely confusing (for example, most agent attempts hit the HRTB error), revisit C before 1.0, while the reverse move is still cheap.",
    "Hypothesis, not measured: that agents succeed more often with the typed shape than with guard-only, and how much real demand there is for WriteHandle. Neither was measured in this run."
  ]
}
```

## judge_q7_callback_writer_owner

```json
{
  "choice": "opt_a_plus_lifecycle_handle",
  "confidence": 0.62,
  "reasons": [
    "Timing: signals have no production users (only crates/flui-app/src/app/ui_realm/tests/mod.rs), the feature is off by default (crates/flui-view/Cargo.toml:123), and W5 already breaks every `Send + Sync` setter for the `!Send` flip, because Signal is already `!Send` (reactive/mod.rs:648-654). Adding the parameter now costs a codemod. After 1.0 it is the expensive C-to-A direction, and before 1.0 A is the only direction that stays cheap to reverse.",
    "For a sole owner whose code is mostly written by agents, compile errors are the cheapest review. Probe a4 turns a write during build into E0061 'provide the argument', which fits the 'make rules types, not reviews' stance in AGENTS.md. B and C fail only on paths that actually execute, and Compose's backwards-write docs and SwiftUI's 'Modifying state during view update' warning show that bug class survives convention plus runtime warnings.",
    "Creating the cx per dispatch from the dispatching realm fixes the wrong-realm write in commands.rs:449-455 by construction. C would need a realm pointer in every Copy handle; B would need an ambient TLS cell, against the globals-reduction direction.",
    "The WriteHandle hatch is small, about one type plus a disallowed_methods entry. It is the public face of the WriterSource that Spawner continuations and UiCommand::SignalWrite need internally anyway, and it follows ADR-0078 (acquired only on LifecycleContext). Without it, the first foreign `Fn()` callback creates pressure for `Writer::current()`, which is shape B by the back door. That drift would be harder for a single maintainer to police than one named, lint-fenced type.",
    "I rejected B because it is dominated: it needs the same wiring as A but fails at run time with NoScope (probe b_*). I rejected D because a dual-signature transition forces type annotations (probe d1).",
    "Why confidence is only moderate: the break is large (about 630 sites) for a single maintainer's WIP budget. The case that typing improves agent correctness is a hypothesis, not measured. Pure A with the hatch added on first need is almost as good, since adding it later is additive; the difference between A and A+ is small."
  ],
  "conditions": [
    "Sequencing and WIP: do this only inside W5, together with the `!Send` flip, as a series of reviewable slices. First, Writer/WriterSource/EventCx land in flui-reactive with the `callback` helper. Second, the Signal and StateCell set/update signatures change and `BuildContext::reactive()` is removed. Third, the setters and listener paths migrate crate by crate through `flui migrate` or a codemod. Hand rewrites are not acceptable for about 630 sites. Keep at most one slice open at a time.",
    "Before the setter PR, publish the classification of the 92 on_* setters into event and query as a table in the ADR, with the grep command that produced it. Query callbacks get no cx: route generators, drag will-accept, and anything returning `EventPropagation` or `bool`.",
    "Unverified, check before committing: that flui-reactive (tier V) sits at or below the layers of both flui-interaction (recognizer `Rc<dyn Fn(Details)>` aliases, tap.rs:91, drag.rs:113-121) and the owner of Listenable/AnimationStatus listeners. Confirm with `cargo xtask workspace` layer metadata. If Listenable lives below tier V, decide whether listeners get the cx or go through WriteHandle, and record that choice.",
    "`StateCell::set/update` must take `&mut Writer`. Otherwise it must be documented as the unguarded tier and excluded from the typing claim. Leaving it silently open voids the benefit, because it is the path users actually write (examples/counter.rs:55).",
    "ADR-0074 stays the authoritative enforcement, and ADR-0075 stays authoritative for compute writes. A new ADR calls Writer/EventCx 'a narrowing, not a replacement for the guard' and keeps a test like a_nested_sync_callback_during_build_still_needs_guard that proves the guard still fires.",
    "EventCx carries realm-level capabilities only (spawn, focus request, realm commands) and no tree position. It must not grow into a second BuildContext. Any addition needs an ADR note.",
    "WriteHandle is acquired only from LifecycleContext, is `!Send`, and is bound to one realm. A `disallowed_methods` entry, or the equivalent xtask gate wired into `checks`, keeps it out of flui-widgets, flui-material and flui-cupertino. A test must show a stashed handle called from build hits WrittenDuringBuild. If no foreign-callback use appears before the API tiering freeze, mark it Evolving or drop it.",
    "Ship the `callback(|cx| ..)` helper in the same PR as the signature change. Add one line to AGENTS.md or the widget-authoring doc naming the helper as the fix for the 'Fn is not general enough' HRTB error (probe a2).",
    "The fallback trigger is written down in advance. If the codemod pilot on one crate, for example flui-cupertino with 9 call sites, shows that the ergonomic cost or the HRTB friction in examples and templates is materially worse than the probe suggests, switch to C (handle-bound realm plus the guard) before 1.0, not after."
  ]
}
```

## verify

```json
{
  "holds": false,
  "problems": [
    {
      "problem": "The cost estimate leaves out the gesture-arena protocol. Recognizer callbacks cannot receive `&mut EventCx` without changing the sealed arena trait and the public arena API. The fallback the engineer judge proposed (keep `Fn(Details)` and have flui-widgets wrap it to open a Writer) needs a Writer source that takes no parameter inside catalog crates, which contradicts the condition that the catalog never uses WriteHandle.",
      "evidence": "User callbacks fire from `GestureArenaMember::accept_gesture(&self, pointer)`, `reject_gesture` and `poll_deadline(&self)` (crates/flui-interaction/src/arena/mod.rs:139-155), with a blanket impl over the public extension trait `CustomGestureRecognizer` (mod.rs:195). `GestureArenaEntry::resolve(&self, disposition)` (mod.rs:396) dispatches synchronously: `resolve_entry` calls `Self::dispatch_pending` (mod.rs:1150-1165). `GestureArena::poll_deadlines(&self)` (mod.rs:1513-1522) runs members under `catch_unwind` without any context parameter. The aliases are `Rc<dyn Fn(TapDetails)>` and similar (tap.rs:91, drag.rs:113-121, long_press.rs:33-42). The option's roughly 630-site / 92-setter cost counts none of this.",
      "severity": "major",
      "fix": "Decide how the cx reaches recognizers before W5 and put it in the cost. Either thread `&mut WriterSource`/cx through `accept_gesture`/`reject_gesture`/`poll_deadline`/`resolve`/`accept`/`sweep`, which is a sealed-trait and custom-recognizer break and requires flui-interaction (layer 2) to depend on flui-reactive, or make the wrapping mechanism an explicit, public `WriterSource` that catalog and third-party widgets are allowed to use. In the second case, drop the 'catalog never uses the hatch' fence or narrow it to `LifecycleContext::write_handle` only."
    },
    {
      "problem": "The claim that A fixes П4 wrong-realm routing 'by construction', and that C would need 'a larger Copy handle', is contradicted by the code. The Signal handle already carries its graph id and cross-realm writes are already rejected at run time. So П4 can be fixed by routing `UiCommand::SignalWrite` through `slot.graph`, with no typed cx. A typed cx also gives no compile-time realm safety: a signal from window A written with window B's cx still fails only at run time. This removes rationale point (3) and one of the listed cons of C.",
      "evidence": "`pub struct SignalSlot { graph: u32, index: u32, generation: u32 }` (crates/flui-view/src/reactive/mod.rs:78-82). `fn check` returns `SignalError::ForeignGraph` when `slot.graph != self.id` (mod.rs:267-273). The П4 site takes the primary realm's `owner.reactive()` and calls `apply(&reactive)` (crates/flui-app/src/app/ui_realm/commands.rs:449-455). The fix there is to look up the realm by the slot's graph id, and that fix is independent of the callback shape.",
      "severity": "major",
      "fix": "Rewrite the ADR rationale so that A's advantage over C rests only on compile-time rejection of build writes. Fix П4 separately, and first, by resolving the target graph from `SignalSlot.graph` in commands.rs, with a failing multi-window test. Remove the 'larger Copy handle' con from C."
    },
    {
      "problem": "Most of WriteHandle's documented use cases cannot compile today. The hatch is `!Send`, but the platform hooks, Listenable, animation-status and post-frame callback types all require `Send` (and some `Sync`). A WriteHandle cannot be captured in any of them. Platform hooks keep the `Send` bound until the separate per-backend W1b work, so there SignalSender is the only route regardless. The gap A+ claims to close ('foreign Fn() on the owner thread') is therefore small and unmeasured, which supports the panel's own fallback: ship A, and add the hatch additively on first need.",
      "evidence": "crates/flui-platform/src/traits/platform.rs:319, 523, 530, 542, 547, 550, 698 are all `Box<dyn Fn../FnMut.. + Send>`. The architecture doc (line 243) keeps these `Send` until W1b. crates/flui-foundation/src/notifier.rs:46,78 has `ListenerCallback = Arc<dyn Fn() + Send + Sync>` and `trait Listenable: Send + Sync`. crates/flui-animation/src/animation.rs:11 has `StatusCallback ... + Send + Sync`, and crates/flui-scheduler/src/frame.rs:729 has `PostFrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>`. Probe scratchpad/probe-q7-verify (`cargo check`) fails on `add_listener(Arc::new(move || h.write(|w| s.set(w,1))))` with E0277: `Rc<()>` and `*const ()` cannot be sent or shared between threads safely.",
      "severity": "major",
      "fix": "Either drop WriteHandle from the W5 scope (ship A and add the hatch additively when a concrete same-thread `!Send` foreign callback appears), or make its introduction depend on the Listenable, animation and post-frame `!Send` flip landing in the same slice. Remove 'platform hooks' from its documented use cases until W1b removes `Send` from the platform callbacks. Do not land WriteHandle 'first, behind the signals feature' as the engineer judge's staging suggests, because nothing could use it then."
    },
    {
      "problem": "The condition that `StateCell::set/update` take `&mut Writer` conflicts with what StateCell is. It already is a capability acquired in init_state and used to write from anywhere, which is the WriteHandle pattern. It is also usable while unbound. Plain `Cell`/`RefCell` fields in state plus `rebuild_handle.schedule()` remain a second write path that is unguarded and cannot be typed. So the typed narrowing covers Signal only, and the claim of closing 'the unguarded path users actually use' is overstated.",
      "evidence": "`pub fn bind(&self, ctx: &dyn LifecycleContext)` is at crates/flui-view/src/state_cell.rs:189. `pub fn set(&self, value: T) { self.value.set(value); self.schedule(); }` is at state_cell.rs:228-231, and the unbound doctest `let count = StateCell::new(0); count.set(5);` is at state_cell.rs:221-226. examples/counter.rs:42 calls `self.count.bind(ctx)` and :55 uses `move || count.update(|n| n + 1)`. StateCell appears in 81 places across 11 files, including the flui-cli template (crates/flui-cli/src/templates/counter.rs).",
      "severity": "minor",
      "fix": "In the ADR, state that StateCell and RebuildHandle are the existing runtime-guarded write-anywhere tier (the ADR-0074 guard, or a build-time guard added to `StateCell::schedule`), rather than requiring `&mut Writer` on an unbound cell. Limit the typing claim to Signal writes."
    },
    {
      "problem": "The event/query classification misses callbacks that the catalog itself invokes from `build`. Under A such a callback cannot be given a cx, because build has no route to a Writer. The widget has to be restructured first, and that work is not in the cost.",
      "evidence": "`AnimatedStateSize::build` calls the user's `on_end()` in a loop inside `fn build` (crates/flui-widgets/src/animated/animated_size.rs:201-209). The `on_end` setter is `impl Fn() + 'static` (animated_size.rs:116).",
      "severity": "minor",
      "fix": "Add a column to the setter classification table for the dispatch site (event dispatch / listener / post-frame / build). Before the signature change, move build-time invocations such as AnimatedSize.on_end to a status listener or post-frame callback, with a test that fails if on_end still runs inside build."
    },
    {
      "problem": "The two policing and tiering mechanisms the option relies on do not exist in the repository. There is one workspace clippy.toml and no per-crate clippy configuration, so a `disallowed_methods` entry would also lint flui-view, flui-app and the examples, not only the catalog. There is also no semver-tier mechanism to mark WriteHandle 'Evolving'.",
      "evidence": "The only clippy config is D:\\flui\\clippy.toml (`find crates -name clippy.toml` returns nothing), and it contains only the allow-*-in-tests keys. `grep -rl Evolving` over *.md/*.rs/*.toml returns nothing, and neither Cargo.toml nor tools/xtask mentions cargo-semver-checks. Hypothesis, based on clippy's documented lookup: a per-crate clippy.toml replaces the root file rather than merging with it, so adding one would drop the allow-unwrap-in-tests settings.",
      "severity": "minor",
      "fix": "Enforce the fence with an xtask check folded into `cargo xtask checks`, as the owner judge's condition already allows. Replace 'mark Evolving' with a concrete mechanism (a doc(cfg)/feature gate or an `#[doc]` stability note that a checks gate verifies), or drop the claim."
    }
  ]
}
```
