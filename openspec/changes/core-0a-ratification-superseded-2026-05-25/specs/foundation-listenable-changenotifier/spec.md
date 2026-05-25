# Foundation — Listenable / ChangeNotifier Specification

## Purpose

Pin the canonical contract for FLUI's observer-pattern primitives — the
`Listenable` trait, `ChangeNotifier` reference implementation,
`ValueListenable<T>` + `ValueNotifier<T>` value-holding variant,
`ListenerCallback` type alias, and `ListenerId` registration handle — at
parity with Flutter's `change_notifier.dart` while documenting Rust-native
shape divergences (HashMap-based listener storage, snapshot-then-fire
allocation discipline, idempotent `dispose`).

Cycle 3 (PRs #102–#106 + Polish) closed the audit findings against this
surface (I-1 `ObserverList` delete; I-4 SmallVec snapshot; I-16 explicit
`'static`; I-20 `into_value` dispose; I-22 `WasmNotSend` delete). This spec
codifies the resulting behaviour as the canonical contract so future
audits and downstream changes (Core.0b, Core.0c) do not re-litigate
disposition or shape decisions.

Owner crate: `crates/flui-foundation` — modules `notifier.rs`,
`callbacks.rs`, `wasm.rs`.

## Requirements

### Requirement: Listenable trait surface

The `Listenable` trait MUST expose exactly three methods —
`add_listener(&self, ListenerCallback) -> ListenerId`,
`remove_listener(&self, ListenerId)`,
`remove_all_listeners(&self)` — and MUST require `Send + Sync` so
listenables can be observed from any thread.

**Audit ref:** I-1 (closed — `ObserverList` deleted; `ChangeNotifier`
remains the canonical observer pattern; verdict ratified as permanent),
I-16 (closed — explicit `+ 'static` on `ListenerCallback` alias).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:70-82`
(`Listenable.addListener`, `Listenable.removeListener`).

**Rust-native divergence:**
- (a) FLUI adds `remove_all_listeners` (Flutter does not expose a public
  bulk-clear; Flutter's `dispose()` clears the internal list privately).
  This is additive — every Flutter consumer that needs bulk clear writes
  a wrapper; FLUI provides it natively.
- (b) FLUI's `Listenable` is `Send + Sync` (Flutter is single-threaded —
  the bound is meaningless in Dart). Forces every `ListenerCallback`
  closure to be `Send + Sync + 'static`. This is the cost of Rust's
  multi-threaded reality; the audit ratifies it as the price of
  parity-faithful behaviour on a parity-better runtime.
- (c) No FLUI consumer breaks — every active workspace adopter
  (`flui-animation` 8 files, `flui-scheduler::Ticker`) already conforms.

#### Scenario: Listenable trait dispatch via dyn dispatch

- GIVEN a `ChangeNotifier` registered with one listener
- WHEN the test code accesses it through `&dyn Listenable` (object-safe
  dispatch)
- THEN `add_listener` / `remove_listener` / `remove_all_listeners`
  through the trait object MUST behave identically to the inherent
  methods on `ChangeNotifier`

#### Scenario: Listenable requires Send + Sync

- GIVEN a generic function `fn assert_listenable<L: Listenable>(_: &L)`
- WHEN the function is instantiated with `ChangeNotifier` and
  `ValueNotifier<i32>`
- THEN compilation MUST succeed (proves the `Send + Sync` supertrait
  bound is satisfied by every workspace adopter)

---

### Requirement: ChangeNotifier dispose contract

`ChangeNotifier::dispose(&self)` MUST be **idempotent** (second and
later calls are no-ops, no panic), MUST clear all listeners, and MUST
take `&self` (not `&mut self`) so callback closures holding a clone may
dispose the notifier from inside a listener callback.

After `dispose()`, subsequent calls to `add_listener`, `notify_listeners`,
or `remove_listener` MUST panic in debug builds (via `debug_assert!`)
and MUST degrade to a `tracing::warn!` + no-op in release builds.

`is_disposed(&self) -> bool` MUST be the shared-state probe — it loads
from an `Arc<AtomicBool>` so every clone of a notifier observes the
same disposed state.

**Audit ref:** I-3 partial (BindingBase covered separately in
`foundation-binding`), the original "gold standard" cycle 1 PR #84
template (audit Mythos verdict, "Don't touch" entry #1). Disposed-state
sharing across clones was a cycle 3 design decision ratified here.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:376-401`
(`ChangeNotifier.dispose`) + `:139-181` (mixin class declaration +
`debugAssertNotDisposed`).

**Rust-native divergence:**
- (a) Flutter's `dispose()` is `@mustCallSuper` and not idempotent (a
  second call passes the `assert(_debugAssertNotDisposed(this))` only
  in release; debug panics). FLUI swaps an `AtomicBool` with `AcqRel`
  ordering — second `dispose()` sees `true` and returns. Idempotency
  is **stricter** than Flutter and necessary for the snapshot-then-fire
  reentrancy story (a listener can call `dispose()` on its own clone
  while `notify_listeners` is iterating the snapshot).
- (b) Flutter's `dispose()` takes `this` (instance method on a single-
  threaded mixin). FLUI's takes `&self` because the notifier is
  internally `Arc`-shared — `&mut self` would forbid the listener-
  during-iteration reentrancy.
- (c) No FLUI consumer breaks — the contract is unchanged from the
  PR #84 baseline that the workspace adopted in cycle 1.

#### Scenario: Idempotent dispose returns without panic

- GIVEN a `ChangeNotifier` with three registered listeners
- WHEN `dispose()` is called twice in sequence on the same notifier
- THEN the first call clears all listeners and flips `is_disposed()`
  to `true`; the second call returns without panic and `is_disposed()`
  remains `true`

#### Scenario: Disposed-state shared across clones

- GIVEN a `ChangeNotifier` `n1` and `let n2 = n1.clone();`
- WHEN `n1.dispose()` is called
- THEN `n2.is_disposed()` MUST return `true` (proves the disposed
  flag is `Arc<AtomicBool>` shared, not per-clone)

#### Scenario: Use after dispose is debug-panic / release-warn

- GIVEN a disposed `ChangeNotifier`
- WHEN `add_listener`, `notify_listeners`, or `remove_listener` is
  called on it
- THEN in `#[cfg(debug_assertions)]` builds a panic with message
  containing "used after dispose" MUST be observed; in release builds
  a `tracing::warn!` event MUST be emitted and the call MUST be a
  no-op (verify via `tracing::subscriber::with_default` capture)

#### Scenario: Dispose from inside notify callback is reentrancy-safe

- GIVEN a `ChangeNotifier` `n` with a listener that captures
  `let n2 = n.clone();` and calls `n2.dispose()` inside its body
- WHEN `n.notify_listeners()` is called
- THEN the listener executes (because notify took a snapshot before
  iterating); after notify returns, `n.is_disposed()` MUST be `true`;
  a subsequent `notify_listeners()` MUST early-return per the
  use-after-dispose contract

---

### Requirement: ChangeNotifier snapshot-then-fire notify path

`ChangeNotifier::notify_listeners(&self)` MUST snapshot the listener
set under the internal mutex into a `SmallVec<[ListenerCallback; 4]>`
(stack-allocated inline-4, heap fallback for ≥5 listeners), release
the mutex, then iterate the snapshot calling each callback. The
snapshot-then-fire shape is the reentrancy guarantee — a callback that
adds, removes, or disposes listeners during notify MUST observe
consistent in-flight behaviour and MUST NOT corrupt the iterator.

**Audit ref:** I-4 (closed Wave 3 — per-frame `Vec` allocation
replaced with `SmallVec<[CB; 4]>` inline cap; verdict ratified as
permanent — the inline-4 sizing covers the typical UI notifier
listener count per the audit's Mythos verdict).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:413-465`
(`ChangeNotifier.notifyListeners` — Flutter iterates `_listeners`
fixed-size array with a `_count` cursor, no per-call alloc; FLUI's
snapshot pattern is the Rust-native equivalent).

**Rust-native divergence:**
- (a) Flutter stores listeners in a `List<VoidCallback?>` with a
  `_count` cursor and null-fills removed slots (O(N) compaction
  amortised). FLUI uses `HashMap<ListenerId, ListenerCallback>` for
  O(1) removal by stable ID. Trade-off recorded as Drift A in audit
  Part III: O(1) removal worth one allocator-free snapshot per
  notify when the notifier has ≤4 listeners.
- (b) FLUI's snapshot-then-fire is the *only* shape that gives
  reentrancy safety with a `HashMap` backing — Flutter's array shape
  can iterate the live `_listeners[0..count]` because null-checking
  the slot is the in-flight modification guard. The shapes are
  parity-faithful at the *observable behaviour* level (listener gets
  exactly one call per notify, in-flight add/remove is consistent),
  divergent at the *data-structure* level.
- (c) No FLUI consumer breaks — `notify_listeners` is `&self`, no
  signature changed.

#### Scenario: notify_listeners with ≤4 listeners makes zero heap allocations

- GIVEN a `ChangeNotifier` with 3 registered listeners
- WHEN `notify_listeners()` is called inside a measured-allocation
  scope (using a thread-local counting allocator or `dhat-rs`)
- THEN the recorded heap-allocation count for the notify call MUST
  be 0 (the `SmallVec` inline cap of 4 absorbs the snapshot)

#### Scenario: notify_listeners reentrancy-safe under listener add/remove

- GIVEN a `ChangeNotifier` `n` with listeners `[A, B]` where listener
  `A` captures `n.clone()` and during its body calls
  `n.add_listener(C)` and `n.remove_listener(b_id)`
- WHEN `n.notify_listeners()` is called once
- THEN exactly listeners `A` and `B` fire during this notify
  (snapshot was taken before `A` mutated the set); a subsequent
  `n.notify_listeners()` MUST then fire exactly `A` and `C`

#### Scenario: notify_listeners with N=8 listeners spills snapshot to heap without behavioural change

- GIVEN a `ChangeNotifier` with 8 registered listeners
- WHEN `notify_listeners()` is called
- THEN all 8 callbacks MUST fire exactly once each in unspecified
  order; the heap-allocation path is taken but observable behaviour
  is identical to the inline-cap path

---

### Requirement: Listener registration returns stable ListenerId

`ChangeNotifier::add_listener(&self, ListenerCallback) -> ListenerId`
MUST return a `ListenerId` that uniquely identifies the registration
for the lifetime of the notifier. The same callback registered twice
MUST yield two distinct `ListenerId`s (Flutter parity:
`addListener` is multi-registration tolerant). `remove_listener(id)`
of a removed-or-never-issued ID MUST be a no-op (not a panic).

**Audit ref:** post-cycle ratification (`ListenerId` is the
`Id<markers::Listener>` family member from `flui-foundation::id`,
covered separately by `foundation-id-system`; this requirement pins
its observable use in the notifier).

**Flutter ref:** Flutter has NO opaque-handle registration — the
`removeListener(VoidCallback)` API removes by callback identity
(reference equality on Dart `Function` values). FLUI's ID-handle
shape is a **deliberate Rust-native improvement** because Rust
closures don't have stable reference identity (each `||{}` is a
fresh type / address).

**Rust-native divergence:**
- (a) Flutter: `removeListener(VoidCallback listener)` — by callback
  reference.
- (b) FLUI: `remove_listener(ListenerId id)` — by stable handle. Rust
  closures cannot be compared by identity; the handle shape is the
  only path that works.
- (c) Consumers `flui-animation::Ticker` (PR #95) and any current /
  future widget binding `add_listener` call already store the
  returned `ListenerId` for paired removal — no breakage.

#### Scenario: Distinct registrations of the same callback yield distinct IDs

- GIVEN a `ChangeNotifier` and a single `ListenerCallback` value
  (`Arc::new(|| {})`)
- WHEN the callback is cloned and registered twice via
  `add_listener(cb.clone())` and `add_listener(cb.clone())`
- THEN the two returned `ListenerId`s MUST be distinct; both registrations MUST fire on `notify_listeners`; removing the first ID MUST leave the second registration intact

#### Scenario: Remove of unknown ListenerId is a silent no-op

- GIVEN a `ChangeNotifier`
- WHEN `remove_listener(ListenerId::new(usize::MAX))` is called
  against a never-issued ID
- THEN the call MUST return without panic; subsequent
  `notify_listeners` MUST still fire all real registrations

---

### Requirement: ValueNotifier holds a value and notifies on change

`ValueNotifier<T>` MUST implement `ValueListenable<T>` (which adds
`value(&self) -> &T` to `Listenable`). `set_value(&mut self, T)` MUST
fire `notify_listeners` exactly when the new value differs from the
current (per `T: PartialEq`). `into_value(self) -> T` MUST call
`dispose()` on the inner notifier before returning the value, so any
in-flight listener registrations are explicitly torn down rather
than silently dropped.

**Audit ref:** I-20 (closed Polish PR — `into_value` calls dispose;
verdict ratified as permanent), I-17 (deferred — `take` / `replace`
/ `value_mut` kept per cycle-3 rationale "Used by tests + internal
consumers; judgment call on which to drop"; verdict for this spec:
**accept-permanent** — keep the methods, future revival of the
"audit / mark unused" judgment is contingent on a real consumer
materialising or being removed).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:484-525`
(`ValueNotifier<T>`).

**Rust-native divergence:**
- (a) `into_value` has no Flutter equivalent (Dart has no
  consume-by-move). FLUI's choice to dispose-before-return mirrors
  the PR #84 explicit dispose template — silent drop of an Arc-shared
  notifier would deactivate listeners that have outstanding handles.
- (b) `take` / `replace` / `value_mut` (4 mutation methods) are
  FLUI-native ergonomic additions; Flutter exposes only the `value`
  setter.
- (c) No consumer breaks; `into_value` semantic change is observable
  only to a hypothetical caller relying on listeners-stay-alive
  semantics, of which there are zero today.

#### Scenario: set_value fires notify when value changes

- GIVEN a `ValueNotifier::new(42)` with one registered listener
  counting invocations
- WHEN `set_value(100)` is called
- THEN the listener fires exactly once and `value()` returns `&100`

#### Scenario: set_value does NOT fire notify when value equals current

- GIVEN a `ValueNotifier::new(42)` with one registered listener
- WHEN `set_value(42)` is called
- THEN the listener MUST NOT fire (PartialEq-equal write is a no-op)

#### Scenario: into_value disposes the notifier

- GIVEN a `ValueNotifier::new(42)`; capture `let n_clone =
  notifier_arc_handle.clone();` from the inner `ChangeNotifier`
- WHEN `notifier.into_value()` is called and returns `42`
- THEN the cloned handle's `is_disposed()` MUST return `true`
  (proves I-20 dispose-before-return)

---

### Requirement: ListenerCallback type carries explicit 'static + Send + Sync

`pub type ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>;`
MUST declare the `'static` bound explicitly. Same for every callback
alias in `callbacks.rs` (`VoidCallback`, `ValueChanged<T>`,
`ValueGetter<T>`, `ValueSetter<T>`, `Predicate<T>`,
`ValueTransformer<T, U>`, `FallibleCallback<T, E>`).

**Audit ref:** I-16 (closed Polish PR — explicit `'static` added;
verdict ratified as permanent).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:70`
(`typedef VoidCallback = void Function();` — Dart functions are
implicitly global / `'static`-equivalent).

**Rust-native divergence:**
- (a) None at observable behaviour; this is doc-clarity. Without the
  explicit `'static`, `dyn Trait` syntax elides the lifetime to
  `'static` anyway, but the elision is non-obvious to callers
  constructing the `Arc::new(|| { /* captures */ })`.
- (b) Explicit bound documents the requirement that captures must be
  `'static` — caught at the `Arc::new(...)` call site rather than
  surfacing as a cryptic borrow checker error later.
- (c) No consumer breaks — every existing call site already
  conforms.

#### Scenario: ListenerCallback alias is verbatim Arc<dyn Fn() + Send + Sync + 'static>

- GIVEN the source file `crates/flui-foundation/src/notifier.rs`
- WHEN searched for `pub type ListenerCallback`
- THEN exactly one match MUST appear and its right-hand side MUST be
  exactly `Arc<dyn Fn() + Send + Sync + 'static>` (regex-match in a
  unit test or `grep`-based CI gate)

---

### Requirement: ObserverList is deleted (no future re-introduction without consumer)

The `ObserverList<T>` type that existed pre-cycle-3 at
`crates/flui-foundation/src/observer.rs` MUST NOT exist in the
crate's public or private surface. `crates/flui-foundation/src/lib.rs`
MUST NOT declare a `pub mod observer;` line. The prelude MUST NOT
re-export `ObserverList`.

Re-introduction is gated on a real in-workspace consumer
materialising. If a future devtools / hot-reload workstream needs
the type, it ports from git history (commit predating PR #105
deletion). The audit's deferred-finding table lists no related
follow-up; this spec records the deletion verdict as
**accept-permanent**.

**Audit ref:** I-1 (closed Wave 1+2 — `ObserverList` deleted;
verdict ratified as permanent per
`no-quick-wins-vanyastaff` memory rule).

**Flutter ref:** Flutter `ObserverList` lives in
`.flutter/packages/flutter/lib/src/foundation/observer_list.dart`
(`class ObserverList<T>`). FLUI deliberately diverges: every FLUI
adopter that would have wanted Flutter's `ObserverList` (in-place
mutation, automatic compaction) uses `ChangeNotifier` instead, which
provides O(1) removal by handle and the snapshot-then-fire
reentrancy story.

**Rust-native divergence:**
- (a) Flutter exposes `ObserverList<T>` as a public foundation type
  (8 in-tree consumers including `_LayerLink._connectedLinks`).
- (b) FLUI deletes it because every FLUI consumer that needs an
  observer-pattern collection uses `ChangeNotifier` or its own
  `HashMap<Handle, T>` (`flui-scheduler` for ticker registrations,
  `flui-layer` for layer-link bookkeeping). Carrying a parity type
  with zero consumers violates the
  `no-quick-wins-vanyastaff` memory rule.
- (c) Zero FLUI consumers break — confirmed by cycle-3 grep at
  audit Appendix A.2.

#### Scenario: observer.rs file does not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-foundation/src/observer.rs` is run
- THEN it MUST exit non-zero (file absent)

#### Scenario: lib.rs does not declare observer module

- GIVEN `crates/flui-foundation/src/lib.rs`
- WHEN searched for `pub mod observer`
- THEN zero matches MUST appear

#### Scenario: ObserverList symbol unresolvable from public API

- GIVEN a downstream-style test crate that depends on `flui-foundation`
- WHEN it attempts to `use flui_foundation::ObserverList;`
- THEN the build MUST fail with an unresolved-import error

---

### Requirement: WasmNotSend is deleted; WasmNotSendSync is the sole wasm-compat marker

`crates/flui-foundation/src/wasm.rs` MUST expose only `WasmNotSendSync`
(the load-bearing supertrait of `Marker`). `WasmNotSend` (the
Send-only variant that existed pre-cycle-3) MUST NOT exist.

**Audit ref:** I-22 (closed Wave 1+2 — `WasmNotSend` deleted;
verdict ratified as permanent).

**Flutter ref:** No Flutter equivalent (Dart is single-threaded;
WASM-without-threads is the only "platform" model; FLUI's wasm
markers are Rust-native plumbing for cfg-conditional Send/Sync
bounds).

**Rust-native divergence:** Pure cleanup. Zero consumers.

#### Scenario: WasmNotSendSync exists, WasmNotSend does not

- GIVEN `crates/flui-foundation/src/wasm.rs`
- WHEN searched for `pub trait WasmNotSendSync` and `pub trait WasmNotSend`
- THEN the first match MUST appear exactly once, the second MUST
  return zero matches

---

### Requirement: Deferred audit finding I-15 — has_listeners / is_empty / len take the listener mutex (accept-permanent)

`ChangeNotifier::has_listeners(&self) -> bool`,
`ChangeNotifier::is_empty(&self) -> bool`,
`ChangeNotifier::len(&self) -> usize` MAY lock the internal
`Mutex<HashMap<...>>` to read the count. Implementations MUST NOT
introduce a parallel `AtomicUsize` counter that mirrors the HashMap
entry count.

This pins the cycle-3 deferral verdict (audit table: "Risk of drift
> benefit (the current `Mutex::lock` is uncontended in the steady-
state read path)") as **accept-permanent**.

Revival trigger (if this verdict needs revisiting): a benchmark in
`flui-foundation/benches/` shows ≥5% improvement on a representative
read-heavy workload (e.g. devtools probe loop calling `len()` at
60 Hz across 100+ notifiers).

**Audit ref:** I-15 (deferred → accept-permanent in this spec).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:225-238`
(`hasListeners` — Flutter reads `_count > 0` directly; single-threaded
so no lock).

**Rust-native divergence:**
- (a) Flutter: lock-free count read (Dart single-threaded).
- (b) FLUI: takes the listener mutex. Adding a parallel atomic would
  require sequencing it with every add/remove HashMap mutation, and
  the audit's risk assessment is that drift between the two states
  outweighs the steady-state read cost.
- (c) Future devtools or stats-collection workstreams may justify
  the atomic; revisit then.

#### Scenario: has_listeners returns true after add, false after remove_all

- GIVEN a `ChangeNotifier`
- WHEN `add_listener(Arc::new(|| {}))` is called, then
  `has_listeners()` is invoked
- THEN `has_listeners()` returns `true`; after
  `remove_all_listeners()` it returns `false`

#### Scenario: No parallel atomic listener counter exists

- GIVEN `crates/flui-foundation/src/notifier.rs`
- WHEN searched for `listener_count` or `count: Arc<AtomicUsize>` on
  the `ChangeNotifier` struct definition
- THEN zero matches MUST appear (proves the I-15 accept-permanent
  verdict is honoured)
