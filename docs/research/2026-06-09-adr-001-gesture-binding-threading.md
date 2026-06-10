# ADR-001 Decision Research: Gesture Binding Threading Model

**Date:** 2026-06-09 · **Feeds:** [`crates/flui-interaction/docs/ADR-001-gesture-binding-threading-model.md`](../../crates/flui-interaction/docs/ADR-001-gesture-binding-threading-model.md) · **Scope:** `flui-foundation`, `flui-interaction`, + 5 binding-defining crates
**Method:** two multi-agent research passes (19 agents each: 5 codebase mappers + 6 external researchers + 5 decision lenses + 2 adversarial critics + 1 synthesis). Every load-bearing code claim below was re-verified by hand against the worktree — `file:line` cited.

---

## Verdict

**The original ADR frames this as "A vs B vs C, and B is the real latency lever." That framing is wrong on the axis. The decision is not about latency and it is not, at root, about the threading model. It reduces to one product/architecture question that the code cannot answer:**

> **Is "a losing recogniser cannot fire `on_*` callbacks" required to be a _compile error_, or is a _tested runtime invariant_ enough?**

- **Runtime invariant is enough → Option A wins outright.** Lift `tap.rs`'s existing accept-flag discipline into one shared capability gate, fix the two recognisers that violate it, wire the dead deadline driver. Ships now, recogniser-by-recogniser, **zero `flui-foundation` change**.
- **Compile-time guarantee is mandatory → Option A and C are structurally disqualified, and Option B becomes _necessary but not sufficient_.** B (single-thread-affine, `!Send` recognisers) is a prerequisite for a consumable owner, but the guarantee _also_ requires redesigning the arena off `Arc::ptr_eq` winner-identity — which no option on the table proposes. B alone buys a better failure mode (deterministic `RefCell` panic instead of silent deadlock) and ecosystem-correct platform fit, **not** the type-level guarantee by itself.

**Latency is a non-factor.** The gesture hot path is single-producer and uncontended; the already-shipped −54.6 % atomic win is ~4.7 µs/frame, < 0.03 % of a 16.67 ms budget. "Beat Flutter on latency via the threading model" is not defensible and should be struck from the ADR's rationale. If B is ever adopted, it is justified on **soundness and platform-correctness alone, or not at all.**

**Recommended sequence (each phase has standalone value, none blocks other work):**
1. **Phase 0 — fix the live dual-binding split-state bug now** (verified below; ~5 lines; no `Send+Sync` change).
2. **Phase 1 — converge callback discipline** behind one shared capability gate + wire the dead deadline driver into the frame loop (fixes held-finger long-press). This is the typestate redesign's *runtime floor* and ships under Option A.
3. **Phase 2 — adopt Option B only if compile-time single-delivery is ratified as a hard requirement**, and only as part of an arena-identity redesign — scheduled as one foundation-wide migration, not smuggled into a parity PR.

Ratify the enforcement-strength question with chief-architect + product-steward **before** funding Phase 2.

---

## 1. Corrections to ADR-001 and ARCHITECTURE.md (all hand-verified)

The research contradicts five load-bearing claims in the current docs. Correct these before either document is cited to settle anything.

