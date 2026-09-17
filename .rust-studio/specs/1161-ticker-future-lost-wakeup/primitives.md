# Primitive survey — #1161

Every claim below was read from the crate source at the version in `Cargo.lock`, not from docs prose.

## My strongest hypothesis was wrong

I briefed this expecting `event-listener-strategy`'s `easy_wrapper!` to make the racy order
**unrepresentable by construction** — which would have been the decisive argument. It does not.
All 549 lines of `event-listener-strategy-0.5.4/src/lib.rs` emit exactly five things: a
`pin_project!`ed newtype, a `_new` ctor, a blocking `wait()`, a `poll_with_strategy` forwarder, and
`impl Future`. **No state-machine logic whatsoever.** The ordering is written by hand by every user;
a user of the macro can write our exact race and it compiles and passes.

## What actually closes the race, and where to read it

`async-broadcast-0.7.2/src/lib.rs`, `RecvInner::poll_with_strategy`. The mechanism is that
**registration and suspension are separate loop iterations**:

```rust
loop {
    match this.receiver.try_recv() { Ok(m) => return Ready(m), Err(Empty) => {} }
    match &this.listener {
        None => {
            *this.listener = Some(inner.recv_ops.listen());   // register, then FALL THROUGH
        }                                                     // ← loops back to the state read
        Some(_) => { ready!(strategy.poll(this.listener, cx)); *this.listener = None; }
    }
}
```

The `None` arm registers and **does not poll** — control returns to the top and re-reads state with
the listener already enrolled. Our `poll` registers and polls in the same arm, which is the whole
bug. `async-lock-3.4.2/src/once_cell.rs`'s `wait()` is a second reference for the same idiom
(`get()` → `listener!` → `get()` again → `await`).

## The upgrade I did not know to ask for

`event_listener::Event<T>` is generic over a **tag**: `Event::<T>::with_tag()`,
`notify(usize::MAX.tag(value))` (`notify.rs`), and the listener's `Output` is `T`. So the resolution
(`Complete` / `Canceled`) can ride the wake instead of forcing the woken awaiter to re-acquire a
lock and re-read. That in turn lets the tri-state become an `AtomicU8` and **deletes the
`parking_lot::Mutex` from `TickerFutureInner` entirely**.

## Property table (source-verified)

| Candidate | Wakes under its own lock? | Dropped awaiter deregisters? | Delivers a value? | Panicking waker starves the rest? | Runtime-free? |
|---|---|---|---|---|---|
| **`event-listener` 5.4.2** | **Yes** — `with_inner` holds the list lock across `f`, `Inner::notify` calls `task.wake()` inside | Yes, **O(1)** intrusive unlink on `PinnedDrop` | **Yes**, via `Event<T>` tags | **Yes, and here permanently** — the loop unwinds, later entries keep `State::Task`, and once-only means nothing re-notifies | Yes, `no_std`-capable |
| `event-listener-strategy` | inherits | inherits | inherits | inherits | Yes |
| `async-lock::OnceCell` | **Yes**, inherited (`notify_additional` → same path) | Yes, O(1) | `&T` — self-borrow forces boxing | inherits | Yes |
| `async-broadcast` | **Yes**, inherited — built on event-listener | Yes, O(1) | Yes | inherits | Yes |
| `futures-channel::oneshot` (N of them) | **No** — every wake site drops the lock first; the "lock" is a non-blocking `try_lock` | **Lazily, and leaks in the registry** until resolution | Yes, `Result<T, Canceled>` | No at the primitive; yes at our fan-out loop unless each `send` is caught | Yes |
| `tokio::sync::{watch, Notify, SetOnce}` | **No** — writes drop the guard, then `notify_waiters` batches and wakes after `drop(waiters)` | Yes, O(1) intrusive | `SetOnce::wait() -> &T` — self-borrow again | **Yes, quietly** — `WakeList`'s drop guard unlinks the remainder *without* waking, "to avoid double panics" | Yes; `sync = []` pulls no runtime |
| hand-rolled `Mutex<Vec<Waker>>` | our choice — the only fully panic-isolating option | only if designed; O(1) needs slot tokens + tombstones, i.e. a slab | trivially | no, if written right | Yes |

## Supply-chain cost, measured against this crate's own closure

