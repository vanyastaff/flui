# flui-animation Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn flui-animation from a silently-broken 7492-LOC engine into a market-ready, Rust-native animation engine: correct combinators + controller, one `Lerp` trait collapsing the tween explosion, interruptible velocity-springs with Apple presets, scroll physics, and a paint-only repaint seam.

**Architecture:** Object-safe `Animation<T>` trait kept (Arc<dyn> is load-bearing). One `ListenerRegistry` (value + status channels, RAII `Drop` subscriptions) replaces Flutter's 4-mixin lattice and structurally fixes the dead-listener bug. `Lerp`/`MaybeLerp` live in flui-geometry; one generic `Tween<V: Lerp>` replaces ~9 hand-rolled tweens. Controller stays `Send+Sync` (scoped ADR-0002 exception) but is built `!Send`-ready.

**Tech Stack:** Rust 1.95+ (edition 2024 idioms), parking_lot, smallvec, thiserror, tracing; `palette` (feature-gated OKLab), workspace `glam` (matrix decompose→slerp), flui-macros (`#[derive(Animatable)]`).

**Design spec:** [docs/research/2026-06-09-flui-animation-redesign.md](../../research/2026-06-09-flui-animation-redesign.md)
**Findings reference (exact Flutter contract + audit line-cites):** `.claude/anim-findings-reference.md`

---

## Scope & sequencing

8 PRs, sequential (later PRs depend on earlier results). This plan fully specifies **PR-1** (foundation `ListenerRegistry`) at step/code granularity — a self-contained, shippable, testable unit. **PR-2** is specified at task granularity. **PR-3–8** are the spec §6 roadmap; each is expanded to step/code granularity at execution time (their exact code depends on PR-1/PR-2 outcomes — specifying it now would be speculation, which violates the no-placeholder rule).

| PR | Deliverable | Granularity here |
|----|-------------|------------------|
| 1 | `Notifier<Arg>` + `ListenerRegistry<S>` + `Subscription` (flui-foundation) | **Full (this doc)** |
| 2 | Controller rescue (B1/B1b/B1c) + repeat/animateBack/fling + dt accumulator + ADR note | Task-level |
| 3 | `Lerp`/`MaybeLerp` in flui-geometry + generic `Tween<V>` + remove clamp | Roadmap |
| 4 | Combinator re-emit via registry + delete `DynAnimation` + const-LUT Cubic | Roadmap |
| 5 | `Matrix4::lerp` + `TextStyleTween`/`DecorationTween`/`GradientTween` (MaybeLerp) | Roadmap |
| 6 | Spring core: `AnimatedValue<T>` + `#[derive(Animatable)]` + Apple presets | Roadmap |
| 7 | Scroll physics: `ScrollSpring`/`Clamped`/`BoundedFriction` + `drag∈(0,1)` | Roadmap |
| 8 | Paint-only seam (flui-view) + Criterion benches + doc-accuracy sweep | Roadmap |

---

## Design of PR-1 (locked before tasks)

**`Notifier<Arg>`** — a generic typed hardened notification channel. Generalizes `ChangeNotifier` (which is effectively `Notifier<()>`) to carry a `Clone` argument to each listener. Self-contained: own id counter, listener map, disposed flag, and the exact firing discipline already proven in [`notifier.rs:299`](../../../crates/flui-foundation/src/notifier.rs) (snapshot → sort by id → drop lock → `catch_unwind` per callback → remove-during-notify skip → dispose guard). The status channel uses `Notifier<AnimationStatus>`; the value channel uses `Notifier<()>`.

> Why not refactor `ChangeNotifier` to `= Notifier<()>` now: `ChangeNotifier` has many consumers across the workspace; re-seating it is its own ripple. PR-1 adds `Notifier<Arg>` alongside and leaves `ChangeNotifier` untouched. A later consolidation PR can alias them.

**`ListenerRegistry<S>`** — composes `value: Notifier<()>` + `status: Notifier<S>`, plus a shared total-listener count (for the lazy first/last edges) and two owner-supplied edge hooks. Embedded by composition in every animation type; `Listenable` impl becomes one-line delegation. Crossing 0→1 total listeners fires `on_first_listener` (owner wires "subscribe to parent"); crossing 1→0 fires `on_last_listener` (owner tears the subscription down). This is the structural fix for B2 — the owner cannot forget to wire what the registry drives.

