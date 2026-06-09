# ADR-001: Gesture binding threading model

Status: **Proposed — superseded framing** · Date: 2026-06-09 · Scope: `flui-interaction`, `flui-foundation`

> **Decision research (2026-06-09):** see
> [`docs/research/2026-06-09-adr-001-gesture-binding-threading.md`](../../../docs/research/2026-06-09-adr-001-gesture-binding-threading.md).
> It reframes this ADR on three points, verified against code:
> (1) **latency is not the axis** — the gesture hot path is single-producer/uncontended, the threading model is not a beat-Flutter lever;
> (2) the real decision is a single product question — **is "a loser cannot fire `on_*`" required to be a _compile error_ or only a _tested invariant_?** Runtime ⇒ Option A wins outright; compile-time ⇒ Option B is necessary-but-not-sufficient (also needs an arena-by-ID redesign, since the winner is matched by `Arc::ptr_eq` over aliased `Arc<dyn>`);
> (3) load-bearing **fact #3 below is false** — the async-timer path is dead (zero production callers); recogniser `Send+Sync` is not actually exercised cross-thread today.
> The research also surfaces two live bugs (dual-binding split-state; held-finger long-press never fires) to fix first, under the status quo.

## Context

The gesture subsystem is pervasively built on `Arc<Mutex<…>>` and atomics: ~300
`Arc<` occurrences, a lock or atomic on most recogniser state. This exists
because **recognisers must be `Send + Sync`**, which is forced by three
load-bearing facts:

1. The arena stores members as `Arc<dyn GestureArenaMember + Send + Sync>`
   (`arena/mod.rs`), so anything added must be `Send + Sync`.
2. `GestureBinding::instance()` returns `&'static Self` via a `OnceLock`
   singleton (`flui-foundation`), so the binding type must be `Sync`.
3. ~~`GestureTimerService` offers an **optional** `tokio::spawn` mode
   (`timer.rs`), so a deadline callback can fire on a worker thread.~~
   **Corrected (2026-06-09): this path is dead.** `run_async` /
   `run_until_shutdown` / `check_timers` have zero production callers; no
   recogniser uses `GestureTimerService` — deadlines are polled inline via
   `Instant::now()`. So fact #3 does **not** justify cross-thread recogniser
   access today; the `Send+Sync` bound is incidental (storage mechanics:
   `OnceLock<T>: Sync` + the `BindingBase: Send+Sync` supertrait), not
   exercised.

Flutter's gesture pipeline is single-threaded; its recognisers are plain
mutable objects on the platform thread. The FLUI port reproduces Flutter's
shared-mutable-by-default model with `Arc<Mutex>`, paying lock/atomic cost
single-threaded Dart never pays. This directly taxes the project's stated
"beat Flutter on predictable latency" goal, and the non-reentrant
`parking_lot::Mutex` is what makes the arena re-entrancy deadlock (closed in
`fix/pr164-criticals`) possible in the first place.

This is the headline finding from the PR #164 review: the subsystem is
substantially a 1:1 Dart→Rust transliteration of GC-shared mutability rather
than a Rust-native ownership design.

**Important — what does NOT work:** several "obvious" Rust-native fixes are
unsound under the current contract and were rejected after verification:
`Cell`/`RefCell` won't compile (recognisers must be `Sync`); making
`GestureSettings` an inline `Copy` field breaks the shared `set_settings` across
`#[derive(Clone)]` handles; `DashMap` can't be swapped for a plain map while the
binding is `&'static Sync`. The lock edifice is a *consequence* of the
`&'static Send+Sync` binding decision, not gratuitous.

## Decision

Choose the binding's threading model before undertaking the recogniser
**typestate redesign** (coupling callback delivery to arena acceptance), because
the typestate's ownership shape depends on whether recognisers are `Send+Sync`
or single-thread-affine.

## Options

### A — Status quo: `&'static Send+Sync` singleton + minimized locks
Keep the singleton binding; reduce per-recogniser lock cost in place (atomics
for flags, one `Mutex` per state struct instead of per field).
- **Pros:** no foundation change; thread-safe; async timer works as-is.
- **Cons:** still pays atomic cost on every event; the C3 deadlock *class*
  remains latent (mitigated, not eliminated); does not realize the GC-free
  latency thesis.
- **Evidence it helps:** `fix/pr164-criticals` already converted
  `RecognizerBase::{disposed, primary_pointer}` to atomics and **measured
  `TapGestureRecognizer::handle_event` 86.6 ns → 39.3 ns (−54.6 %)**. So there is
  real headroom even within Option A.

### B — UI-thread-affine binding (`thread_local` / single owner)
Make the binding owned by the platform/UI thread (not a `&'static Sync`
singleton). Recognisers become `Rc<RefCell<…>>` / `Cell` / plain fields; the
async timer posts back to the UI thread instead of calling recogniser code
cross-thread.
- **Pros:** zero atomic/lock cost on the recogniser hot path; **structurally
  eliminates** the arena re-entrancy deadlock class (single-threaded, reentrancy
  is a logic bug not a hang); matches Flutter's model and the beat-Flutter
  thesis; recognisers become `!Send` which *documents* the thread affinity in
  the type system.
- **Cons:** workspace-wide ripple through `flui-foundation` (the `HasInstance`
  /`OnceLock` binding pattern); every binding consumer must run on the UI thread;
  the async-timer ergonomics change (post-back channel).

### C — Hybrid boundary
Keep `Send+Sync` only at the arena/binding boundary; isolate single-threaded
recogniser state behind that boundary. (In practice this is Option A taken to its
limit and converges on its cost profile.)

## Recommendation

**Option B is the real lever** for the project's stated competitive goal, but it
is a `flui-foundation`-level change and should be scheduled deliberately, not
smuggled into a parity PR. **Until then, continue Option A's incremental wins**
(the atomic conversion is the template; `GestureSettings` consolidation and
per-event allocation removal are next), and design the typestate redesign so it
does **not** bake in assumptions that Option B would have to unwind — i.e. keep
the recogniser↔arena acceptance coupling expressed in terms that work whether
recognisers are `Arc`-shared or single-owner.

## Consequences

- The PR #164 remediation (`fix/pr164-criticals`) takes Option A: deadlock fixed
  at the arena (defer notifications out of the lock), hot-path locks converted to
  atomics. No foundation change.
- The deep typestate redesign is **blocked on this decision** for its ownership
  model. If B is chosen, the redesign uses `Rc`/owned state; if A, it stays
  `Arc`-shared with the stable-identity (`Weak`) pattern introduced for the
  long-press deadline win.
- Revisit when the renderer/scheduler threading model is finalized, since the
  binding's thread affinity should be consistent across `flui-foundation`.
