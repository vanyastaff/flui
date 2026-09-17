# Spec — #1055 `end_of_frame`: demand-driven and cancellation-safe

Base: `main` at `8b53598d`. Branch: `fix/1055-end-of-frame-demand-and-cancellation`.
Problem and non-goals carried from `intent.md`. Research in `survey.md`.

**Revision 5.** Four review rounds; `## Review history` records each. Revision 5 is the first one
that gets *smaller*: the demand gate is deleted, not redesigned. Two independent routes arrived at
the same answer — cross-language prior art (`survey.md` §4b) and an adversarial pass that asked what
a redundant demand actually costs.

## Problem (carried)

`UpdateScheduler::end_of_frame` (a) registers a waiter without ever requesting a frame, so an idle
caller can wait forever, and (b) parks a `Waker` in a strongly-shared `Arc` that nothing removes on
drop, so a cancelled wait pins executor resources until the next completion.

Two independent one-liners plus one house-trap fix. Revisions 2–4 grew the second one into six
fixes; this revision prices it back down.

## Fix 1 — cancellation: the registry's handle becomes `Weak`

```rust
struct FrameCompletionNotifier { state: Weak<Mutex<FrameCompletionState>> }
```

The future keeps the only `Arc`. Dropping it frees `FrameCompletionState` — and the stored `Waker`
with it — immediately, **with no lock taken**, no registry mutation, and therefore no drop-order or
reentrancy hazard. The drain skips an entry whose `upgrade()` returns `None`; that means
*cancelled*, never *error*, and must not be traced (it would log at frame rate).

Lock-free cancellation is not merely convenient — `survey.md` §4b records that AOSP shipped
`BroadcastFrameClock` with a locking cancellation path, hit a production deadlock when a wait was
cancelled while a frame was dispatching, and fixed it by making cancellation acquire no locks.
Same defect, same fix, arrived at from the other direction.

Two arguments belong in the code, because only one of them is what actually holds:

- **Why `retain` under the guard is safe:** dropping a `Weak` whose strong count is already zero
  runs no user code (`T::drop` ran when strong hit zero; only an allocator free can happen).
- **Why it never removes a live waiter:** `strong_count() == 0` is a **final, stable observation**
  for a `Weak` — `upgrade()` fails at zero and the count can never rise again. The inverse race
  (reads 1, drops to 0 immediately after) is harmless: the entry survives to the next compaction or
  to the drain, where `upgrade()` returns `None`.

One interleaving is **legal and must be commented as such at the wake site**: a waiter that observes
`completed` on another thread between the drain's guard release and its `wake()` resolves, and the
drain then wakes a waker whose task is gone. `Waker::wake` after completion is explicitly permitted;
say so, or someone will "fix" it into a lock held across the wake.

**Compaction.** The drain empties the vec every completion, so dead entries never survive a frame.
The unbounded case is registration while no frame ever completes. Compact on push:
`if len() >= next_compaction { retain(|n| n.state.strong_count() > 0); next_compaction = 2*len() + K }`.

- **`K = 8`, and `next_compaction` starts at `K`.** With `K = 0`, a `retain` leaving `len() == 0`
  sets the threshold to 0 and every subsequent push compacts, forever.
- **The drain resets `next_compaction = K`** in its own locked section, or the threshold ratchets to
  the all-time peak and the bound becomes a high-water mark rather than the live population.
- **The invariant it buys** is `len() <= 2 * live_at_last_compaction + K`, hence
  `len() <= 2 * peak_live + K`, with total scan work over *n* pushes in O(*n*).

  **Not `2 * live + K` "at all times" — that is false, and the builder measured it rather than
  arguing it.** `compact_if_due` sizes the next threshold from the live count *at that scan* and
  nothing lowers it again until the next scan or a drain, so a live population that **collapses**
  after a scan leaves `len` climbing toward a threshold sized for a population that no longer
  exists. Measured worst case on the shape criterion 11 walks — hold 9 waiters until a scan lifts
  the threshold, drop all 9, then register-and-cancel — was `len = 24, live = 0`, against a
  `2*0 + 8 = 8` budget. The peak form is still the useful statement: holdings are bounded by a
  constant factor of a population that was genuinely live at some point, and the drain resets the
  threshold every frame, so "peak" is per-frame in practice. Criterion 11 asserts the peak form and
  walks the collapse shape deliberately.

  "Dead entries do not accumulate" is stronger than amortized doubling provides — do not write it.