**`Subscription`** — RAII handle returned by `add_value_listener`/`add_status_listener`. Holds a `Weak` to the registry inner + a channel tag + the `ListenerId`. On `Drop`, removes itself and updates the shared count (firing `on_last` at 1→0). `Weak` so a dropped registry isn't resurrected. Replaces Flutter's leak-prone manual `dispose()`.

**Files:**
- Create: `crates/flui-foundation/src/notifier_generic.rs` (`Notifier<Arg>`)
- Create: `crates/flui-foundation/src/listener_registry.rs` (`ListenerRegistry<S>`, `Subscription`)
- Modify: `crates/flui-foundation/src/lib.rs` (module decls + re-exports)
- Tests: inline `#[cfg(test)] mod tests` in each new file.

---

## PR-1 Tasks

### Task 1: `Notifier<Arg>` — typed hardened channel

**Files:**
- Create: `crates/flui-foundation/src/notifier_generic.rs`
- Modify: `crates/flui-foundation/src/lib.rs`

- [ ] **Step 1: Declare the module + re-export**

In `crates/flui-foundation/src/lib.rs`, add next to the existing `pub mod notifier;` / `pub use notifier::...` lines:

```rust
pub mod notifier_generic;
pub use notifier_generic::Notifier;
```

- [ ] **Step 2: Write failing tests**

Create `crates/flui-foundation/src/notifier_generic.rs` with the test module first:

```rust
//! `Notifier<Arg>` — a generic, typed, hardened notification channel.
//!
//! Generalizes [`crate::notifier::ChangeNotifier`] (which is effectively
//! `Notifier<()>`) to deliver a `Clone` argument to each listener. Reuses the
//! same firing discipline: snapshot-under-lock, registration-order, drop-lock
//! before callbacks, per-callback `catch_unwind`, remove-during-notify skip,
//! and a dispose guard.

use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

use crate::id::ListenerId;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn delivers_arg_to_listener() {
        let n: Notifier<i32> = Notifier::new();
        let last = Arc::new(AtomicI32::new(0));
        let last2 = Arc::clone(&last);
        let _id = n.add(Arc::new(move |v: i32| last2.store(v, Ordering::SeqCst)));
        n.notify(7);
        assert_eq!(last.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn fires_in_registration_order() {
        let n: Notifier<()> = Notifier::new();
        let log = Arc::new(Mutex::new(Vec::<u8>::new()));
        for k in 0u8..3 {
            let log = Arc::clone(&log);
            let _ = n.add(Arc::new(move |()| log.lock().push(k)));
        }
        n.notify(());
        assert_eq!(*log.lock(), vec![0, 1, 2]);
    }

    #[test]
    fn panicking_listener_does_not_abort_rest() {
        let n: Notifier<()> = Notifier::new();
        let ran = Arc::new(AtomicUsize::new(0));
        let _ = n.add(Arc::new(|()| panic!("boom")));
        let r = Arc::clone(&ran);
        let _ = n.add(Arc::new(move |()| {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        n.notify(());
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn removed_during_notify_is_skipped() {
        let n: Notifier<()> = Notifier::new();
        let fired_b = Arc::new(AtomicUsize::new(0));
        let id_b_cell = Arc::new(Mutex::new(None::<ListenerId>));
        let n2 = n.clone();
        let cell2 = Arc::clone(&id_b_cell);
        let _a = n.add(Arc::new(move |()| {
            if let Some(id) = *cell2.lock() {
                n2.remove(id);
            }
        }));
        let fb = Arc::clone(&fired_b);
        let id_b = n.add(Arc::new(move |()| {
            fb.fetch_add(1, Ordering::SeqCst);
        }));
        *id_b_cell.lock() = Some(id_b);
        n.notify(());
        assert_eq!(fired_b.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn remove_and_len_and_dispose() {
        let n: Notifier<()> = Notifier::new();
        let id = n.add(Arc::new(|()| {}));
        assert_eq!(n.len(), 1);
        n.remove(id);
        assert_eq!(n.len(), 0);
        let _ = n.add(Arc::new(|()| {}));
        n.dispose();
        assert!(n.is_disposed());
        assert_eq!(n.len(), 0);
        n.dispose(); // idempotent — must not panic
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Notifier used after dispose")]
    fn notify_after_dispose_panics_in_debug() {
        let n: Notifier<()> = Notifier::new();
        n.dispose();
        n.notify(());
    }
}
```

