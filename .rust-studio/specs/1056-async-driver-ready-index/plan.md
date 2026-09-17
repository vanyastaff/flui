# #1056 — AsyncDriver readiness discovery: implementation plan v2

v1 had a data-loss bug (harsh-critic #1) and a forever-reallocating index
(systems-perf-lead A). Fixed by taking full ownership of one pump's ready
batch (`PumpGuard`) instead of scanning or racing a shared set.

## 1. Acceptance criteria

- R=0/N dormant tasks: `poll_ready`/`ready_task_count` touch no dormant slot.
- K wakes before the next pump ⇒ exactly 1 frame request, 1 poll.
- Self-wake during a poll defers to the next pump (unchanged test).
- N tasks woken descending ⇒ still polled ascending (new test).
- **Stranding fixture**: three ready tasks, middle one panics on poll ⇒ the
  third (never reached) is polled next pump with no external wake needed.
  `tests/frame_panic_recovery.rs:415-460`
  (`async_future_poll_panic_closes_the_frame`) is exactly this fixture
  already — v2 must keep it green, not just add a new unit test.
- **Allocation oracle (the real complexity gate, CI-run):** R=0 at N∈{0,100k}
  costs 0 allocations/pump once warm; steady R=k costs 0 after the first.
  `just ci` has no bench step — this test IS the gate.
- Criterion bench: evidence only, not gated (CI's `bench-compile` is
  `--no-run`); reported, never asserted as a merge gate.

## 2. Data structure, ownership protocol

`TaskStore { tasks: BTreeMap<TaskId, Task>, ready: Vec<TaskId> }` behind the
one existing `Mutex` (`Inner.store`, renamed from `Inner.tasks`). `ready` is
**wake-arrival order**, sorted once per drain — not a `BTreeSet`
(systems-perf A: its `mem::take` hands back a *deallocated* tree, so any
R>0 pump reallocates ⌈R/11⌉ nodes forever; a persistent `Vec` +
capacity-donation does not). No dedup: the per-task `AtomicBool` gate makes
a double-push impossible by construction; state that via `debug_assert!`.

**`ready` is never proactively purged on cancel, nor scrubbed for a
self-woken id whose task then panics.** Both go stale for ≤1 pump and
self-heal via `poll_ready`'s existing "id not found ⇒ skip" arm — cheaper
than an O(R) scan per cancel. The only place staleness must not mean
**loss** is the panic path, handled by `PumpGuard`.

**Protocol** (lock state per step):
- *spawn_local*: alloc (no lock) → lock `store`: `tasks.insert`,
  `ready.push(id)`; unlock → `request_frame()`.
- *spawn_local_eager*: poll inline (no lock) → `Pending` ⇒ lock once:
  `tasks.insert`; if `ready.load(Acquire)` also `ready.push(id)`, same
  section; unlock → outside lock, if loaded, `request_frame()`.
- *wake_by_ref*: cancelled fast-exit → `ready.swap(true)`, already-true ⇒
  return (no lock, unchanged fast path) → upgrade → lock: if
  `tasks.contains_key(id)`, `ready.push(id)`; unlock → if live,
  `request_frame()`.
- *TaskToken::cancel*: **unchanged from today**, field rename only.
- *poll_ready*: (1) lock; `let mut remaining = mem::take(&mut
  store.ready)`; unlock. (2) `remaining.sort_unstable()` **outside** the
  lock: exclusively this thread's data now; an O(R log R) pass touching
  no shared state shouldn't make a waker on another thread wait.
  `debug_assert!` no adjacent duplicates. (3) `PumpGuard{inner, remaining,
  cursor:0, in_flight:None}`. (4) while `cursor < remaining.len()`: lock,
  clear flag, `future.take()`, clone Arcs, unlock (absent ⇒ `cursor+=1;
  continue`); `in_flight=Some(id)`; **no lock**: poll (`future` declared
  before this iteration's outcome-lock, so its own eventual drop (running
  a nested `TaskToken` per #1038) happens after that lock releases, same
  shape as today's L402-vs-L440); `in_flight=None`; `polled+=1`; lock
  (bound via `let`, never a match scrutinee, per rules/async.md), apply
  outcome as today, unlock; `cursor+=1`. (5) normal exit: lock,
  `recycle(&mut store, mem::take(&mut remaining))`; unlock; return `polled`.
- `recycle(store, mut buf)`: `buf.clear(); buf.append(&mut store.ready);
  store.ready = buf;` — donates the drained batch's capacity as the new
  backing buffer, folding in anything pushed mid-pump; called on both the
  normal exit and `PumpGuard::drop`, so steady R never reallocates twice.
- **`PumpGuard::drop`** (unwind only, `in_flight` is `Some(id)`): lock;
  `tasks.remove(&id)` (**supersedes `RemoveZombieSlotOnUnwind`: delete it,
  and cluster C's planned fix to its drop-under-lock bug is moot, so drop
  that item**); `tail = remaining.split_off(cursor+1)` (ids never reached);
  `recycle(&mut store, mem::take(&mut remaining))`; `store.ready.append(&mut
  tail)`; unlock. Runs **no user code**: Rust drops the innermost scope
  first on unwind, so `future`/`ready`/`cancelled`/the waker (declared
  inside the loop body) fully drop first (including any nested `cancel()`
  reentry) before this guard's own Drop runs, so its lock acquisition
  never contends with a destructor it triggered.
- `ready_task_count`: `store.lock().ready.len()`, O(1), **exact between
  pumps**; may transiently over-count inside one in-flight pump racing a
  cancel/self-wake against a sibling's panic — self-heals next pump. 5 call
  sites, all this file's own tests; `UpdateScheduler` never forwards it.

**`//!` doc addition** (required): state *"`ready==true` ⟹ id ∈
`store.ready` ∪ the in-flight pump's own unprocessed tail"* and name every
path that sets the flag true: `spawn_local` (seeds+pushes); `wake_by_ref`'s
false→true edge (pushes if live); `spawn_local_eager` step 5 (pushes if
loaded post-poll); a pump's own remainder (`PumpGuard`, restored on panic,
consumed on success).

## 3. Tests + revert matrix

Existing 20 stay green. Verified renames (not review's approximate
`~L1033/~L1221`): `is_unlocked` (L489-491) and `Debug` (L494-512) read
`self.inner.tasks` → `self.inner.store`; the `Probe` in
`cancel_drops_the_future_outside_the_task_lock` (L1032) likewise.
`replacing_the_request_frame_hook_...` (~L1221) reads `inner.request_frame`,
untouched — no edit; that second citation was imprecise.

| Test | Pins | Revert target |
|---|---|---|
| `panic_mid_pump_keeps_unreached_siblings_indexed` | stranding fix: 3 tasks, middle panics, `ready_task_count()==1` after unwind, next pump polls the third | delete `PumpGuard::drop`'s tail restore |
| `poll_ready_visits_ready_ids_ascending_even_when_woken_in_reverse` | sort-at-drain vs. arrival order | delete `sort_unstable()` |
| `self_wake_during_a_poll_that_then_panics_leaves_a_stale_id_that_self_heals` (renamed) | one-pump over-count that self-heals via the skip-arm — **not** a hazard the O(N) scan lacked, per review | make the skip-arm re-poll instead of skip |
| `cancelling_a_ready_but_unpolled_task_self_heals_on_the_next_pump` (renamed) | same self-heal story, cancel side | same |
| ~~`wake_from_many_threads_indexes_exactly_once`~~ | **cut** — vacuous, passes on main and under any revert | — |

**Allocation oracle** (new, CI-gated — the actual complexity proof):
`crates/flui-scheduler/tests/async_driver_ready_index_allocation.rs`,
following `frame_telemetry_allocation.rs`'s convention exactly — dedicated
binary, thread-local counting `#[global_allocator]` over `System`, ONE
`#[test]` (same cross-test-race rationale as that file). Asserts: (a) R=0 at
N∈{0,100k}, 0 allocations per `poll_ready` once warm; (b) steady R=64
(self-re-waking tasks) across 5 pumps, 0 allocations after the first.
Supersedes review's optional flag-load-counter: one mechanism, not two.

**Bench** (evidence, not a gate — say so in the file's own doc comment):
`benches/async_driver_pump.rs`, criterion, `harness=false`, **two groups**
(shrunk per review): `empty_pump` N∈{0,100k}, oracle = median(100k)/median(0)
≤2× *and* an absolute ceiling (e.g. ≤200ns/pump); 2× is noise tolerance,
not discrimination (an O(N) regression reproduces at ~40,000×); `ready_heavy`
N∈{1k,10k} (all self-re-wake), oracle = per-task median(10k) ≤1.2× per-task
median(1k), else investigate `sort_unstable` cost — a number, not judgment.
Construct outside `b.iter`; re-arm every iteration, stated in the doc
comment. `critcmp` not installed — attach raw criterion output. PR body:
machine spec, before/after tables, allocation-oracle result, "local
measurement, not CI-gated."

## 4. Risks / gates

- **Lost wakeups**: closed by `PumpGuard` owning the whole batch and
  restoring its unreached tail on unwind — v1's actual bug.
- **Reallocation forever**: closed by `recycle` on both exits; proven by
  the allocation-oracle test, not just asserted.
- **Duplicate/stale entries**: flag-gated (debug-asserted); self-heals in
  ≤1 pump via the existing skip-arm, documented not assumed.
- **ASYNC-GATE**: no blocking added; no lock across `poll` or a destructor
  (Rust's innermost-scope unwind order verifies this); public API unchanged.
- **PERF-GATE**: allocation-oracle test (CI-run) + bench evidence
  (not CI-run), per §1/§3.

## 5. Maintainer-grade pre-code verdict: **ACCEPTABLE**

1. Owning file unchanged: `flui-scheduler/src/async_driver.rs`.
2. `RemoveZombieSlotOnUnwind` deleted, not kept beside `PumpGuard` — one
   ownership mechanism for the drained batch. Cluster C's fix to that
   struct's drop-under-lock bug is superseded; drop that item.
3. Reused: the one mutex domain, ascending order (recovered via
   sort-at-drain), the per-task `AtomicBool` gate, the
   destructor-outside-lock discipline `cancel` already modeled.
4. Hot-path, internal-layout-only change; no public API/semver impact.
5. Pushback a strict maintainer keeps: is "staleness self-heals, don't
   purge" documented loudly enough that nobody later asserts an exact
   `ready_task_count()` mid-pump and files a false bug — addressed by the
   `//!` invariant and the downgraded-claim note, not left implicit.
6. No breaking changes needed.
7. Ecosystem check unchanged from v1 (futures-rs/async-task/tokio
   `LocalSet`, for the separation-of-concerns principle only).

MEMORY: a ready-index drained by `mem::take` and processed in a loop must
own the *whole* remaining batch across a panic, not just the in-flight id —
restoring the unreached tail on unwind (`PumpGuard`) is what an O(N)-scan
design never needed, since it re-derives readiness from ground truth every
call and can't strand a sibling.

MEMORY: `BTreeSet`/any `mem::take`-drained set-like index reallocates every
pump it's non-empty, because `take` hands back a deallocated container, not
an empty-but-warm one; a persistent `Vec` with explicit capacity donation
(clear + append-back, swapped in) reaches zero allocations at steady state
— prove it with a counting-allocator test, a criterion bench can't assert zero.

## Plan-review amendments to v2 (harsh-critic — ACCEPTABLE once applied)

A1. **`PumpGuard::drop` gate and cursor semantics.** There are TWO user-code sites per loop iteration: the
`poll` (in_flight `Some`) and the polled future's own destructor on the `Ready` / cancelled / slot-gone arms,
which runs at the loop body's closing brace — AFTER `in_flight = None` and after `cursor += 1`. A destructor
panic there unwinds with `in_flight == None`, so an `in_flight`-gated Drop restores nothing and strands the
tail (v1's bug through the other door); and `split_off(cursor+1)` with `cursor` already advanced loses one id.
Ungated on the NORMAL path, `split_off` on the already-taken empty Vec panics and a second `recycle` swaps in an
empty buffer (defeats the allocation oracle). Required shape:
- `done: bool`, set after step 5's `recycle`; `Drop` restores the tail only if `!done` (house style; the guard
  being deleted used the same arm/disarm).
- `if let Some(id) = in_flight { tasks.remove(&id) }` gates the slot removal SEPARATELY from the tail restore.
- Advance `cursor` BEFORE the poll, so it always means "ids consumed"; the tail is `remaining.split_off(cursor)`
  at every unwind site.
- New test beside the stranding test: `panic_in_a_completed_futures_destructor_keeps_unreached_siblings_indexed`
  — 3 tasks, the middle one completes with a panicking `Drop`; the third is polled next pump. Reddens under an
  `in_flight`-gated Drop.

A2. **Eager double-push is real; `dedup()` after `sort_unstable()`.** Interleaving: T1 `spawn_local_eager`
inline poll returns Pending (waker handed to a worker); T2 `wake_by_ref` swaps false→true and blocks on the
store lock; T1 locks, `tasks.insert`, `ready.load() == true`, `ready.push(id)`, unlock; T2 acquires,
`contains_key` → true, `ready.push(id)`. Two entries in one `store.ready` with no pump between → adjacent
duplicate after sort → the planned `debug_assert!` fires; in release the second entry finds the reinserted
future and polls the task twice in one pump. `spawn_local` is immune (no waker exists before its push). Fix:
`remaining.dedup()` after `sort_unstable()` — O(R) on exclusively owned data outside the lock; delete the
"double-push impossible" claim and its `debug_assert!`; comment names the eager race as the reason.

A3. Churn without a pump is bounded (residue = spawns + wakes since the last pump, 8 B each; nothing spawns
without a build, a build implies a frame, `spawn_local` requests one) — state the bound in the `//!` doc.

A4. Confirmed: the in-flight slot holds `future: None`, so `tasks.remove` in `Drop` drops two
`Arc<AtomicBool>` refcounts only — no user code; the panicking future's own destructor already ran during
unwind with no lock held (a panicking destructor there is a double panic → abort, the same exposure
`RemoveZombieSlotOnUnwind` has today; not new).

A5. **Inherited from cluster C (moved here):** add `poll_ready_self_cancel_during_pending_removes_slot_once` —
a future that drops its own `TaskToken` during `poll` (token held in an `Rc<RefCell<Option<TaskToken>>>` set
post-spawn) and returns `Pending`; its `Drop` probes `AsyncDriver::is_unlocked()` (async_driver.rs ~L489) so a
wrong guard order fails fast instead of hanging; assert slot removed, dropped exactly once, never re-polled,
`pending_task_count() == 0`. Reddens if the outcome step unconditionally reinserts `task.future = Some(future)`
(resurrecting a self-cancelled future). Written against the `PumpGuard` shape.