`completion_waiter_count()` returns `waiters.len()`, which under `Weak` **no longer decrements when
a future is dropped, by design.** Criteria that count waiters need a new
`live_completion_waiter_count()`; the existing one stays for capacity assertions.

## Fix 2 — demand: the registry's own live population is the memo

Revisions 2–4 each asked *"where should the demand gate read from?"* — `ensureVisualUpdate`, then
`event-listener`, then `phase() == Idle`, then a `frame_open` flag — and each answer was wrong in a
new place. Nobody asked what a redundant demand **costs**. It costs one frame:
`request_frame_impl` is a `swap(true, AcqRel)` firing the wake hook only on the `false → true` edge,
and `handle_begin_frame` clears the latch before the frame body. No correctness cost, no loop, no
starvation. The entire `frame_open` apparatus — a new field, three wedge mitigations, an RAII guard,
a `debug_assert`, and a blocking defect of its own — existed to avoid that one frame.

`survey.md` §4b independently says the same thing: Compose, Choreographer, `requestAnimationFrame`,
and Unity all make **registration itself the demand**, and Compose states the rule exactly —
the hook fires on the **0 → 1 transition of the awaiter set**. None reads a phase; none keeps
per-frame state.

```rust
pub fn end_of_frame(&self) -> FrameCompletionFuture {
    let (future, state) = FrameCompletionFuture::new();
    let need_demand = {
        let mut reg = self.inner.frame.completion_waiters.lock();
        reg.compact_if_due();
        // LIVE, not `is_empty()` — see the invariant below. `any` short-circuits
        // on the first live entry, so the walk is O(1) in the common case and
        // O(n) only when every entry is a corpse.
        let had_live = reg.waiters.iter().any(|n| n.state.strong_count() > 0);
        reg.waiters.push(FrameCompletionNotifier { state: Arc::downgrade(&state) });
        !had_live
    };                                   // guard released HERE — load-bearing
    if need_demand { self.schedule_frame_if_enabled(); }
    future
}
```

**The invariant has two halves, and only the first belongs to the registry.**

*Issuance — the registry decides it, and the argument is airtight by induction.* After any drain the
vec is empty, so the first push after a drain sees no live entry and demands. Every later push
either sees a live entry — whose demand postdates that drain, by induction — or demands itself. A
cancellation racing a drain on another thread cannot lose one: the drain takes the whole vec under
the guard, so an entry is either in the drained batch (served) or was pushed after the guard was
released, and that push reads the post-drain vec. **There is no bit written at one time and read at
another** — the predicate is a pure function of the vec's contents at push time, which is the whole
reason this is smaller than revision 4 rather than merely different.

*Survival — no registry predicate can decide it.* A demand can be **revoked** without a drain: by
`finish_async_pump`, and by frames being disabled at request time. If a live waiter's demand is
revoked, a later push sees the live entry, stays silent, and both hang. **`set_frames_enabled(true)`'s
unconditional re-demand is the other half of this liveness argument, not a consistency nicety** —
and that is what its `## Mapping decisions` reason must say.

**The revocation leg IS reachable, and the earlier draft of this paragraph got the reason wrong in a
way that would have licensed deleting the thing that saves it.** The live sequence:

1. Frames enabled. `end_of_frame()` finds an empty registry, demands, `frame_scheduled = true`,
   waiter W is live.
2. Lifecycle → `Hidden`: `frames_enabled = false`. No frame ran; the latch is still `true`.
3. A pump tick takes the `PumpAsync` arm → `finish_async_pump()` → `frame_scheduled = false`.
   **W's demand is revoked with no drain.** Any later registration sees W live and stays silent.