- [ ] **Step 3: Run tests, verify they fail to compile (`Notifier` undefined)**

Run: `cargo test -p flui-foundation notifier_generic`
Expected: FAIL — `cannot find type Notifier`.

- [ ] **Step 4: Implement `Notifier<Arg>`**

Insert above the `#[cfg(test)]` module:

```rust
/// A listener callback that receives a `Clone` argument.
pub type ArgCallback<Arg> = Arc<dyn Fn(Arg) + Send + Sync + 'static>;

/// A generic, typed, hardened notification channel. See module docs.
#[derive(Clone)]
pub struct Notifier<Arg> {
    listeners: Arc<Mutex<HashMap<ListenerId, ArgCallback<Arg>>>>,
    next_id: Arc<AtomicUsize>,
    is_disposed: Arc<AtomicBool>,
}

impl<Arg> Default for Notifier<Arg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Arg> Notifier<Arg> {
    /// Create an empty notifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicUsize::new(1)),
            is_disposed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn mint_id(&self) -> ListenerId {
        ListenerId::new(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Whether [`dispose`](Self::dispose) has been called (shared across clones).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.is_disposed.load(Ordering::Acquire)
    }

    #[inline]
    fn check_disposed(&self) -> bool {
        if self.is_disposed.load(Ordering::Acquire) {
            #[cfg(debug_assertions)]
            panic!("Notifier used after dispose");
            #[allow(unreachable_code)]
            {
                tracing::warn!("Notifier used after dispose");
                return true;
            }
        }
        false
    }

    /// Register a listener; returns its id.
    pub fn add(&self, listener: ArgCallback<Arg>) -> ListenerId {
        if self.check_disposed() {
            return self.mint_id();
        }
        let id = self.mint_id();
        self.listeners.lock().insert(id, listener);
        id
    }

    /// Remove a previously registered listener.
    pub fn remove(&self, id: ListenerId) {
        if self.check_disposed() {
            return;
        }
        self.listeners.lock().remove(&id);
    }

    /// Remove all listeners.
    pub fn remove_all(&self) {
        if self.check_disposed() {
            return;
        }
        self.listeners.lock().clear();
    }

    /// Number of registered listeners.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.listeners.lock().len()
    }

    /// Whether there are no listeners.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.listeners.lock().is_empty()
    }

    /// Discard listeners and mark disposed. Idempotent.
    pub fn dispose(&self) {
        if self.is_disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.listeners.lock().clear();
    }
}

impl<Arg: Clone> Notifier<Arg> {
    /// Fire every listener with `arg`, in registration order.
    ///
    /// Mirrors [`ChangeNotifier::notify_listeners`](crate::notifier::ChangeNotifier::notify_listeners):
    /// snapshot under lock, drop the lock, re-check each listener's live
    /// registration (skip if removed mid-notify), `catch_unwind` per callback.
    pub fn notify(&self, arg: Arg) {
        if self.check_disposed() {
            return;
        }
        let mut snapshot: smallvec::SmallVec<[(ListenerId, ArgCallback<Arg>); 4]> = self
            .listeners
            .lock()
            .iter()
            .map(|(&id, cb)| (id, Arc::clone(cb)))
            .collect();
        snapshot.sort_unstable_by_key(|(id, _)| *id);

        for (id, callback) in &snapshot {
            if !self.is_disposed.load(Ordering::Acquire)
                && !self.listeners.lock().contains_key(id)
            {
                continue;
            }
            if let Err(payload) =
                catch_unwind(AssertUnwindSafe(|| callback(arg.clone())))
            {
                tracing::error!(
                    listener_id = ?id,
                    panic_payload = ?payload,
                    "Notifier listener panicked; continuing"
                );
            }
        }
    }
}
```

- [ ] **Step 5: Run tests, verify PASS**

Run: `cargo test -p flui-foundation notifier_generic`
Expected: PASS (all 6 tests). If `ListenerId::new`/`get` signature differs, adjust per `crates/flui-foundation/src/id.rs`.

- [ ] **Step 6: Lint + format**