`cargo tree -p flui-scheduler -e normal` is **30 crates**, and it already contains `event-listener`.
It does **not** contain `event-listener-strategy`, `async-lock`, `async-broadcast`, `futures-channel`
or `tokio` — so "already in `Cargo.lock`" was the wrong denominator; the per-package `feature-matrix`
job builds this closure, not the workspace's.

- `event-listener` — **+0 crates**
- `event-listener-strategy` +1, `async-lock` +2 — both in the default host graph through **exactly
  one edge**: `flui-assets → moka`. Drop moka and a "free" choice silently becomes new.
- `async-broadcast` +3, `futures-channel` +2 — **not in the default host graph at all**
  (`cargo tree -i` errors); reachable only via the a11y/zbus and reqwest paths.
- `tokio` with `features = ["sync"]` +1, and it compiles tokio plus `pin-project-lite` and nothing
  else. But `flui-scheduler` is inside `just wasm-check`'s set, and **both `flui-app` and
  `flui-platform` deliberately target-gate tokio behind `cfg(not(target_arch = "wasm32"))`** — this
  would be the framework core's first tokio edge and would put it into the wasm32 build. Size proxy:
  `tokio/src/{sync,util}` is 22,075 lines against all of `event-listener/src` at 2,737.

## Recommendation and the rejections behind it

**Keep `event-listener`; fix the loop shape. Zero manifest change.** Split the register arm from the
poll arm as `async-broadcast` does. Optionally take the tag upgrade in the same pass.

Rejected, each on something falsifiable rather than taste:

- **`tokio::sync::SetOnce`** — the only candidate whose *primitive* is a monotone one-shot with the
  correct order baked into its own `set`/`wait`, and it does not wake under a lock. Rejected because
  it inverts a target gate two crates apply deliberately, and because `wait(&self) -> &T`
  self-borrows, so a named `Clone`-able future must box an async block per awaiter.
- **`event-listener-strategy`** — rejected *as a fix* because it fixes nothing (above). Worth
  adopting only to dedup the `cfg(wasm32)` dance in `when_complete_or_cancel`.
- **`async-lock::OnceCell`** — same wake-under-lock path, +2 crates, same boxing. Strictly dominated.
- **`futures-channel::oneshot`** — genuinely viable and the only one that wakes with no lock at all,
  but `TickerFuture: Clone` makes the awaiter count unbounded, the registry needs its own pruning
  because a dropped awaiter is detected only when `send` fails, and it is +2 crates not in the
  default graph.
- **`async-broadcast`** — inherits wake-under-lock verbatim; useful only as the idiom to copy.
- **hand-rolled** — beats everything on properties 1, 2 and 4 simultaneously, but O(1) deregistration
  means slot tokens and tombstones, i.e. re-implementing an intrusive list with none of
  `event-listener`'s loom coverage.

## The residual, stated rather than buried

Keeping `event-listener` keeps **wake-under-lock**, and a panicking waker starves every awaiter
registered after it — **permanently**, because the transition is once-only and nothing re-notifies.
That is the same failure shape as the bug being fixed, arrived at differently.

It is not unavoidable: the notify loop advances its cursor and marks each entry `Notified` *before*
waking, and the list lock is poison-tolerant, so a `catch_unwind` around `notify` **retried until it
returns cleanly** resumes at the next un-notified entry and terminates. That is exactly the
containment `notify_frame_completion` carries (#1158), and the plan should take it rather than
accept the residual.

## Checked and found absent, as distinct from not checked

`async-event`, `diatomic-waker`, `oneshot` are in no lock entry. `event-listener 5.4.2/src` has only
three files — there is no alternative non-intrusive list, so wake-under-lock is unconditional, not a
`cfg` branch. `event-listener-strategy/src` is a single file with no hidden ordering helper.
**No existing test in `ticker.rs` polls a `TickerFuture` with a real waker and then resolves it**, so
no named test fails on this bug today and none would fail if a fix regressed.

Not checked: nothing was compiled (read-only), so every cost figure is a line-count or graph
-membership proxy, never a measured build time or binary delta.

## A micro-finding, out of scope, not yet filed

`TickerFuture::poll`'s `Canceled` arm registers a listener that can never fire. That is a **faithful
port** — Flutter's `_cancel` completes only the secondary completer and never the primary, so its
base future genuinely never resolves either — but the boxed `EventListener` is then held forever for
nothing. The arm should return `Pending` without registering.