4. Lifecycle → `Resumed`: the `!frames_were_enabled && should_render` edge calls `request_frame()`.
   Recovered.

So the demand *was* issued and *was* revoked; what saves every stranded waiter is **step 4**, not —
as this spec previously claimed — that the call "was a no-op anyway". That claim covered only a
registration made *while* frames were already off, which is not the interesting case.

**The production leg is `handle_app_lifecycle_state_change`, not `set_frames_enabled`.**
`set_frames_enabled` has **zero** production callers: the one I cited as "passes `false`" is inside
a `#[cfg(test)] mod tests`. Its re-arm ships as the API-surface mirror so the public setter is not a
trap; the leg that actually carries liveness in production is the pre-existing lifecycle resume
edge, and both records must name it or a reader who greps for callers will conclude the survival
argument is vacuous and delete it.

Single-threadedness is what keeps step 3 from racing: `UiRealm` is `!Send + !Sync`
(`assert_not_impl_any!`), so the pump loop and the lifecycle transition share a thread and there is
no window between the `frames_enabled` read and the latch clear.

**Why `is_empty()` is not enough, recorded because I argued both sides of it and was wrong twice.**
`is_empty()` would also be sound *today* — a corpse can only exist since the last drain, and the
push that created it demanded, so that demand is still outstanding. But its soundness spans three
pieces of state (registry population, the `frame_scheduled` latch, and `frames_enabled` plus the
lifecycle edge), none of them asserted anywhere, and one leg is the **public** `finish_async_pump`.
`had_live` reads only the vec it is already holding. A predicate that is sound only via an untested
three-way coupling is exactly the failure mode the first four revisions kept producing.

**Drive-by doc fix, owed here because Fix 2's reasoning depends on it.** `finish_async_pump`'s own
doc comment states that *"the only place that ever clears the latch back to `false` is
`handle_begin_frame`"* — a claim that method falsifies three lines further down in the same comment,
where it clears the latch itself. Correct it to name both clearers.

The three load-bearing details, **with the reasons earlier revisions got wrong**:

1. **Release the guard before demanding.** Revision 3 justified this with a test harness whose hook
   drives a frame inline — a harness that violates `set_on_frame_scheduled`'s own documented
   contract (*"must only touch wake machinery, never re-enter the scheduler"*). The real reason is
   already enforced: `frame_scheduled_hook_runs_with_no_scheduler_lock_held` asserts every scheduler
   mutex, `completion_waiters` included, is free inside that hook. **Holding the guard across
   `schedule_frame_if_enabled()` fails an already-green test.**
2. **Register before demanding.** Not for the inline-hook reason either: demand-then-register lets a
   concurrent frame begin *and* drain between the two, so the waiter misses frame N and buys a
   redundant N+1. Register-first makes the registration the linearization point. **The
   `## Mapping decisions` entry must carry this reason** — it is a divergence from Flutter, which
   requests first, and a record with the wrong reason is wrong on the day it lands.
3. **`schedule_frame_if_enabled()`, not `request_frame()`.** Dart's `scheduleFrame()` carries the
   `framesEnabled` check internally; `request_frame()` would force frames on a scheduler whose owner
   disabled them.

### Residual cost, stated rather than engineered away

A registration that lands mid-frame while no other waiter is live demands a frame the in-flight
drain would have served anyway: **one surplus frame, self-limiting, never a loop.** That is the
price of deleting `frame_open`, and it is the price four toolkits already pay.

### Consequences