Run: `cargo clippy -p flui-foundation --all-targets -- -D warnings && cargo fmt -p flui-foundation`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/flui-foundation/src/notifier_generic.rs crates/flui-foundation/src/lib.rs
git commit -m "feat(flui-foundation): add generic typed Notifier<Arg> channel"
```

---

### Task 2: `ListenerRegistry<S>` + `Subscription` — unified value+status with first/last edges + RAII

**Files:**
- Create: `crates/flui-foundation/src/listener_registry.rs`
- Modify: `crates/flui-foundation/src/lib.rs`

- [ ] **Step 1: Declare the module + re-exports**

In `crates/flui-foundation/src/lib.rs`:

```rust
pub mod listener_registry;
pub use listener_registry::{ListenerRegistry, Subscription};
```

- [ ] **Step 2: Write failing tests** (first/last edges, RAII drop, value+status independence, dispose)

Create `crates/flui-foundation/src/listener_registry.rs`:

```rust
//! `ListenerRegistry<S>` — unified value + status listener registry with
//! lazy first/last edge hooks and RAII `Subscription` teardown.
//!
//! Collapses Flutter's 4-mixin listener lattice (Lazy/Eager + LocalListeners +
//! LocalStatusListeners, one shared count) into one composed type. Crossing
//! 0→1 total listeners fires `on_first_listener` (owners wire "subscribe to
//! parent" here); crossing 1→0 fires `on_last_listener` (tear down). Dropping a
//! `Subscription` removes the listener — the structural fix for the dead
//! combinator-listener bug.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};

use parking_lot::Mutex;

use crate::id::ListenerId;
use crate::notifier_generic::{ArgCallback, Notifier};

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + Clone) {
        let c = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&c);
        (c, move || {
            c2.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn first_listener_edge_fires_once() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let firsts = Arc::new(AtomicUsize::new(0));
        let f2 = Arc::clone(&firsts);
        reg.set_on_first_listener(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        });
        let s1 = reg.add_value_listener(Arc::new(|| {}));
        let s2 = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(firsts.load(Ordering::SeqCst), 1, "first edge fires once");
        drop(s1);
        drop(s2);
    }

    #[test]
    fn last_listener_edge_fires_on_drop_to_zero() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let lasts = Arc::new(AtomicUsize::new(0));
        let l2 = Arc::clone(&lasts);
        reg.set_on_last_listener(move || {
            l2.fetch_add(1, Ordering::SeqCst);
        });
        let s1 = reg.add_value_listener(Arc::new(|| {}));
        let s2 = reg.add_status_listener(Arc::new(|_s: u8| {}));
        assert_eq!(lasts.load(Ordering::SeqCst), 0);
        drop(s1);
        assert_eq!(lasts.load(Ordering::SeqCst), 0, "still 1 listener");
        drop(s2);
        assert_eq!(lasts.load(Ordering::SeqCst), 1, "last edge at 1->0");
    }

    #[test]
    fn shared_count_spans_value_and_status() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let firsts = Arc::new(AtomicUsize::new(0));
        let f2 = Arc::clone(&firsts);
        reg.set_on_first_listener(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        });
        let _s = reg.add_status_listener(Arc::new(|_s: u8| {})); // status counts
        let _v = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(firsts.load(Ordering::SeqCst), 1, "one shared first edge");
        assert_eq!(reg.listener_count(), 2);
    }

    #[test]
    fn notify_value_and_status_independent() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let (vc, vcb) = counter();
        let _v = reg.add_value_listener(Arc::new(vcb));
        let sc = Arc::new(AtomicUsize::new(0));
        let sc2 = Arc::clone(&sc);
        let _s = reg.add_status_listener(Arc::new(move |s: u8| {
            sc2.fetch_add(s as usize, Ordering::SeqCst);
        }));
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1);
        assert_eq!(sc.load(Ordering::SeqCst), 0, "value notify != status");
        reg.notify_status(5);
        assert_eq!(sc.load(Ordering::SeqCst), 5);
        assert_eq!(vc.load(Ordering::SeqCst), 1, "status notify != value");
    }

    #[test]
    fn drop_subscription_stops_delivery() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let (vc, vcb) = counter();
        let s = reg.add_value_listener(Arc::new(vcb));
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1);
        drop(s);
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1, "dropped sub does not fire");
    }

    #[test]
    fn subscription_outliving_registry_is_safe() {
        let s = {
            let reg: ListenerRegistry<u8> = ListenerRegistry::new();
            reg.add_value_listener(Arc::new(|| {}))
            // reg dropped here; Subscription holds a Weak
        };
        drop(s); // must not panic / use-after-free
    }
}
```

- [ ] **Step 3: Run tests, verify they fail to compile**

Run: `cargo test -p flui-foundation listener_registry`
Expected: FAIL — `ListenerRegistry`/`Subscription` undefined.

- [ ] **Step 4: Implement `ListenerRegistry<S>` + `Subscription`**

Insert above the test module:

```rust
/// Which channel a [`Subscription`] belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Value,
    Status,
}