| # | Current claim | Corrected finding | Evidence (verified) |
|---|---|---|---|
| **C-1** | ADR-001 fact #3: the `GestureTimerService` tokio mode "can fire a deadline callback on a worker thread" — a live justification for `Send+Sync`. | **The async-timer path is dead.** `run_async` / `run_until_shutdown` / `check_timers` have **zero** non-definition, non-test callers workspace-wide. Recognisers resolve deadlines by inline `Instant::now()` polling. | grep: only `timer.rs` matches; `long_press.rs:415-418` inline poll |
| **C-2** | ARCHITECTURE.md:70: `GestureTimerService` uses `tokio::sync::Mutex` + `OnceLock`. | **Wrong lock and cell type.** Code uses `parking_lot::Mutex` + `once_cell::sync::Lazy`. And no recogniser uses `GestureTimerService` at all. | `timer.rs:72` (`parking_lot::Mutex`), `timer.rs:424` (`once_cell::sync::Lazy`) |
| **C-3** | Implied: "loser cannot fire" is upheld, only *latent* after the C3 fix. | **Actively violated in shipped code on live single-pointer paths.** `long_press.rs` fires `on_long_press_start` (`:434-437`) before/without arena confirmation; `tap_and_drag.rs` makes `resolve_pointer(Accepted)` and `accept_gesture` documented **no-ops** while firing off its private FSM (`:593-621`); only `tap.rs` gates on a runtime flag (`:513`). Three recognisers, three disciplines. | `long_press.rs:427-452`; `tap_and_drag.rs:593-621`; `tap.rs:504-532` |
| **C-4** | The binding "must be `Sync` because of cross-thread access." | **Split the claim.** Binding-struct `Sync` is *incidental* — forced by `OnceLock<T>: Sync` + the `BindingBase: Sized + Send + Sync + 'static` supertrait, **not** by any executing cross-thread field access (`GestureBinding` is not even in the `AssertSendSync` list). Only the *arena + recogniser* `Send+Sync` is load-bearing, and only via the now-dead timer path. | `flui-foundation/src/binding.rs:106`; `flui-interaction/src/lib.rs:336-371` |
| **C-5** | (Not addressed.) | **NEW — live dual-binding split-state bug.** Two distinct `GestureBinding` allocations exist: the owned `AppBinding.gestures` (`GestureBinding::new()`, drives all production input) and the `GestureBinding::instance()` global (init-throwaway + a hit-test accessor). Two arenas; registrations and sweeps can diverge. | owned: `flui-app/src/app/binding.rs:127,294,456,515`; global: `flui-app/src/bindings/renderer_binding.rs:273,317` |

What the ADR got **right** and survives: the per-event lock/atomic edifice *is* a consequence of the `&'static Send+Sync` decision; the arena *does* store `Arc<dyn GestureArenaMember + Send + Sync>`; and Option C *does* converge on A's cost profile.

---

## 2. Scorecard

Two independent lens passes (one without external grounding, one with) produced consistent scores. Merged, 1–10, higher = better on that lens:

| Lens | A (`&'static` Send+Sync) | B (UI-affine, `!Send`) | C (hybrid: Send+Sync at arena boundary only) | Weight |
|---|---|---|---|---|
| **Latency / per-event cost** | 6–7 | 6–7 | 5–7 | **lowest** (all frame-budget-equivalent) |
| **Soundness / typestate** | 3–4 | **9** | 5 | **high** |
| **Ripple / migration cost** | **8** | 3–4 | 6 | **high** |
| **Platform / portability** | 3 | **9** | 5–6 | high |
| **Ecosystem / threading-model fit** | 4 | **9** | 7 | medium |

**How to read it.** Latency is the lowest-weight lens and should not drive the decision — the spread there is microbench-only. The decision rides on **Soundness** and **Platform** (B dominates) traded against **Ripple** (A dominates, B is worst). The headline soundness gap (B 9 vs A 3) is **real but conditional**: it only materialises if compile-time single-delivery is a hard requirement *and* the arena is redesigned off `Arc::ptr_eq` (see §3). Absent that, B's soundness advantage collapses to "better failure mode" (panic vs deadlock), worth perhaps 5–6.

---

## 3. The decisive technical finding (hand-verified)

Both research passes converged, after adversarial review, on a single fact that reframes the entire decision:

**A consuming `accept_gesture(self) -> AcceptedGesture` transition — the thing that would make "a loser fires a callback" a compile error — is unrepresentable at the arena trait-object boundary under _every_ option, A, B, and C alike.**

Verified mechanics:

1. The arena stores members as `Arc<dyn GestureArenaMember>` and disambiguates the winner among N competitors by **pointer identity over shared borrows**: `self.members.retain(|m| !Arc::ptr_eq(m, member))` and the winner loop `.clone()` members into the pending-notification list — **never moved out** (`arena/mod.rs:344,381,385`, internal `resolve(&mut self, winner: Option<Arc<dyn …>>)`).
2. The recogniser stashes a `Weak<dyn GestureArenaMember>` to that **same allocation** and upgrades it in `accept_tracked(&self)` to re-enter `resolve` (`recognizer.rs:91,206,216`).
3. Therefore at the instant of resolution every recogniser is **irreducibly multiply-aliased** — arena `members` Vec + the upgradeable `Weak` + the `&self` call frame. There is no unique owner to consume. `team.rs` already pays this tax: its only `&mut self` accept path is forced behind `Arc<Mutex<CombiningMember>>` via a `&self` wrapper precisely because the arena trait is `&self`-only (`team.rs:238-252`).