- **`set_frames_enabled(true)` re-demands on the edge** — load-bearing, per the invariant above:
  `let was = swap(enabled); if !was && enabled { request_frame() }`, mirroring
  `handle_app_lifecycle_state_change` (Flutter's `_setFramesEnabledState`). It has zero *enabling*
  callers today (the one production call passes `false`; the live enable edge is the lifecycle path,
  which already re-arms), so it costs nothing to add and closes the only hole in Fix 2's invariant.
  A registry check here would **not** self-deadlock — read, drop the guard, then request, the same
  shape as registration; the real objection is a second site of the same ordering discipline, and
  the edge re-arm needs no registry access at all.
- `execute_idle_callbacks` returns 0 while a frame is scheduled, so a pending `end_of_frame` now
  suppresses idle work until the frame runs. Intended meaning of demand-driven.
- `end_of_frame` becomes **panic-capable** (it can reach a user hook) → `# Panics` section.
- **`#[must_use]`** — because a registration now costs a frame whether or not the future is awaited.
  (Not, as revision 4 claimed, to catch `let _ = …`; that is the sanctioned way to *silence* the
  lint.)
- **Three doc obligations, all owed in this PR.** Each names a pre-existing hazard that Fix 2 makes
  *reachable* by turning `end_of_frame` from "always hangs, so nobody calls it" into a usable idiom.
  None needs code here; all three need saying:
  1. **A completion future whose scheduler is dropped never resolves.** #1162 carries the fix (there
     is no sentinel to return while `Output = FrameTiming`); this PR carries the note.
  2. **A panicking `on_frame_scheduled` hook loses the demand permanently.** `request_frame_impl`
     swaps `frame_scheduled = true` *before* firing the hook, so an unwinding hook leaves the latch
     set with no wake delivered, and every later demand hits the no-op edge. Compose defines this
     transition (a throwing `onNewAwaiters` permanently fails the clock and resumes all current and
     future awaiters with the error); FLUI has no defined transition, and naming the gap is enough
     for this PR.
  3. **`FrameCompletionFuture` is not fused.** `poll` `take()`s `completed`, and the drain has
     already removed the registry entry, so a second poll after `Ready` stores a waker nothing will
     ever call — a silent permanent hang. Unity documents the same shape as undefined behaviour.
     Fix 3 is already editing `poll`; a `finished: bool` and a `panic!` is one line if we want it,
     but at minimum the doc says it.

## Fix 3 — the lock order, written into the code