type EdgeHook = Box<dyn FnMut() + Send>;

struct RegistryInner<S> {
    value: Notifier<()>,
    status: Notifier<S>,
    count: AtomicUsize,
    on_first: Mutex<Option<EdgeHook>>,
    on_last: Mutex<Option<EdgeHook>>,
}

impl<S> RegistryInner<S> {
    fn after_add(&self) {
        // 0 -> 1 transition fires on_first.
        if self.count.fetch_add(1, Ordering::AcqRel) == 0
            && let Some(hook) = self.on_first.lock().as_mut()
        {
            hook();
        }
    }

    fn after_remove(&self) {
        // 1 -> 0 transition fires on_last.
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1
            && let Some(hook) = self.on_last.lock().as_mut()
        {
            hook();
        }
    }
}

/// Unified value + status listener registry. See module docs.
///
/// `S` is the status argument type (e.g. `AnimationStatus`). It must be `Clone`
/// to fan out to status listeners. No `Send + Sync` bound is required on the
/// registry itself beyond what the channels impose.
pub struct ListenerRegistry<S> {
    inner: Arc<RegistryInner<S>>,
}

impl<S> Default for ListenerRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Clone for ListenerRegistry<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<S> ListenerRegistry<S> {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                value: Notifier::new(),
                status: Notifier::new(),
                count: AtomicUsize::new(0),
                on_first: Mutex::new(None),
                on_last: Mutex::new(None),
            }),
        }
    }

    /// Set the hook fired when the total listener count crosses 0 → 1.
    /// Owners wire "subscribe to parent" here.
    pub fn set_on_first_listener(&self, f: impl FnMut() + Send + 'static) {
        *self.inner.on_first.lock() = Some(Box::new(f));
    }

    /// Set the hook fired when the total listener count crosses 1 → 0.
    /// Owners tear down the parent subscription here.
    pub fn set_on_last_listener(&self, f: impl FnMut() + Send + 'static) {
        *self.inner.on_last.lock() = Some(Box::new(f));
    }

    /// Total registered listeners across both channels.
    #[must_use]
    #[inline]
    pub fn listener_count(&self) -> usize {
        self.inner.count.load(Ordering::Acquire)
    }

    /// Register a value listener (zero-arg). Returns a RAII [`Subscription`].
    pub fn add_value_listener(
        &self,
        cb: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Subscription {
        let id = self.inner.value.add(cb);
        self.inner.after_add();
        Subscription {
            registry: Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>,
            channel: Channel::Value,
            id,
        }
    }

    /// Register a status listener (receives `S`). Returns a RAII [`Subscription`].
    pub fn add_status_listener(&self, cb: ArgCallback<S>) -> Subscription {
        let id = self.inner.status.add(cb);
        self.inner.after_add();
        Subscription {
            registry: Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>,
            channel: Channel::Status,
            id,
        }
    }

    /// Whether any listener is registered.
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        self.listener_count() > 0
    }

    /// Dispose both channels. Subsequent notifies are no-ops (debug-panic).
    pub fn dispose(&self) {
        self.inner.value.dispose();
        self.inner.status.dispose();
    }
}

impl<S: Clone> ListenerRegistry<S> {
    /// Fire all value listeners.
    pub fn notify_value(&self) {
        self.inner.value.notify(());
    }

    /// Fire all status listeners with `status`.
    pub fn notify_status(&self, status: S) {
        self.inner.status.notify(status);
    }
}

/// Object-safe removal hook so a `Subscription` can drop without knowing `S`.
trait RemoveFrom: Send + Sync {
    fn remove(&self, channel: Channel, id: ListenerId);
}

