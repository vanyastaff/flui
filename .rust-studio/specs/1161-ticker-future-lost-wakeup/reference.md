# Reference reading — #1161 ticker futures

`.flutter` verified at tag **3.44.0** (`git -C .flutter describe --tags`).
Source: `packages/flutter/lib/src/scheduler/ticker.dart`, `class TickerFuture`.

## The structural fact that reframes defect (1)

Flutter's `TickerFuture` is built on `Completer`, and **the lost-wakeup class does not exist there
at all** — there is no listener to register, so there is no window between observing the state and
subscribing to a change. An awaiter attached before or after `complete()` is resolved either way.

FLUI's lost wakeup is therefore **not** a missed edge case of the reference. It is introduced by
modelling a *level* fact — a once-only, monotone state transition — with an *edge* primitive
(`event_listener::Event`), whose own doc says a notification with no listener registered "simply
gets lost". The design rule that follows is the same one #1055 arrived at from the other end: **the
durable state is the source of truth; the notification is only a hint to re-read it.** Any fix that
leaves resolution depending on catching the edge is the same bug with a smaller window.

## Contract points to match or diverge from deliberately

| Reference | FLUI today |
|---|---|
| `_completed`: `null` unresolved / `true` complete / `false` canceled | `TickerFutureState::{Pending, Complete, Canceled}` — same tri-state |
| `_complete()` / `_cancel()` open with `assert(_completed == null)` — a **debug assertion** that the transition happens once | `if *state == TickerFutureState::Pending { … }` — a **silent no-op** on a second call |
| `_cancel` completes only the *secondary* completer; the primary future never resolves on cancel | matched: the base future resolves only on `Complete`, `or_cancel` carries the error |
| `orCancel` is lazily created, and if accessed **after** resolution completes immediately with the recorded outcome | to verify against `TickerFuture::or_cancel` |
| `whenCompleteOrCancel(cb)` is `orCancel.then(thunk, onError: thunk)` — **registers a continuation and returns immediately** | `when_complete_or_cancel` **blocks the calling thread** (`listener.wait()`), and says so in its own doc |

## Two divergences that owe a record and do not have one

`crates/flui-scheduler/ARCHITECTURE.md` `## Mapping decisions` has exactly one ticker entry (the
#1160 callback-slot state machine). Neither of these appears anywhere:

1. **`when_complete_or_cancel` blocks; Flutter's does not.** A caller who expects Flutter's
   fire-and-forget registration gets a parked thread. On wasm32 the code already cannot block and
   calls the callback immediately instead — which is a *third* behaviour, and also unrecorded.
2. **A second `set_complete`/`set_canceled` is silently ignored where Flutter asserts.** Under the
   Prime Directive that is a deliberate improvement or a lost edge case, and right now it is
   neither recorded nor tested.

Both are in this fix's blast radius: (1) because the blocking path is the one place in the file that
already registers before re-checking, so it is the shape the async paths must adopt; (2) because the
once-only transition is exactly the invariant the fix depends on.

## What the reference does NOT settle

Flutter is single-isolate, so it has nothing to say about: a waker woken under a lock, a panicking
waker starving siblings, or a cancelled await releasing executor resources. Those three are
FLUI-only obligations, and the sibling precedent is `scheduler.rs` after #1163 — not `ticker.dart`.