`FrameCompletionFuture::poll` does `state.waker = Some(cx.waker().clone())` under the `state` guard,
which **drops the displaced `Waker` — executor code — under the lock**. House trap (#1038, #1156).

**The hazard is single-threaded self-relock, not ABBA.** Revision 3 claimed a two-thread
`completion_waiters → state` inversion; that edge is not constructible in the design being specified
(teardown is deferred, and compaction uses `strong_count()`, which takes no `state` lock).
The real failure: a displaced `Waker` whose `Drop` re-polls the same future relocks the
non-reentrant `state` mutex and hangs. Overclaiming invites someone to disprove the fix and drop it.

Fix: bind the displaced waker out of the guard. Then write the rule at the registry definition:

> **`completion_waiters` strictly before `FrameCompletionState`, never nested, and neither held
> across `wake()`, a `Waker` drop, or `schedule_frame_if_enabled()`.**

`notify_frame_completion` already satisfies it and must keep three properties, commented at the site
because they are load-bearing and invisible: `upgrade()` bound at loop-body scope (never inside the
`completion_waiters` block); the `state` guard in a nested block ending before `wake()`; and the
waker always `take()`n before the temporary `Arc` can be the last strong reference — which is what
stops `FrameCompletionState::drop` running user code under a guard.

This callback family has **no lock-discipline test at all**, while every other one in
`lock_discipline_tests.rs` does. Add the completion-waker call site there.

## Acceptance criteria

Labelled by what a green run proves. Round 4 caught three mislabels; the labels are part of the
spec, not decoration.

### Red-then-green — fails on `main` @ `8b53598d`

1. **Idle demand.** Idle scheduler, `frames_enabled`: `end_of_frame()` fires the wake hook exactly
   once and resolves with that frame's timing.
2. **Post-drain registration demands.** Register from inside a completion waker (raw `impl Wake`,
   **no frame-driving hook** — see the note below); after `execute_frame()` returns,
   `is_frame_scheduled()`. The primary oracle for the defect revisions 2–4 kept reintroducing.
   Under Fix 2 it passes *structurally*: the drain emptied the registry, so the waker's push finds
   no live waiter.
3. **Same on the abort path**, asserting **exactly one** demand.
4. **Coalescing.** N registrations inside one frame fire the wake hook **at most once**. This is the
   property worth pinning now that the gate is gone — `request_frame_impl`'s swap edge delivers it,
   and it is what stops "registration is the demand" from meaning "a frame per registration".
5. **After every frame-ending path**, a subsequent `end_of_frame()` demands, hook firing exactly
   once. (Round 4: this is red on `main` — `end_of_frame` never calls any request path — and
   revision 4 filed it as a guard.)
6. **Cancellation releases immediately.** Poll once with a custom `Wake`, drop the future, then
   `Arc::strong_count(&probe_waker) == 1` (2 on `main`).
7. **Displaced-waker lock freedom.** Poll with **two distinct** wakers, retaining no `Arc` to the
   first; the displaced waker's `Drop` asserts `state.try_lock().is_some()`. Both conditions matter:
   a retained clone means the `Drop` never runs user code, and one waker means `will_wake` displaces
   nothing. Red without Fix 3.
8. **Hook lock freedom.** `end_of_frame()` on an idle scheduler with a hook whose body is
   `assert_no_scheduler_lock_held(&probe)` — **and assert the hook actually ran**, or it passes on
   `main` by never firing.
9. **Idle callbacks.** A pending `end_of_frame` suppresses `execute_idle_callbacks`.
10. **Re-enable.** `is_frame_scheduled()` immediately after the `set_frames_enabled(true)` edge.
    (Not "the waiter resolves" — driving a frame in the test resolves it on `main` too.)

### New-code invariant (cannot compile against `main`)

11. **Compaction bound.** `len() <= 2 * live + K` across a register/drop burst; the drain resets the
    threshold. Unit test inside `scheduler.rs` — `completion_waiter_count` is `#[cfg(test)]`-private
    — needing `live_completion_waiter_count` and a test-only scan counter.

### Guards — green both ways, defending against a future wrong shape

12. **Enablement respected**; **no sibling starvation**; **lock discipline from a completion waker**;
    **type pins** (`Send + Sync + Unpin` and `assert_not_impl_any!(…, Clone)` — all already true on
    `main`, since Fix 1 changes the *registry* handle and never the future's own shape, so a green
    run proves nothing about this fix and it belongs here); and the three existing pins (#1057
    inline-poll, #1158 panicking waker, `frame_panic_recovery.rs`).

**Note on `frame_panic_recovery.rs`:** its `!is_frame_scheduled()` assertion stays green only
because `armed_completion_probe` registers *before* `drive_frame`, so `handle_begin_frame`'s
`frame_scheduled.store(false)` clears the new demand. One ordering detail from red — say so at the
assertion rather than claiming "unmodified and green" bare.

**No oracle may install a frame-driving `on_frame_scheduled` hook.** `AsyncDriver`'s wake hook
requests unconditionally with no gate, so a task awaiting under the in-crate driver is green against
the bug (raw `impl Wake`, never `spawn_local`); and an inline-driving hook reached from post-drain
demand re-enters `handle_begin_frame` at phase `PostFrameCallbacks`, which has no valid transition.

## Red evidence (on `main` @ `8b53598d`)

Preserved at `<scratchpad>/end_of_frame_lifecycle.rs` (86 lines); **landing it on the branch is the
first build step** — it exists only in a builder worktree, which gets cleaned.

```
PASS (1/3) explicit_frame_completes_waiter_and_releases_waker
FAIL (2/3) idle_frame_waiter_requests_a_frame — left: (false, 0)  right: (true, 1)
FAIL (3/3) dropped_waiter_releases_its_executor_waker — cancelled wait retains executor waker
```

It is **red evidence, not a pin**: `idle_frame_waiter_requests_a_frame` polls after registration, so
it passes whether demand happens at creation or at first poll. Criteria 2–5 are the pins.

## Scope

**This PR:** Fixes 1–3. One registration-lifecycle change; Fix 3's hazard is created by Fix 1.
`Output` stays `FrameTiming`.

**Deferred to #1162** with the review findings attached: the three-arm `FrameCompletionOutcome`,
`impl Drop for SchedulerInner`, the `docs/runtime-contract.toml` note, and their mapping entries.
The argument for deferring is round 3's, not mine: the breaking window is open *because* there are
zero production callers, which is exactly what makes a second break free too — and the teardown's
flagship guarantee is unreachable in the common ownership shape while its precedence rule is
provably vacuous.

## Review history

- **r1** — cited `ensureVisualUpdate` as `endOfFrame`'s demand path. Wrong: `endOfFrame` calls
  `scheduleFrame()`.
- **r2** — proposed replacing the registry with `event-listener`. Withdrawn: `Event::notify` wakes
  under its own non-reentrant mutex, against two green tests (`survey.md` §5).
- **r3** — the `Idle`-only gate reintroduces #1055's hang in the post-drain window; `poll` drops the
  displaced waker under the guard; no `Drop` exists to hang teardown on; `resume_unwind` in that
  `Drop` would abort; `Option` spends the breaking window on the wrong discriminant.
- **r4** — the `frame_open`/drain pairing claim was false in three places; two justifications rested
  on a harness that violates a documented contract; the teardown precedence rule is vacuous; the
  compaction constants degenerate; the ABBA claim was an overclaim. Scope cut six → three.
- **r5** — `frame_open`'s clear point (a `Drop` guard at the end of `end_frame_impl`) is
  *later* than the phase reset it replaced, so it reproduced revision 2's defect verbatim and
  criterion 2 would have failed **with** the fix. Rather than move the clear point, the field is
  deleted: the registry's own live population is the memo, with no clear point to get wrong. Three
  criteria were mislabelled, three oracles were vacuous as worded, and three justifications were
  wrong (`#[must_use]`'s reason, a false `&mut self` claim — `UpdateScheduler` is `#[derive(Clone)]`
  — and a "could self-deadlock" overclaim). Cross-language prior art (`survey.md` §4b) reached the
  same design from the other side.

- **r5.1** — my first corpse objection was wrong (the corpse's own push had already demanded), so
  the predicate was set to `is_empty()`. Four criteria relabelled, three oracles restated, three
  pre-existing hazards that Fix 2 makes reachable given doc obligations.
- **r5.2 (this)** — `is_empty()` reverted to a live count, on a reason neither of us had: the
  reviewer found `frame_scheduled` has a **second** clearer, the public `finish_async_pump`, whose
  own doc comment claims `handle_begin_frame` is the only one. `is_empty()` is sound only through an
  untested three-way coupling across registry population, that latch, and the frames-enabled edge.
  The invariant was split into issuance (the registry decides) and survival (only the frames-enabled
  edge can re-issue a revoked demand), which is what makes the `set_frames_enabled` re-arm
  load-bearing rather than parity-motivated.

- **r6 (post-build)** — the compaction invariant `len() <= 2 * live + K` "at all times" was **false**,
  found by the builder with a measured counterexample (`len = 24, live = 0`) rather than an argument.
  Criterion 11 as written could not have been satisfied by a correct implementation. Corrected to the
  peak form above.

**Verdict: built.** Fixes 1 and 3 were never reshaped and are independently corroborated by AOSP's
own history on the same problem (`survey.md` §4b). Fix 2's mechanism is the part that thrashed; its
answer was the registry's own population, counted live — `is_empty()` was the near-miss, sound today
only through an untested coupling that includes a public method.

## Links

- Issue: #1055 · Intent: `intent.md` · Research: `survey.md` · Filed on the way: #1161, #1162
- Movement under this code: #1156, #1157, #1158, #1160