impl<S: Send + Sync + 'static> RemoveFrom for RegistryInner<S> {
    fn remove(&self, channel: Channel, id: ListenerId) {
        match channel {
            Channel::Value => self.value.remove(id),
            Channel::Status => self.status.remove(id),
        }
        self.after_remove();
    }
}

/// RAII handle: dropping it removes the listener and updates the shared count
/// (firing `on_last_listener` at the 1 → 0 edge). Holds a `Weak` so a dropped
/// registry is never resurrected.
#[must_use = "dropping the Subscription immediately removes the listener"]
pub struct Subscription {
    registry: Weak<dyn RemoveFrom>,
    channel: Channel,
    id: ListenerId,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            reg.remove(self.channel, self.id);
        }
    }
}
```

> **Implementation note (verify at execution):** the `Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>` unsizing coercion requires `RegistryInner<S>: RemoveFrom` with `S: Send + Sync + 'static`. Add those bounds to the `add_*` methods (`impl<S> ListenerRegistry<S>` → gate the two `add_*` on `where S: Send + Sync + 'static`, or move them into an `impl<S: Send + Sync + 'static>` block). The test uses `S = u8` which satisfies this.

- [ ] **Step 5: Run tests, verify PASS**

Run: `cargo test -p flui-foundation listener_registry`
Expected: PASS (6 tests). If the `let … && let …` let-chains trip MSRV, rewrite as nested `if let`.

- [ ] **Step 6: Lint + format + full-crate test**

Run: `cargo clippy -p flui-foundation --all-targets -- -D warnings && cargo fmt -p flui-foundation && cargo test -p flui-foundation`
Expected: clean, all foundation tests green (no regression in `notifier.rs`).

- [ ] **Step 7: Commit**

```bash
git add crates/flui-foundation/src/listener_registry.rs crates/flui-foundation/src/lib.rs
git commit -m "feat(flui-foundation): ListenerRegistry<S> with first/last edges + RAII Subscription"
```

---

### Task 3: PR-1 exit verification

- [ ] **Step 1: Workspace builds + foundation suite green**