**What this does to each option:**

- **Option A / C:** winner identity lives in a runtime `Arc::ptr_eq` + `is_resolved` bool. Single-delivery can be a capability *token* that is type-*guided* but not type-*guaranteed* — the arena can still mint twice or accept a stale token; the runtime guard stays load-bearing. **Compile-time guarantee: unreachable.**
- **Option B:** makes the *binding* thread-affine and lets the *recogniser-internal* FSM be `Rc<RefCell<State>>`. The consuming witness must be **threaded out of the `&self` arena method** (`state.transition_to_accepted()` returns an `AcceptedGesture` into the caller's owned scope), *not* produced by consuming the trait object. This narrows the guarantee to the **callback-firing surface** (the actual defect) — but note `Rc<RefCell<State>>` still yields `&mut` via a runtime borrow, not ownership, so even under B the *true* compile-time-consumed form needs the arena to stop holding the `Arc` for `ptr_eq` (i.e. an **arena-by-ID redesign**). **B is necessary but not sufficient.**

**Consequence:** the "B enables the typestate, A doesn't" claim in the ADR is half-true. B is a *prerequisite* for a consumable owner; it does not *by itself* deliver the compile-time guarantee. The full guarantee is an arena-identity redesign that B unlocks but does not contain — and that redesign is explicitly out of scope for the current typestate brief.

---

## 4. Does the typestate redesign force the threading decision?

**Conditionally yes — and the condition is a product decision, not a code fact.**

- If single-delivery must be a **compile error**: `&self`-aliased `Arc<dyn>` cannot carry a consuming transition (every arena trait method is `&self`: `accept_gesture`/`reject_gesture` at `arena/mod.rs`, `did_exceed_deadline` at `long_press.rs`). A and C are disqualified; B is the entry ticket (plus the arena-by-ID redesign of §3).
- If single-delivery need only be a **tested invariant**: A wins trivially — lift `tap.rs`'s flag discipline into a shared gate, fix the two violators, done; no foundation change.

The codebase does **not** establish compile-time enforcement as a requirement, and the "three recognisers kept consistent by a shared gate + tests" alternative does the same job at a fraction of B's cost. So the genuinely open, blocking question is **enforcement strength**, and it is upstream of A/B/C. Resolve it first; it either dissolves the threading debate (runtime → A) or escalates it into a larger, separately-scoped arena-redesign ADR (compile-time → B + by-ID arena).

---

## 5. External evidence (what the ecosystem actually does)

The platform/ecosystem lens — grounded in the second pass — is the strongest *non-conditional* argument for B, and it is about **correctness-of-fit**, not latency:

- **Every retained-mode Rust GUI surveyed is `!Send`-by-default:** GPUI (`App: !Send + !Sync`), Druid / Masonry / Xilem (cross-thread only via `EventLoopProxy` + a `UserEvent`), iced (`Subscription: !Send`), Floem (`Rc` default), Dioxus (`RefCell` default). **egui is the lone `Send+Sync` outlier — and its issue #1379 is structurally identical to flui's C3 deadlock.** flui currently sits on the egui side of that line.
- **winit forces it:** `EventLoop` is `!Send + !Sync`; macOS/iOS enforce main-thread UI at the OS level (`with_any_thread` is unavailable there); wasm has no threads. Events already arrive on one thread — so Option A's `Send+Sync` is a tax paid for concurrency **no supported platform delivers** on the gesture control plane.
- **Flutter parity is a category error, not a point for B.** Flutter's safety rests on Dart **isolate** isolation (no shared heap) — a *stronger* invariant than Rust `!Send`-on-one-thread. flui's renderer is already `Arc<Mutex<Renderer>>`, a sharing topology Flutter structurally cannot have. So "reproduce Flutter's lock-free gesture model" would create an *inconsistency* (locked render path + lock-free gesture path), not fidelity. **Adopt B for Rust soundness, not for mimicry.**

Net: the ecosystem says a latency-sensitive native Rust UI is `!Send`-single-thread by default, and flui's `Send+Sync` gesture stack is the unusual choice. That is a real argument for B's *direction* — but it is a *fit* argument, decided as a storage/affinity question, not a reason the typestate work must bundle it.

---

## 6. Steelman — Option B, for and against (narrowed by adversarial review)

**For (survives scrutiny):**
- Eliminates the C3 re-entrancy deadlock *class*: with no lock reachable from member code, re-entrancy degrades from a silent hang to at most a deterministic `RefCell` double-borrow panic (and Flutter's snapshot-before-iterate `copiedGlobalRoutes` pattern makes even that unreachable by construction).
- Only option that gives the recogniser-internal FSM a consumable owner (necessary condition for the type-level guarantee, per §3).
- Ecosystem- and platform-correct (§5); structurally collapses the dual-binding bug (one owner cannot diverge into two arenas).

**Against (also survives — these narrow B's case):**
- **The compile-time guarantee is overstated** unless the arena is also redesigned off `Arc::ptr_eq` (§3). B alone ⇒ runtime `RefCell` underneath a type-guided witness.
- **B can *add* deadline latency.** Today deadlines resolve inline on the UI thread — zero hops. A "timer posts back" lane (worker → mpsc → `EventLoopProxy` → drain) risks **up to one frame** of added long-press/double-tap latency *if drained on the post-frame lane* — a regression in the one dimension that actually matters (vsync-on-input ≈ 16.7 ms), to remove a lock costing < 0.03 %. Mitigation: drain on the **microtask** lane, and don't build the lane at all until a real timer driver exists (it's dead today).
- **Foundation ripple is an ownership rewrite with a silent-correctness trap, not a rename.** `thread_local`'s `is_initialized()`/`check_instance()` semantics flip from global to **per-thread** (breaks init-on-A-assert-on-B, including the spawned re-entrancy test); `thread_local` Drop is a no-op on the main thread at process exit, so a resource-owning binding cannot use it — forcing an owned-on-stack model that **deletes** `instance() -> &'static Self` and ripples every `'static`-closure capture (`Scheduler::instance().schedule_frame(Box::new(...))` at `renderer_binding.rs:220,347`). The hand-rolled `AppBinding` `OnceLock` (`app/binding.rs:142-148`) is invisible to an `impl_binding_singleton!` grep.

**Honest verdict on the steelman:** B's load-bearing case is *deadlock-class elimination + correctly-scoped soundness + platform fit*. Its latency and Flutter-fidelity arguments are **not** load-bearing and should be dropped.

---

## 7. Option C is not a footnote

If the requirement is ratified *and* the foundation change must be deferred, **Option C deserves a real look**: a `MainThreadMarker` / `PhantomData<*const ()>` token gating recogniser-internal `!Send` state while keeping `Arc<dyn Member + Send + Sync>` arena storage. It captures the recogniser hot-path `Rc<RefCell>`/owned-state win with **zero `flui-foundation` ripple**. Its cost is an `AtomicRefCell`-style unsafety seam (UB only if a member method is ever reached off-thread) — acceptable precisely *because* the async path is dead (C-1). C is the pragmatic middle if compile-time enforcement is wanted but a foundation-wide migration is not yet fundable.

---

## 8. Migration sketch (recommended sequenced path)

### Phase 0 — fix the dual-binding bug now (any option; ~5 lines; no `Send+Sync` change)
`AppBinding` owns `gestures: GestureBinding = GestureBinding::new()` driving all production input (`app/binding.rs:127,456,515`), while `RenderingFlutterBinding` touches a *separate* `GestureBinding::instance()` global (`renderer_binding.rs:273,317`). Two arenas. **Pick one authoritative arena:** delegate `AppBinding` to `instance()`, or route the hit-test accessor through the owned field and drop the global. Add a test asserting the arena that receives registrations is the one `handle_pointer_event` sweeps. Fixes a real bug today and gives B a clean single-owner starting point.

### Phase 1 — converge callback discipline + wire the deadline driver (Option A; `flui-interaction` only)
- Lift `tap.rs`'s `accepted`-flag pattern (`:513`) into a shared `AcceptanceGate` — a `&self`-callable "has the arena confirmed this member won?" check.
- Route `long_press.rs` (`try_fire_timer` must gate **before** firing `on_long_press_start`, not after `accept_tracked`) and `tap_and_drag.rs` (FSM emission through the gate) through it. Three recognisers, one discipline.
- **Wire the dead deadline driver** (`check_timer`/`resolve_timed_out_arenas`) into the frame loop on the event-loop thread, beside `flush_pending_moves` (`app/binding.rs:454-456`), so a stationary held finger actually fires long-press. Stays single-threaded — no timer thread, no `Send` added.
- This is the typestate redesign's **runtime floor** and the exact code Phase 2 would promote into a type-level witness. Lands incrementally with per-recogniser regression tests.

### Phase 2 — Option B, only after the requirement is ratified
`flui-foundation`: relax `BindingBase: Sized + Send + Sync + 'static` → `Sized + 'static` (or give `GestureBinding` a bespoke thread-affine accessor); replace `instance() -> &'static Self` with an owned-on-stack model held by the winit `ApplicationHandler` (**not** `thread_local`); handle the hand-rolled `AppBinding` `OnceLock` explicitly. **Stays `Send+Sync`:** `EventLoopProxy` (the only cross-thread bridge), wgpu/`Window` handles, the scheduler queue *types*. **Async timer:** dead today — don't build the post-back lane speculatively; if ever needed, send a `Send` `TimerFired(RecognizerId)` data message via `EventLoopProxy`, drain on the **microtask** lane, resolve on the UI thread; keep the post-lock defer discipline (relocated to a thread-local queue, not eliminated). Land the bound relaxation as one foundation PR *after* enumerating every `'static`-closure capture; convert `GestureBinding` first, the other bindings lazily (relaxed bound compiles unchanged until each migrates); the ~50 `flui-app` call sites are mechanical once `instance()`'s shape is fixed, but they are one undivided batch.

---

## 9. Bugs surfaced by the research (independent of A/B/C)

1. **Dual-binding split-state (C-5)** — two `GestureBinding` allocations / two arenas; live input uses the owned one, a global is init-and-accessor-only. Latent wrong-arena hazard. *(Phase 0.)*
2. **Held-finger long-press never fires** — `check_timer`/`did_exceed_deadline`/`resolve_timed_out_arenas` have zero production callers; deadlines only advance opportunistically on the next pointer event. A stationary finger past the deadline does nothing. *(Phase 1 wiring closes it.)*
3. **Doc drift** — ADR-001 fact #3 (dead async-timer) and ARCHITECTURE.md:70 (`tokio::sync::Mutex`/`OnceLock` → actually `parking_lot::Mutex`/`once_cell::Lazy`). *(Patched alongside this doc.)*

---

## 10. Open questions — re-verify before committing Phase 2

1. **THE gating question:** compile-time single-delivery — hard requirement or nice-to-have? (chief-architect + product-steward). Nice-to-have ⇒ stop at Phase 1; do not fund Phase 2.
2. **Enumerate every `'static`-closure capture of `instance()`** — determines whether deleting `instance() -> &'static Self` is tractable. Currently un-enumerated; the dominant hidden ripple.
3. **Re-verify the consuming witness is expressible on the recogniser-internal FSM** without re-introducing `Arc<Mutex>`, given the `team.rs` `Arc<Mutex<CombiningMember>>` precedent — and confirm whether the *full* compile-time form needs the arena-by-ID redesign (§3).
4. **Witness arity:** must cover `PrimaryPointer` (one pointer) and `OneSequence` (`tracked_pointers()`, many). A single `AcceptedGesture` must be per-sequence or carry the pointer set, or a runtime check leaks back into the "type-safe" witness.
5. **Deadline post-back lane:** confirm microtask (same-frame) vs post-frame so B does not add a frame of latency.
6. **`is_initialized()`/`check_instance()` cross-thread audit** — any init-on-A / assert-on-B path silently breaks under a per-thread model.
7. **Confirm the held-finger gap with an integration test** before claiming Phase 1 closes it.
8. **Re-score Option C** (`MainThreadMarker` token, `Arc<dyn>` arena retained) if the requirement is ratified but the foundation change is deferred.

---

## Provenance

Two multi-agent passes (`wf_19dd45de-bf1`), 19 agents each. Pass 1's external-research leg failed (agent-type namespace bug) and produced a code-only verdict ("stay A; typestate orthogonal to threading"); pass 2 ran the external leg (Flutter / winit / Rust-UI-framework survey / latency evidence) and produced the grounded, sequenced verdict above. The two agree on every verified code fact and on "latency is noise"; pass 2 corrects pass 1's overstatement that the guarantee is unreachable *even under B* — it is reachable on the recogniser-internal FSM, conditional on the arena-by-ID redesign. All `file:line` claims in §1, §3, §5, §8, §9 were re-verified by hand against the worktree on 2026-06-09.