Run: `cargo test -p flui-foundation && cargo clippy -p flui-foundation --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 2: Confirm exit criteria**
  - `Notifier<Arg>` delivers args, fires in order, isolates panics, skips removed-mid-notify, dispose-guards. ✓ (Task 1 tests)
  - `ListenerRegistry<S>` fires first edge once, last edge at 1→0, shares count across channels, value/status independent, RAII drop stops delivery, sub outlives registry safely. ✓ (Task 2 tests)
  - No change to existing `ChangeNotifier` behavior. ✓ (foundation suite green)

- [ ] **Step 3 (optional): open PR-1**

Per repo flow (`/pr` or `gh pr create`). Title: `feat(flui-foundation): ListenerRegistry + generic Notifier (animation redesign PR-1)`.

---

## PR-2 — Controller rescue (task-level)

**Goal:** fix B1/B1b/B1c; add `repeat(count,period,min,max)`, `animateBack`, `fling`; frame-coherent `dt` accumulator × `time_dilation`; lifecycle gating; embed PR-1 registry; delete dead scheduler field; ADR-0002 exception note; doc fixes.

**Files:** `crates/flui-animation/src/controller.rs` (primary), `src/builder.rs`, `src/animation.rs` (registry embed), `docs/adr/ADR-0002-engine-wide-threading-architecture.md` (amendment).

**Tasks (each test-first, frame-advance harness driving a manual Ticker):**
1. **Real tick.** Replace the `move |_elapsed| notifier.notify_listeners()` callback ([controller.rs:450](../../../crates/flui-animation/src/controller.rs)) with one that advances `inner.value` from elapsed time per the active simulation/curve, recomputes status, then notifies. *Test:* value advances monotonically across N manual ticks; ticker stops at completion (no leak).
2. **Per-run duration.** `animate_to(target, Option<Duration>)` uses the override for THIS run only — store it on the run/simulation, never write `inner.duration` ([controller.rs:439](../../../crates/flui-animation/src/controller.rs)). *Test:* base duration intact after a timed `animate_to`.
3. **Drop-lock-before-notify.** Snapshot status + drop the `MutexGuard` before `notify_status_listeners` ([controller.rs:455](../../../crates/flui-animation/src/controller.rs)). *Test:* a status callback that re-enters the controller does not deadlock (run under a timeout).
4. **`set_value` recomputes status** (Dismissed/Completed/Forward/Reverse) consistently.
5. **`repeat(count, period, min, max)` + `animateBack` + `fling`.** Port from `.flutter/.../animation_controller.dart` (`_RepeatingSimulation`, `fling` + `kFlingTolerance`/spring defaults). *Tests:* finite repeat count stops; period overrides; fling uses spring.
6. **Frame-coherent `dt` accumulator.** Seed from the vsync timestamp, scale by `scheduler.time_dilation()`, clamp to `k·frame_duration`. *Test:* dilation halves progress per wall-second; a long pause doesn't jump.
7. **Lifecycle gating.** Mute on Hidden/Paused; reset epoch on Resume. *Test:* paused controller doesn't advance.
8. **Embed `ListenerRegistry`.** Replace the controller's hand-rolled status `Vec`+counter with `ListenerRegistry<AnimationStatus>`; `Listenable` + status methods delegate. Delete the dead scheduler field. *Test:* status + value listeners both fire; drop semantics via `Subscription`.
9. **ADR-0002 amendment** documenting the scoped `Send+Sync` exception + the flip conditions. **Doc fixes:** `TweenAnimation::new` arg order, RwLock→Mutex, remove fictional widget-layer claim.

**Exit:** `animate_to` advances + preserves base duration + no deadlock; repeat/animateBack/fling correct; controller suite green; clippy clean.

---

## PR-3 – PR-8 — roadmap (expand at execution)

Each PR follows the same test-first discipline; exact code is written when the PR is reached (depends on prior PRs). Scope + exit criteria are fixed by the spec:

- **PR-3 Lerp + generic Tween** — spec §3 D4 / §6.3. `Lerp`/`MaybeLerp` in flui-geometry (blanket over `GeometryOps` + hand impls); flui-types impls (Color/Alignment/BorderRadius); collapse `*Tween` → `Tween<V: Lerp>` + aliases; **remove value-layer clamp** + once-only endpoint short-circuit; rename tween method `lerp`→`transform` (collision); `Color::lerp` u8 round; `TweenSequence` `partition_point`. Migrate all call sites (no shim).
- **PR-4 Combinator re-emit** — spec §6.4. Wire curved/reverse/compound/proxy/switch via `registry.set_on_first_listener(subscribe-to-parent)` + RAII drop in `on_last`; `CompoundAnimation` listens to both children; delete `DynAnimation`; const-LUT `Cubic`; delete dead `ParametricCurve`.
- **PR-5 MaybeLerp + missing tweens** — spec §6.5. `Matrix4::lerp` (glam decompose→slerp); `RelativeRect`; `TextStyleTween`; `DecorationTween`/`GradientTween` (MaybeLerp).
- **PR-6 Spring core** — spec §3 D5 / §6.6. Reconcile `with_duration_and_bounce` bounce<0; `AnimatedValue<T>` per-component (pos,vel) springs + velocity-preserving retarget; `#[derive(Animatable)]`/`TwoWayConverter` (assoc `type Vector`) in flui-macros; Apple `smooth/snappy/bouncy` + `with_response_and_damping` default; f64-internal constants; unit-aware rest epsilon via `distance()`.
- **PR-7 Scroll physics** — spec §6.7. `ScrollSpringSimulation`, `ClampedSimulation`, `BoundedFrictionSimulation`, `FrictionSimulation::through`, `drag∈(0,1)` assert (fix never-terminate hang).
- **PR-8 Paint-only seam + credibility** — spec §3 D6 / §6.8. `create_mark_needs_paint_callback` in flui-view + listener→`add_node_needing_paint` bridge + paint-vs-layout partition + direct integration test (repaint-not-rebuild, counters); committed Criterion benches; doc-accuracy sweep.

---

## Cross-cutting standards (every PR)

- TDD: failing test → minimal impl → green → refactor → commit. Dynamic-path coverage is the #1 gap — test first.
- `cargo clippy --all-targets -- -D warnings` + `cargo fmt` clean per crate before commit.
- No shims/stubs/TODOs; finish cross-crate ripples (no-quick-wins).
- Diagnostics through `flui_foundation::log`/`tracing`, not `println!`.
- Atomic commits, one logical unit each; PR per the table above.
- Observability + docs ship in the same PR as the code.
