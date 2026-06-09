
# SDD Explore — `core-0a-foundation-adversarial-reaudit`

| Field | Value |
|---|---|
| Phase | sdd-explore (adversarial re-audit, NOT ratification) |
| Change ID | `core-0a-foundation-adversarial-reaudit` |
| Chain run | `1d15b70e` |
| Crates under audit | `crates/flui-foundation` (5,915 src LOC, 13 files) + `crates/flui-tree` (6,576 src LOC, 16 files) |
| Audit basis | Adversarial re-examination of cycle-3 (PR #102–#106 + Polish) closures + deferrals under advanced-SDD + Rust 1.95 + edition 2024 |
| skill_resolution | `paths-injected` (multi-agent, rust-ownership-system, clippy-configuration) |
| Cross-vendor broadcasts | **ALL BROADCASTS FAILED** — child harness lacks shell/intercom access (bg_task → "not found", contact_supervisor/intercom → "Broker failed to start within timeout"). Re-dispatch from a write-and-shell-capable harness required for cross-vendor peer-review on top-3 findings before sdd-design. |

## Audit method

For every cycle-3 finding (closed and deferred), I re-evaluated the claim against the actual current source. For each, I asked: (a) does the closure stand under adversarial scrutiny on D1-D10? (b) is the deferral premise still valid? (c) what did cycle-3 NOT touch on the same axes? Findings below are NEW relative to cycle 3 — they extend or contradict its conclusions, not restate them.

The 10 dimensions (per task brief):
- D1 — Soundness on `unsafe`
- D2 — Concurrency correctness
- D3 — Variance / lifetime
- D4 — Edition 2024 / Rust 1.95 idioms
- D5 — `.flutter/foundation/` parity
- D6 — `.gpui/` Rust-native pattern adoption
- D7 — `bon` builder candidacy
- D8 — `Diagnosticable` derive macro opportunity
- D9 — Inline-storage library choice
- D10 — Test scenario coverage gaps

## Findings inventory (30 findings)

### F1 — `Id<T>::from_raw` is declared `pub unsafe` but performs no unsafe operation

**Dimension:** D1 Soundness
**Severity:** P3
**Location:** `crates/flui-foundation/src/id.rs:213-217`
**Cycle-3 status:** I-9 deferred ("scheduler re-exports") — never re-examined the soundness premise
**Evidence:**
```rust
#[inline]
pub const unsafe fn from_raw(raw: RawId) -> Self {
    Self(raw, PhantomData)
}
```
The body wraps a *validated* `RawId` (which is itself a `NonZeroUsize` newtype enforcing non-zero by construction) with a `PhantomData<T>` tag. No `unsafe` operation is performed — no `transmute`, no `*_unchecked`, no pointer arithmetic. The SAFETY doc "caller must ensure raw ID is valid for this marker type" is **vacuous** because markers are uninhabited ZSTs that carry no values; any RawId is "valid" for any Marker by the type system's construction.
**Why cycle-3 missed it:** The single-model audit chained off Constitution Principle 3 (no unsafe in foundation except for narrow NonZeroUsize invariants) and flagged the surface area (`unsafe pub`), but didn't decompose whether each individual `unsafe fn` actually performed unsafe work. Modern Rust-1.81+ practice is `unsafe fn` only when the body literally requires an unsafe op.
**Proposed fix shape:** Remove `unsafe` from `from_raw`. The function becomes `pub const fn from_raw(raw: RawId) -> Self`. Serde deserialize path (line 608) simplifies (no `unsafe { ... }` wrapping). Public API surface gets safer without breaking any caller.
**Breaking-change risk:** No — removing `unsafe` from a public function never breaks callers (the callers' `unsafe { ... }` blocks become tagged `#[warn(unused_unsafe)]` which is benign).
**Cross-vendor verdict:** BROADCAST FAILED — child session lacks shell access; finding stands on single-model analysis.

---

### F2 — `Key::new()` overflow assert is off-by-one; post-wrap call is UB

**Dimension:** D1 Soundness (UB by construction)
**Severity:** **P0**
**Location:** `crates/flui-foundation/src/key.rs:138-153`
**Cycle-3 status:** I-7 deferred ("adds new public API; defer until real overflow-recovery callsite materializes") — the deferral rationale missed the off-by-one entirely
**Evidence:**
```rust
pub fn new() -> Self {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    assert!(
        id != u64::MAX,
        "Key counter overflow! ..."
    );
    Self(unsafe { NonZeroU64::new_unchecked(id) })
}
```
`fetch_add(1)` returns the **pre-increment** value. State machine:
- Call when COUNTER = `u64::MAX - 1` → returns `u64::MAX - 1`, COUNTER becomes `u64::MAX`. Returns Key(u64::MAX-1). OK.
- Call when COUNTER = `u64::MAX` → returns `u64::MAX`, COUNTER wraps to `0`. `assert!(id != u64::MAX)` PANICS. OK.
- **Call AFTER catching that panic (or in another thread)** → COUNTER = `0`, `fetch_add(1)` returns `0`, COUNTER becomes `1`. `id == 0`. `assert!(0 != u64::MAX)` PASSES. `NonZeroU64::new_unchecked(0)` is **UB** (Rustonomicon §3.2 invalid value).
The `assert!` is a recoverable panic — any test, `std::panic::catch_unwind`, or `tokio::task::JoinHandle` can swallow it and continue. The third call is then UB.
**Why cycle-3 missed it:** Cycle 3's I-7 framed the issue as "panic in release violates Constitution Principle 6 spirit" and proposed a `try_new() -> Result` constructor. The Result-shape proposal doesn't fix the off-by-one — `NonZeroU64::new(id)` would return `None` on `id == 0`, but `new_unchecked` is the actual UB site. Single-model analysis didn't trace the state machine across catch_unwind boundary.
**Proposed fix shape:** Either:
- (a) Change assert to `assert!(id != 0, "...")` (catches the post-wrap state, panics again — still recoverable but no UB).
- (b) Replace `new_unchecked` with `new(id).expect("Key counter exhausted")` — eliminates the unsafe entirely; `expect` panics on the same wrap case but no UB.
- (c) Use `compare_exchange` loop to refuse wrap-around (counter saturates at `u64::MAX`, subsequent calls return error/panic).
(b) is the Rust-1.81+ idiom and matches the Constitution Principle 3 "minimize unsafe" rule.
**Breaking-change risk:** No — fix is internal.
**Cross-vendor verdict:** BROADCAST FAILED — child session lacks shell access; finding stands on single-model analysis. Recommend Codex peer-review during sdd-design (Codex strong on Rust UB pathways).

---

### F3 — `UniqueKey::new()` has NO overflow check; silent collision after `2^64` calls

**Dimension:** D1 Soundness (contract violation, not UB)
**Severity:** P3
**Location:** `crates/flui-foundation/src/key.rs:476-481`
**Cycle-3 status:** Never audited (the I-7 finding only covered `Key::new`, not `UniqueKey::new`)
**Evidence:**
```rust
pub fn new() -> Self {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    Self { id }
}
```
No overflow check. After `2^64-1` calls, COUNTER wraps to 0; subsequent calls produce duplicate "unique" IDs. Not UB (`id: u64` has no niche invariant), but the trait contract "Each `UniqueKey` instance is different from all other keys" (line 458) is violated.
**Why cycle-3 missed it:** Cycle 3's I-7 verdict was framed around `Key::new`'s `NonZeroU64`-wrapping UB risk and didn't sweep the sibling `UniqueKey::new` shape. Practical-impossibility framing ("584 years at 1ns/call") was applied to Key and never re-asked for UniqueKey.
**Proposed fix shape:** Mirror Key's assert: `assert!(id != u64::MAX, "UniqueKey counter exhausted")` before the wrap point. Or use `Key::new()` internally as the unique-id source so both share one counter.
**Breaking-change risk:** No.
**Cross-vendor verdict:** —

---

### F4 — BindingBase steady-state pays unnecessary `Release` store per `instance()` call

**Dimension:** D2 Concurrency
**Severity:** P2
**Location:** `crates/flui-foundation/src/binding.rs:175-201`
**Cycle-3 status:** I-3 closed ("flip store AFTER `new()` returns"); cycle-3 comment at line 192-200 explicitly acknowledges the steady-state cost but did NOT apply the fix:
> "On steady-state callers the store is a single atomic write per call... If the per-call atomic store becomes measurable in a profile, replace with `INITIALIZED.compare_exchange(false, true, Release, Relaxed).ok();`"
**Evidence:** Every call to `MyBinding::instance()` (5 production bindings: SchedulerBinding, GestureBinding, RendererBinding, WidgetsBinding, SemanticsBinding) executes `Self::INITIALIZED.store(true, Release)` even after first-init. For per-frame binding lookups (WidgetsBinding consulted at every `setState`, SchedulerBinding consulted at every frame callback registration), this is N atomic Release-store writes propagating through cache coherency.
**Why cycle-3 missed it:** Cycle 3 prioritized correctness (re-init-after-panic hazard) over performance and explicitly punted the steady-state optimization. The audit doc lists it as "future profile-driven optimization." Advanced-SDD + Rust 1.95 should land both in one pass.
**Proposed fix shape:**
```rust
let _ = Self::INITIALIZED.compare_exchange(
    false, true,
    std::sync::atomic::Ordering::Release,
    std::sync::atomic::Ordering::Relaxed,
);
```
Steady-state path becomes a single atomic load (`Relaxed` on the CAS-fail branch) — cheaper than the current Release-store-always.
**Breaking-change risk:** No — pure perf, observable semantic identical.
**Cross-vendor verdict:** —

---

### F5 — Removing a listener during notify still fires it (parity drift from Flutter)

**Dimension:** D2 Concurrency + D5 Parity
**Severity:** P1
**Location:** `crates/flui-foundation/src/notifier.rs:248-258` (notify_listeners) vs `.flutter/.../change_notifier.dart:443-490`
**Cycle-3 status:** Never audited. Cycle-3 Drift A acknowledged HashMap-vs-array storage but did not enumerate observable behavior divergences. The `dispose_during_notify_iteration_safe` test exists but does not exercise this distinct semantic.
**Evidence:** Flutter (`change_notifier.dart:480-498`):
```dart
void removeListener(VoidCallback listener) {
  for (var i = 0; i < _count; i++) {
    if (_listeners[i] == listener) {
      if (_notificationCallStackDepth > 0) {
        _listeners[i] = null;     // ← null out so the in-flight loop skips
        _reentrantlyRemovedListeners++;
      } else {
        _removeAt(i);
      }
      break;
    }
  }
}
// notifyListeners loop tests `_listeners[i]?.call();` — null-check skips removed.
```
FLUI (`notifier.rs:251-257`):
```rust
let callbacks: smallvec::SmallVec<[ListenerCallback; 4]> =
    self.listeners.lock().values().cloned().collect();
for callback in &callbacks {
    callback();   // ← removed listener's Arc is still in the snapshot, fires regardless
}
```
A listener registered as `id` that, during one of the earlier callbacks in the snapshot, calls `notifier.remove_listener(id)` is correctly removed from the underlying HashMap, but the Arc was already cloned into the snapshot — it STILL fires later in this loop. Flutter's behavior: it does NOT fire. Observable behavior divergence.
**Why cycle-3 missed it:** Cycle-3 framed the snapshot-then-fire pattern purely as a reentrancy-safety mechanism (drop-during-notify avoidance) and did not cross-check the per-iteration removal semantic against Flutter. The test suite asserts only the dispose-mid-notify case.
**Proposed fix shape:** Two options:
- (a) Change snapshot to `SmallVec<[(ListenerId, ListenerCallback); 4]>`, re-check `self.listeners.lock().contains_key(&id)` before each callback. Adds 1 lock + lookup per callback.
- (b) Accept the divergence and document it in `notify_listeners` doc-comment as a Rust-native Choice ("removed-during-notify still fires") — but then add a parity-drift entry to the audit catalog.
For Flutter-loyal behavior (Constitution Principle 1), (a) is correct.
**Breaking-change risk:** Yes — flui-animation crate (cycle-3 audit notes 8 files consume ChangeNotifier) may have callers that rely on the current (Rust-native) "removed listener still fires" semantic for cleanup callbacks. Audit needed.
**Cross-vendor verdict:** —

---

### F6 — Panic in one listener aborts the rest of `notify_listeners`; Flutter wraps each call in try/catch

**Dimension:** D1 Soundness + D2 Concurrency + D5 Parity
**Severity:** **P0**
**Location:** `crates/flui-foundation/src/notifier.rs:255-257` vs `.flutter/.../change_notifier.dart:443-470`
**Cycle-3 status:** Never audited. I-4 (cycle 3) addressed per-frame Vec→SmallVec alloc but the listener-panic propagation was untouched.
**Evidence:** Flutter:
```dart
for (var i = 0; i < end; i++) {
  try {
    _listeners[i]?.call();
  } catch (exception, stack) {
    FlutterError.reportError(FlutterErrorDetails(
      exception: exception, stack: stack, ...
    ));
  }
}
```
FLUI:
```rust
for callback in &callbacks {
    callback();   // ← unhandled panic unwinds, skips remaining callbacks
}
```
Consequences:
1. **Frame-state inconsistency**: a buggy listener at position 2 panics → listener 3 (the renderer's repaint subscriber) NEVER fires → next frame has stale state.
2. **Cascading caller damage**: the panic unwinds through `notify_listeners`, through `ValueNotifier::set_value`, through `MyView::on_input_changed`, etc. Each unwound frame may leave its data in an inconsistent state.
3. **Constitution Principle 6 violation in spirit**: "no panics in production" — but a buggy listener can take down the whole UI.
**Why cycle-3 missed it:** I-4's framing was "allocator pressure," not "listener loop hazards." The dispose-during-notify test indirectly exercises the snapshot pattern but never panics inside a listener. Mythos audit method did not check the listener-loop error-handling axis.
**Proposed fix shape:**
```rust
for callback in &callbacks {
    let result = std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| callback())
    );
    if let Err(payload) = result {
        tracing::error!(?payload, "ChangeNotifier listener panicked; continuing");
    }
}
```
`AssertUnwindSafe` is justified because the closure borrows `&ListenerCallback`; the Arc semantics guarantee the callback is `Send + Sync` (and therefore `RefUnwindSafe`). Mirrors Flutter's `try/catch + FlutterError.reportError` exactly.
**Breaking-change risk:** Low. Callers that previously relied on the propagating panic (e.g. test scaffolds that catch a panic to confirm "the notifier observed a buggy callback") would need to switch to a tracing-subscriber-based check. Acceptable.
**Cross-vendor verdict:** BROADCAST FAILED — child session lacks shell access; finding stands on single-model analysis. **Strong recommendation: re-broadcast to Codex (Rust unwinding semantics) AND Gemini (large-context cross-check against `.flutter/foundation/` + `.gpui/EventEmitter`) before sdd-design.**

---

### F7 — `Id<T>` uses `PhantomData<T>` (covariant); should use `PhantomData<fn() -> T>` for invariance defense-in-depth

**Dimension:** D3 Variance
**Severity:** P3
**Location:** `crates/flui-foundation/src/id.rs:204`
**Cycle-3 status:** Never audited (variance dimension absent from cycle-3 method)
**Evidence:** `pub struct Id<T: Marker>(RawId, PhantomData<T>);` — `PhantomData<T>` is covariant in T. With current markers (uninhabited `'static` enums), no immediate hazard. But if a future Marker becomes parameterized (`Marker<'a>`), covariance allows `Id<&'long_marker>` → `Id<&'short_marker>` coercion at the unsafe boundary.
Per Rustonomicon §3.10 ("Variance"): for tag-only PhantomData where T's role is purely type discrimination, **invariance** via `PhantomData<fn() -> T>` (or `PhantomData<*const T>`) is the conservative choice. The wgpu-inspired ID system FLUI cites at `id.rs:8` uses invariant phantoms.
**Why cycle-3 missed it:** Variance is one of the most subtle Rust dimensions; single-model audit on a complex Mythos pass tends to skip it unless flagged. Advanced-SDD with explicit variance dimension catches it.
**Proposed fix shape:** `pub struct Id<T: Marker>(RawId, PhantomData<fn() -> T>);`. Compile-time-equivalent for all current consumers; future-proofs against parameterized markers.
**Breaking-change risk:** None for current callers (markers are all `'static`).
**Cross-vendor verdict:** —

---

### F8 — `TreeReadExt`/`TreeNavExt` HRTB predicate bounds (`for<'a> FnMut(&'a Self::Node) -> bool`) are over-engineered

**Dimension:** D3 Lifetime + D4 Idiom
**Severity:** P3
**Location:** `crates/flui-tree/src/traits/read.rs:198-247`, `traits/nav.rs:295-340`
**Cycle-3 status:** Cycle 3 T-15 was deferred ("kept as the public extension surface — real-world ergonomic value"). The audit accepted the HRTB shape without questioning it.
**Evidence:**
```rust
fn find_node_where<P>(&self, mut predicate: P) -> Option<I>
where P: for<'a> FnMut(&'a Self::Node) -> bool { ... }
```
In Rust 1.75+, the lifetime elision rules make `FnMut(&Self::Node) -> bool` semantically equivalent in 99% of cases — the predicate is invoked with a borrow whose lifetime is at most `&self`'s. The explicit HRTB form only matters if the predicate must store the borrow across multiple calls with disjoint lifetimes (which `find_node_where` never does).
**Why cycle-3 missed it:** The HRTB form was speculation from the cycle-3 closed-iteration set; the cycle-3 verdict was "ergonomic value" without questioning whether the HRTB was necessary.
**Proposed fix shape:** Drop the `for<'a>` quantifier:
```rust
fn find_node_where<P: FnMut(&Self::Node) -> bool>(&self, mut predicate: P) -> Option<I> { ... }
```
Simpler trait bound, identical inference behavior.
**Breaking-change risk:** None (relaxation of the bound, callers compile unchanged).
**Cross-vendor verdict:** —

---

### F9 — `#[allow(unsafe_code)]` should be `#[expect(unsafe_code)]` (Rust 1.81+ idiom)

**Dimension:** D4 Edition 2024 / Rust 1.95 idiom
**Severity:** P2
**Location:** `crates/flui-foundation/src/id.rs:30`, `crates/flui-foundation/src/key.rs:3`
**Cycle-3 status:** Never swept. `#[expect]` is a Rust 1.81 stable idiom; cycle 3 (Mythos audit, pre-1.81-aware) didn't enforce it.
**Evidence:** Both files declare `#![allow(unsafe_code)]` at the module level. The `unsafe` IS used (NonZeroUsize::new_unchecked) — so `#[expect(unsafe_code, reason = "...")]` is the correct idiom. If the `unsafe` is ever removed (e.g. F2 fix replaces `new_unchecked` with `new().expect()`), `#[expect]` surfaces a `unfulfilled_lint_expectation` warning, forcing the attribute to be removed in lockstep. `#[allow]` would silently rot.
**Why cycle-3 missed it:** Toolchain idiom predates `#[expect]` stabilization. Advanced-SDD should mandate `#[expect]` for any "must trigger" lint allowance.
**Proposed fix shape:** Sweep all `#[allow(<lint>)]` in foundation+tree → `#[expect(<lint>, reason = "<one-liner>")]` where the lint MUST trigger.
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F10 — Missing `Listenable.merge` / `MergedListenable` equivalent (parity gap)

**Dimension:** D5 Parity
**Severity:** P2
**Location:** `crates/flui-foundation/src/notifier.rs` (entire file); compare `.flutter/.../change_notifier.dart:506-525`
**Cycle-3 status:** Never audited. Cycle-3 catalog enumerated Listenable/ValueListenable/ChangeNotifier but missed the merge factory.
**Evidence:** Flutter:
```dart
abstract class Listenable {
  factory Listenable.merge(Iterable<Listenable?> listenables) = _MergingListenable;
  ...
}
class _MergingListenable extends Listenable {
  _MergingListenable(this._children);
  final Iterable<Listenable?> _children;
  @override void addListener(VoidCallback listener) {
    for (final Listenable? child in _children) child?.addListener(listener);
  }
  @override void removeListener(VoidCallback listener) {
    for (final Listenable? child in _children) child?.removeListener(listener);
  }
}
```
FLUI has no equivalent. Consumers needing "fire on ANY of N listenables changing" must roll their own. flui-animation's `CompoundAnimation` (cycle 2 + cycle 3 references) would benefit; future `AnimationController.merge(...)` ports also.
**Why cycle-3 missed it:** Cycle-3 Drift A noted HashMap-vs-array storage but didn't enumerate Flutter's full Listenable surface. The Mythos parity dimension was column-by-column on existing FLUI symbols, not Flutter-leading.
**Proposed fix shape:**
```rust
pub struct MergedListenable {
    children: Vec<Arc<dyn Listenable>>,
}
impl MergedListenable {
    pub fn new(children: Vec<Arc<dyn Listenable>>) -> Self { Self { children } }
}
impl Listenable for MergedListenable {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        // Returns the FIRST child's listener id (or a wrapping new id type)
        // — design needed for ID semantics across children
        ...
    }
    fn remove_listener(&self, id: ListenerId) { ... }
    fn remove_all_listeners(&self) { ... }
}
```
The ID semantic is the design call (return composite ID, or use the first child's). Defer to sdd-design.
**Breaking-change risk:** Additive — no existing code breaks.
**Cross-vendor verdict:** —

---

### F11 — `impl Default for ValueNotifier<T>` is surprising-default; cycle-3 swept Key/UniqueKey but missed this

**Dimension:** D5 Parity (surprise) + cycle-3 closure incompleteness
**Severity:** P1
**Location:** `crates/flui-foundation/src/notifier.rs:430-435`
**Cycle-3 status:** Cycle-3 closed I-5 (`Default for Key`, `Default for UniqueKey` removed) on the rationale "every call returns a different value — violates least-surprise for Default." The same rationale applies to ValueNotifier and was missed in the sweep.
**Evidence:**
```rust
impl<T: Clone + Default> Default for ValueNotifier<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
```
Every call to `ValueNotifier::<T>::default()` creates a NEW `ChangeNotifier` field (fresh empty listener HashMap, fresh `is_disposed: false`). Two `Default`-derived structs containing `ValueNotifier<i32>` will have non-equal notifier identities — and `PartialEq for ValueNotifier<T>` (line 437) compares ONLY `self.value`, so the equality is misleading.

```rust
impl<T: Clone + PartialEq> PartialEq for ValueNotifier<T> {
    fn eq(&self, other: &Self) -> bool { self.value == other.value }
}
```
Two `Default`-constructed ValueNotifier<i32> ARE `==` per this impl, but their notifiers are independent → a listener added to one will not fire on the other's notify. Surprising hidden state divergence under `==` equivalence.
**Why cycle-3 missed it:** I-5 closure swept only Key/UniqueKey by name. The audit did not enumerate all `impl Default for ...` sites in foundation+tree.
**Proposed fix shape:** Remove `impl Default for ValueNotifier<T>`. Callers must explicitly call `ValueNotifier::new(value)`. Or keep but switch `PartialEq` to compare both value AND notifier-identity (Arc::ptr_eq on the inner notifier Arc) — but that's a stricter equality that may break existing usage.
**Breaking-change risk:** Yes — flui-animation (cycle 3 reference: 8 files use ValueNotifier as canonical Listenable). Audit needed.
**Cross-vendor verdict:** —

---

### F12 — Diagnosticable port is shallow: missing typed property subclasses

**Dimension:** D5 Parity
**Severity:** P2
**Location:** `crates/flui-foundation/src/debug.rs` (entire `DiagnosticsProperty` family); compare `.flutter/.../diagnostics.dart`
**Cycle-3 status:** Cycle-3 closed I-19 (`Box<str>` over `String` for error payloads) but never audited the typed-subclass gap.
**Evidence:** Flutter `diagnostics.dart` ships:
- `EnumProperty<T>` — renders enum name without module path
- `FlagProperty` — renders boolean as `name` (true) or `-` (false)
- `IterableProperty<T>` — renders iterable with count
- `ObjectFlagProperty<T>` — renders nullable as `present` / `<none>`
- `MessageProperty` — single-value diagnostic without name
- `StringProperty` — quoted-string-aware rendering
- `IntProperty` / `DoubleProperty` — typed numeric with unit suffix
- `PercentProperty` — 0.0..1.0 → "12.3%"
- `DiagnosticsStackTrace` — pretty-printed stack
- `DiagnosticsBlock` — collapsible group

FLUI has ONE generic `DiagnosticsProperty { name: String, value: String, ... }` where `value` is `to_string()` of an `impl Display`. All discrimination is lost — `EnumProperty<MyEnum>(MyEnum::Variant)` renders identically to `StringProperty("MyEnum::Variant")`.
**Why cycle-3 missed it:** Audit focused on deletion (I-1 ObserverList, I-2 FoundationError) and surface-area fixes. The shallow-port issue is additive and was outside the deletion-focused scope.
**Proposed fix shape:** Add a `DiagnosticsProperty` enum variant set:
```rust
pub enum DiagnosticsPropertyKind {
    Generic { value: String },
    Enum { type_name: &'static str, variant: &'static str },
    Flag { active: bool, if_true: &'static str },
    Iterable { count: usize, summary: String },
    OptionalRef { present: bool },
    Stack(Vec<String>),
}
```
And matching constructors. Each renders differently in `format_with_style`. Net ~300 LOC.
**Breaking-change risk:** Additive if old `DiagnosticsProperty::new` becomes a forwarder to the `Generic` variant.
**Cross-vendor verdict:** —

---

### F13 — Cycle 3's `ObserverList` deletion lost de-dup semantic; no migration documented for future hit-test consumers

**Dimension:** D5 Parity + cycle-3 closure side-effect
**Severity:** P2
**Location:** Cycle-3 deletion of `crates/flui-foundation/src/observer.rs` (PR #102)
**Cycle-3 status:** I-1 closed by deletion (271 LOC removed, 0 in-workspace consumers cited)
**Evidence:** Flutter `observer_list.dart` ships:
- `ObserverList<T>` — `add`/`remove` O(1), iteration order-preserving, with deduplication for hit-test entries (used by `RenderObject.handleEvent`).
- `HashedObserverList<T>` — same API, O(1) lookup via `HashMap<T, int>` for high-churn scenarios.

The de-dup semantic is the key load-bearing piece: a hit-test path that registers the same observer twice should not fire it twice. Cycle-3 deletion did not migrate the semantic to flui-interaction's hit-tester. When a future flui-interaction enhancement needs this property, it will be re-derived ad-hoc.
**Why cycle-3 missed it:** Deletion was justified by "0 consumers." Flutter's ObserverList is consumed internally by hit-testing in render/binding.dart; FLUI's flui-interaction hit-test path may already roll its own (cycle 1 audit). The architectural-pattern documentation was not preserved.
**Proposed fix shape:** Either:
- (a) Re-introduce ObserverList behind `#[cfg(feature = "observer-list")]` defaulted off (cycle 3 rejected feature-gating on the "still maintenance burden" axis — re-evaluate).
- (b) Document the de-dup semantic in flui-interaction's hit-test ARCHITECTURE.md as a "if you reinvent this, here's the requirement" note.
- (c) Wait for the first flui-interaction consumer to ask, then port from git history.
(b) is the lowest-friction option — preserves architectural memory without re-introducing code.
**Breaking-change risk:** None for (b).
**Cross-vendor verdict:** —

---

### F14 — Multiple ≥3-arg `pub fn new(...)` constructors warrant `bon` builder sweep

**Dimension:** D7 bon adoption
**Severity:** P2
**Location:** Multiple — see list below
**Cycle-3 status:** T-17 deferred (`Slot::with_siblings` specifically). No systematic sweep.
**Evidence (grep across foundation+tree):**
- `Slot::new(parent: I, index: usize, depth: Depth)` — `iter/slot.rs:91`. 3 positional.
- `Slot::with_siblings(parent, index, depth, previous_sibling, next_sibling)` — `iter/slot.rs:107`. 5 positional, two `Option<I>` indistinguishable by position. (Cycle-3 T-17 deferred.)
- `TreeError::invalid_parent(child: usize, parent: usize)` — `error.rs:134`. 2 positional `usize`, easy to swap.
- `TreeError::max_depth_exceeded(element: usize, max: usize)` — `error.rs:142`. Same shape.
- `TreeError::not_supported(id: usize, reason: &'static str)` — `error.rs:154`. 2 positional, mixed types — OK as-is.
- `ArityError::too_many(arity: &'static str, attempted: usize)` — `arity/error.rs:54`. 2 positional, mixed types — OK.
- `IndexedSlot::new(index: usize, previous: Option<I>)` — `iter/slot.rs:368`. 2 positional, low risk.
- `DiagnosticsProperty::new(name, value)` — already has fluent `.with_*` chain. OK.
- `DiagnosticsNode::new(name)` — same. OK.

Constitution Part IV names `bon` as the canonical builder dep. Cycle 3 deferred T-17 in isolation; the sweep was not done.
**Why cycle-3 missed it:** Single-finding focus on T-17. Edition 2024 + bon ecosystem maturity (since cycle-3 timeframe `bon` 2.x landed) makes builder adoption cheap.
**Proposed fix shape:** Add `#[bon::builder]` to `Slot::with_siblings` (T-17) and `Slot::new` (F14). TreeError constructors stay as-is (the small-positional-arg cases don't justify a builder; the risk-of-swap is low because the types differ).
**Breaking-change risk:** Additive (positional constructors retained).
**Cross-vendor verdict:** —

---

### F15 — `#[derive(Diagnosticable)]` macro would close ~150 LOC across flui-rendering

**Dimension:** D8 Macro opportunity
**Severity:** P1
**Location:** `crates/flui-foundation/src/debug.rs::Diagnosticable` trait + `crates/flui-macros/src/lib.rs` (new derive)
**Cycle-3 status:** Never audited. flui-macros currently provides only `#[derive(Stateless)]` + `#[derive(Stateful)]` (per AGENTS.md). Diagnosticable is hand-rolled everywhere.
**Evidence:** flui-rendering ships 10+ Diagnosticable impls (cited in cycle-3 Appendix A.3):
```rust
impl Diagnosticable for RenderFlex {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("direction", &self.direction);
        builder.add("main_axis_alignment", &self.main_axis_alignment);
        builder.add("cross_axis_alignment", &self.cross_axis_alignment);
        ...
    }
}
```
A `#[derive(Diagnosticable)]` macro reading named fields + their `Display` impls auto-generates this body:
```rust
#[derive(Diagnosticable)]
pub struct RenderFlex {
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    ...
}
```
Cost: ~200-400 LOC proc-macro in flui-macros. Saves ~10-15 LOC per Diagnosticable impl downstream × 10+ existing + 50+ future render objects.
**Why cycle-3 missed it:** Audit focus on existing surface, not on macro-driven reductions. Advanced-SDD with `flui-macros` already proven would surface this.
**Proposed fix shape:** New `flui_macros::Diagnosticable` derive with field-by-field `builder.add(stringify!(field), &self.field)`. Optional `#[diagnostic(skip)]` field attribute for internal-only fields.
**Breaking-change risk:** None — derive is opt-in.
**Cross-vendor verdict:** —

---

### F16 — SmallVec retention for ChangeNotifier is justified; tinyvec doesn't fit (info-only)

**Dimension:** D9 Inline-storage choice
**Severity:** P3 (informational)
**Location:** `crates/flui-foundation/src/notifier.rs:251`
**Cycle-3 status:** I-4 closed with `SmallVec<[ListenerCallback; 4]>` (heap fallback).
**Evidence:** Alternative library analysis:
- `tinyvec::TinyVec<[T; N]>` — requires `T: Default`. `ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>` does NOT impl Default. Fails to compile.
- `tinyvec::ArrayVec<[T; N]>` — same requirement.
- `arrayvec::ArrayVec<[T; N]>` — fixed-cap, no heap fallback. Would silently drop the 5th listener (silent data loss) — worse than SmallVec.
- `Vec::with_capacity(4)` — always heap-allocates. Defeats the I-4 purpose.
- `SmallVec<[T; 4]>` — heap fallback, no `Default` requirement, uses internal `unsafe` for inline storage (ManuallyDrop, MaybeUninit).

**Conclusion:** SmallVec is the correct choice for this use case. The internal `unsafe` is in a well-audited library (smallvec 1.x, ~12M crates.io downloads). Cycle-3's call stands.
**Why this is a finding anyway:** The trade-off rationale was never documented in the source comment at line 251. Future maintainers may second-guess the choice. Add a `// NB: tinyvec rejected — ListenerCallback: !Default` comment.
**Proposed fix shape:** Documentation lift only (3-line comment).
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F17 — Missing test: BindingBase `instance()` called twice when first `new()` panics and second succeeds

**Dimension:** D10 Test gap
**Severity:** P1
**Location:** `crates/flui-foundation/src/binding.rs::tests`
**Cycle-3 status:** I-3 closed with `init_panic_does_not_flip_initialized_flag` test (PanicBinding always panics). The retry-after-panic case is untested.
**Evidence:** Per `OnceLock::get_or_init` Rust stdlib docs: "If this function panics, the cell is unchanged. Future calls may retry init." A binding whose `new()` panics on first call (transient resource failure, e.g. wgpu init race) but succeeds on second call SHOULD initialize cleanly. Cycle-3's PanicBinding tests only the always-panic case.
**Proposed test:**
```rust
struct RetryingBinding { counter: AtomicUsize }
impl RetryingBinding {
    fn new() -> Self {
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("transient init failure");
        }
        Self { counter: AtomicUsize::new(0) }
    }
}
impl_binding_singleton!(RetryingBinding);

#[test]
fn instance_retries_after_panic() {
    assert!(!RetryingBinding::is_initialized());
    let r1 = std::panic::catch_unwind(|| RetryingBinding::instance());
    assert!(r1.is_err());
    assert!(!RetryingBinding::is_initialized());

    // Second call must succeed and flip flag.
    let inst = RetryingBinding::instance();
    assert!(RetryingBinding::is_initialized());
    assert!(std::ptr::eq(inst, RetryingBinding::instance()));
}
```
**Why cycle-3 missed it:** The regression test was scoped to "don't flip flag on panic" — the symmetric "do flip flag after recovery" was outside the regression scope.
**Proposed fix shape:** Add the test above to `binding.rs::tests`.
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F18 — No test for `Id` production at `usize::MAX - 1` boundary

**Dimension:** D10 Test gap
**Severity:** P3
**Location:** `crates/flui-foundation/src/id.rs::tests`
**Cycle-3 status:** Never audited.
**Evidence:** `Id::zip(usize::MAX)` calls `NonZeroUsize::new(usize::MAX) = Some(...)` — no panic. A slab grown to `usize::MAX` entries (impractical but type-system-valid) produces the maximum-valued Id. The next insertion would `slab.insert()` returning index `usize::MAX` and `Id::zip(usize::MAX + 1)` would WRAP to 0 → panics.
**Why cycle-3 missed it:** Practical-impossibility framing.
**Proposed fix shape:** Add boundary test:
```rust
#[test]
fn id_at_usize_max() {
    let id = ViewId::zip(usize::MAX);
    assert_eq!(id.unzip(), usize::MAX);
}

#[test]
#[should_panic(expected = "non-zero")]
fn id_overflow_wrap_panics() {
    let _ = ViewId::zip(usize::MAX.wrapping_add(1)); // wraps to 0, panics
}
```
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F19 — `TreeWrite::remove` default cascade has no cycle-detection guard; corrupted tree OOMs

**Dimension:** D1 Soundness + D10 Test gap
**Severity:** **P0**
**Location:** `crates/flui-tree/src/traits/write.rs:104-150` (the iterative cascade from PR #103)
**Cycle-3 status:** PR #103 hardened against deep recursion (worklist+drain pattern). T-12 added cycle-bound to `Ancestors::next`. But the cascade walk (symmetric to ancestors) was NOT given the same guard.
**Evidence:**
```rust
let mut worklist: Vec<I> = Vec::new();
let mut to_visit: Vec<I> = Vec::with_capacity(8);
to_visit.push(id);
while let Some(current) = to_visit.pop() {
    worklist.push(current);
    for child_id in self.children(current) {
        to_visit.push(child_id);  // ← no cycle check
    }
}
```
Failure mode: a corrupted tree (`a.children = [b], b.children = [a]`) produces a `to_visit` that grows unboundedly: `[root], [a], [b], [a], [b], ...` → OOM.

Construction via the public API is currently impossible (cycle 2 PR #100/#101 added cycle-rejection to `add_child`; cycle 3 T-1 lifts cascade to trait). But:
1. `unsafe` paths in storage (e.g. flui-rendering's slab manipulation) can introduce a cycle.
2. De-serialization of an externally-corrupted tree can introduce a cycle.
3. A future TreeWriteNav impl that fails to enforce the cycle-rejection at `set_parent` can introduce a cycle.

The iterator-side defenses (T-12 step counter on Ancestors) exist precisely as defense-in-depth against these. The cascade-side defense is missing.
**Why cycle-3 missed it:** The cycle-3 PR #103 review focus was "stack-safety on deep but valid trees" (2k-deep chain test). The corrupted-tree (cyclic) test was never written; the audit's T-12 motivation didn't propagate to the cascade walk.
**Proposed fix shape:** Cap the worklist at `tree.len().saturating_mul(2)` or use a `HashSet<I>` visited-set for cycle detection. SmallVec-backed visited-set keeps the typical case allocation-light:
```rust
let mut worklist: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
let mut visited: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
let max_steps = self.len().saturating_mul(2).max(1);
let mut to_visit: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
to_visit.push(id);
let mut steps = 0;
while let Some(current) = to_visit.pop() {
    steps += 1;
    if steps > max_steps {
        tracing::warn!(?id, "TreeWrite::remove cascade aborted — cycle suspected");
        return None;
    }
    if visited.contains(&current) { continue; }
    visited.push(current);
    worklist.push(current);
    for child_id in self.children(current) {
        to_visit.push(child_id);
    }
}
```
Also addresses F24 (Vec → SmallVec).
**Breaking-change risk:** None (defense-in-depth; behavior identical for valid trees).
**Cross-vendor verdict:** BROADCAST FAILED — child session lacks shell access; finding stands on single-model analysis. **Strong recommendation: re-broadcast to Codex during sdd-design (cycle-detection + worklist-bounding is a Codex specialty).**

---

### F20 — `check_disposed` in release mode unreachably calls `tracing::warn!` after `debug_assert!(false, ...)`

**Dimension:** D2 + D4 Idiom
**Severity:** P3
**Location:** `crates/flui-foundation/src/notifier.rs:212-224`
**Cycle-3 status:** Never audited.
**Evidence:**
```rust
fn check_disposed(&self) -> bool {
    if self.is_disposed.load(Ordering::Acquire) {
        debug_assert!(
            false,
            "ChangeNotifier used after dispose: ..."
        );
        tracing::warn!("ChangeNotifier used after dispose");
        return true;
    }
    false
}
```
In debug builds, `debug_assert!(false, ...)` always panics → `tracing::warn!` is unreachable. In release, `debug_assert!` is removed → `tracing::warn!` fires + returns true. The logic is correct but the layout is misleading; a reader expects both to fire.
**Proposed fix shape:**
```rust
fn check_disposed(&self) -> bool {
    if self.is_disposed.load(Ordering::Acquire) {
        #[cfg(debug_assertions)]
        panic!("ChangeNotifier used after dispose: ...");
        #[cfg(not(debug_assertions))]
        tracing::warn!("ChangeNotifier used after dispose");
        return true;
    }
    false
}
```
Now the dead-code is explicit.
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F21 — `flui-tree` enables `clippy::pedantic` with blanket `#![allow]` overrides

**Dimension:** D4 Idiom
**Severity:** P2
**Location:** `crates/flui-tree/src/lib.rs:31-32`
**Cycle-3 status:** Never audited.
**Evidence:**
```rust
#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions, clippy::too_many_lines)]
```
The blanket `#![allow]` masks every occurrence of these lints across the crate. Per Rust-1.81+ idiom, the correct shape is per-call-site `#[expect(clippy::too_many_lines, reason = "...")]` with explicit rationale.
**Why cycle-3 missed it:** clippy-configuration skill (loaded in this session) flags this pattern. Cycle-3 didn't load it.
**Proposed fix shape:** Sweep `flui-tree/src/` for functions that legitimately need `clippy::too_many_lines` allowance, attach `#[expect(...)]` per-fn. Remove the blanket `#![allow]`.
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F22 — clippy `pedantic` asymmetry between `flui-foundation` and `flui-tree`

**Dimension:** D4 Idiom
**Severity:** P3
**Location:** `crates/flui-foundation/src/lib.rs:118` vs `crates/flui-tree/src/lib.rs:32`
**Cycle-3 status:** Never audited (cross-crate idiom consistency).
**Evidence:**
- `flui-foundation::lib.rs:118 #![warn(missing_docs, missing_debug_implementations, rust_2018_idioms, unreachable_pub)]` — no `clippy::pedantic`.
- `flui-tree::lib.rs:32 #![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]` — `clippy::pedantic` enabled.
A sibling pair of foundation-layer crates should have consistent clippy policy. Inconsistency makes it harder to surface drift.
**Proposed fix shape:** Pick one policy and apply across foundation+tree. Recommendation: enable `clippy::pedantic` on both (with per-call-site `#[expect]` for legitimate violations) — matches advanced-SDD discipline.
**Breaking-change risk:** None (lint-level changes).
**Cross-vendor verdict:** —

---

### F23 — I-10 deferral rationale ("scheduler re-exports") is invalid; imports aren't used

**Dimension:** D4 + cycle-3 closure incompleteness
**Severity:** P3
**Location:** `crates/flui-foundation/src/id.rs:71` (Index pub alias), `:81` (RawId pub struct); `crates/flui-scheduler/src/id.rs:36` (alleged consumer)
**Cycle-3 status:** I-10 deferred citing scheduler imports.
**Evidence:** Grep confirms `flui-scheduler/src/id.rs:36 use flui_foundation::{Id, Identifier, Index, Marker, RawId, ...}` — the imports exist. But the impl body uses only `Id<M>` and `Marker`; `Index` and `RawId` are imported-but-unused (a `cargo clippy --warnings unused_imports` would flag them).
**Why cycle-3 missed it:** Audit verified the imports textually but didn't run usage analysis. Advanced-SDD with `cargo check --warnings unused-imports` mandatory would catch this.
**Proposed fix shape:** (1) Remove unused `Index, RawId` from scheduler import. (2) Then close I-10: `pub(crate) struct RawId` + `pub(crate) type Index = usize`.
**Breaking-change risk:** None (scheduler imports are crate-internal; foundation visibility downgrade affects no external consumer per existing cycle-3 grep evidence).
**Cross-vendor verdict:** —

---

### F24 — `TreeWrite::remove` default uses `Vec<I>` for worklist instead of `SmallVec<[I; INLINE_TREE_DEPTH]>`

**Dimension:** D2 Hot path
**Severity:** P1
**Location:** `crates/flui-tree/src/traits/write.rs:117-118`
**Cycle-3 status:** PR #103 added the iterative cascade using bare `Vec<I>`. Cycle-3 T-10 unified `INLINE_TREE_DEPTH = 32` as the SmallVec sizing canon; T-18, T-20 applied it to LCA + Siblings. Cycle-3 missed applying it here.
**Evidence:**
```rust
let mut worklist: Vec<I> = Vec::new();
let mut to_visit: Vec<I> = Vec::with_capacity(8);
to_visit.push(id);
while let Some(current) = to_visit.pop() { ... }
```
Per-cascade: 2 `Vec` allocations + N pushes. For typical subtrees (≤32 nodes), `SmallVec<[I; INLINE_TREE_DEPTH]>` keeps everything on the stack — zero allocator pressure on the cascade hot path.
**Why cycle-3 missed it:** T-10 was applied to specific sites in nav.rs (LCA, ancestors). The newly-introduced cascade in write.rs was added AFTER the T-10 sweep and never retroactively swept.
**Proposed fix shape:** Replace both `Vec<I>` with `SmallVec<[I; INLINE_TREE_DEPTH]>`. Combine with F19 cycle-detection fix.
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F25 — `TreeWrite::remove` cascade order is right-to-left for siblings vs Flutter's left-to-right dispose

**Dimension:** D5 Parity
**Severity:** P2
**Location:** `crates/flui-tree/src/traits/write.rs:117-145`
**Cycle-3 status:** Never audited (parity-of-order dimension absent from cycle-3 method).
**Evidence:** Default impl pushes children to `to_visit` (LIFO stack) in iteration order:
```rust
for child_id in self.children(current) {
    to_visit.push(child_id);   // pushes child0, child1, child2
}
// pop returns child2, then child1, then child0 — REVERSE order in worklist
```
Then `worklist.into_iter().rev()` for dispose → original child order reversed. Net: leaves of the LAST child dispose FIRST.

Flutter `RenderObject.dispose` / `Layer.dispose` cascade in child-order (left-to-right). Engine listeners that depend on disposal order (e.g. a layer-removal hook that expects siblings to dispose in registration order) observe different sequencing.
**Why cycle-3 missed it:** The cycle-3 comment at write.rs:144 explicitly says "child order in `worklist` doesn't matter — what matters is that every descendant appears *after* its parent." This is true for cascade correctness BUT not for observable disposal order.
**Proposed fix shape:** Reverse the child push to preserve left-to-right disposal:
```rust
let children: SmallVec<[I; 8]> = self.children(current).collect();
for child_id in children.into_iter().rev() {
    to_visit.push(child_id);   // pushes child2, child1, child0
}
// pop returns child0, then child1, then child2 — original order in worklist
// reverse for post-order: child2.leaves first, then child1.leaves, then child0.leaves — wrong
```
Actually achieving Flutter-parity (left-to-right post-order disposal) requires a more careful walk. Best shape: recursive helper or explicit two-stack post-order traversal. Defer the implementation strategy to sdd-design.
**Breaking-change risk:** Yes for engine consumers that observed the current order; minimal currently (cycle-2-onwards engine code).
**Cross-vendor verdict:** —

---

### F26 — Foundation doc-comment examples use `println!` inside listener callbacks (Constitution Principle 6 violation in docs)

**Dimension:** D4 + D5 Doc parity
**Severity:** P3
**Location:** `crates/flui-foundation/src/lib.rs:38, 81-99`; `crates/flui-foundation/src/notifier.rs:25-30, 72-83, 95-106`
**Cycle-3 status:** Never audited.
**Evidence:** Examples like:
```rust
let listener_id = notifier.add_listener(Arc::new(|| {
    println!("Value changed!");
}));
```
appear repeatedly in doc-comments. Constitution Principle 6 (AGENTS.md "Logging via `tracing` only. No `println!`") — doc-comments compile-as-doctests. The rustdoc examples ARE shipped + executed in CI (`cargo test --doc`).
**Why cycle-3 missed it:** Cycle-3 audit focused on production code; doctests were outside scope.
**Proposed fix shape:** Replace `println!` with `tracing::info!` or `// react to change` placeholder comments throughout foundation doc-comments. Same sweep should cover `callbacks.rs` (line 30, 51, 82, etc.).
**Breaking-change risk:** None.
**Cross-vendor verdict:** —

---

### F27 — `Diagnosticable::to_diagnostics_node` uses full `type_name` (with module path) vs Flutter's simple `runtimeType`

**Dimension:** D5 Parity
**Severity:** P3
**Location:** `crates/flui-foundation/src/debug.rs::Diagnosticable::to_diagnostics_node` default impl
**Cycle-3 status:** Never audited.
**Evidence:**
```rust
fn to_diagnostics_node(&self) -> DiagnosticsNode {
    let type_name = std::any::type_name::<Self>();
    let mut node = DiagnosticsNode::new(type_name);
    ...
}
```
For a `flui_rendering::objects::render_padding::RenderPadding`, this yields the fully-qualified name. Flutter's `runtimeType.toString()` yields simply `RenderPadding`. DevTools output is verbose-noisy as a result.

Note that `Id::fmt` (id.rs:312) already uses the right pattern: `let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);`.
**Proposed fix shape:** Mirror the `Id::fmt` strip:
```rust
let type_name = std::any::type_name::<Self>();
let simple_name = type_name.rsplit("::").next().unwrap_or(type_name);
let mut node = DiagnosticsNode::new(simple_name);
```
**Breaking-change risk:** Yes — devtools/inspector output text changes. Tests that string-match the full path will break. Audit needed.
**Cross-vendor verdict:** —

---

### F28 — `Identifier` trait has redundant conversions (`Into<Index>` + `get(self) -> Index`)

**Dimension:** D4 Idiom
**Severity:** P3
**Location:** `crates/flui-foundation/src/id.rs:425-445`
**Cycle-3 status:** Never audited (cycle-3 T-14 simplified one direction but kept both as the public Identifier surface).
**Evidence:**
```rust
pub trait Identifier:
    ... + Into<Index> + ... + 'static
{
    fn get(self) -> Index;
    fn zip(index: Index) -> Self;
    fn try_zip(index: Index) -> Option<Self>;
}
```
Both `id.into()` and `id.get()` produce `usize`. Two API paths for the same conversion — increases cognitive cost. The blanket impl uses `id.unzip()` for `get`, and `Id<T>: Into<Index>` is also implemented separately at `id.rs:362-367`.
**Why cycle-3 missed it:** T-14 unification was about the `From<Index> for Id<T>` direction, not the `Id → Index` direction.
**Proposed fix shape:** Make `Identifier::get` the canonical path. Remove the `+ Into<Index>` bound from `Identifier` (callers that need usize call `.get()`). The `impl<T: Marker> From<Id<T>> for Index` at id.rs:362 stays as a convenience but is no longer required by the Identifier bound. Combine with F30 cleanup.
**Breaking-change risk:** Low — callers that did `let n: usize = id.into()` can `let n: usize = id.get()`.
**Cross-vendor verdict:** —

---

### F29 — `assert.rs::debug_assert_valid!` and friends reinvent `debug_assert!`

**Dimension:** D4 Idiom
**Severity:** P3
**Location:** `crates/flui-foundation/src/assert.rs` (entire file)
**Cycle-3 status:** Cycle 3 closed I-14 (deleted `report_error!` / `report_warning!`). The sibling cleanup of `debug_assert_*` macros was not done.
**Evidence:** Each macro:
```rust
macro_rules! debug_assert_valid {
    ($cond:expr, $($arg:tt)+) => {
        if cfg!(debug_assertions) && !$cond {
            panic!($($arg)+);
        }
    };
    ...
}
```
Expands to exactly what `debug_assert!($cond, $($arg)+)` already does (stdlib `cfg`-removes in release). The custom macro adds:
1. A fixed message prefix? — no, the macro forwards user message verbatim.
2. A specific lint? — no.
3. Telemetry? — no.

Net: zero added value over stdlib `debug_assert!`. Same cleanup as cycle-3 I-14.
**Why cycle-3 missed it:** I-14 swept tracing-related macros; the assert-related macros were on the same axis but missed.
**Proposed fix shape:** Replace all consumers of `debug_assert_valid!` / `debug_assert_range!` / `debug_assert_finite!` / `debug_assert_not_nan!` with stdlib `debug_assert!`. Delete the macros. ~80 LOC reduction.
**Breaking-change risk:** Breaking for any external consumer using the macros (none in workspace per grep — the macros are in the `prelude` but no one imports the prelude in a binding context).
**Cross-vendor verdict:** —

---

### F30 — Partial T-14 closure: `TreeWriteNav::move_children` / `insert_child` still bound `I: Into<usize>` where `I::get()` is canonical

**Dimension:** D4 + cycle-3 closure incompleteness
**Severity:** P2
**Location:** `crates/flui-tree/src/traits/write.rs:212` (move_children), `:256` (insert_child)
**Cycle-3 status:** T-14 closed `From<Index> for Id<T>` direction (always-available, not test-only). The symmetric direction (`Id → Index`) was left awkward.
**Evidence:**
```rust
fn move_children(&mut self, from: I, to: I) -> TreeResult<()>
where Self: Sized, I: Into<usize>,
{
    if !self.contains(from) {
        return Err(TreeError::not_found(from.into()));  // ← needs Into<usize> bound
    }
    ...
}
```
The `Identifier::get` method is the canonical path; using `into()` requires the bound and obscures the intent. Cycle 3 deferred this as "judgment call" but it's a clean two-site fix.
**Why cycle-3 missed it:** Closed T-14 by adding the always-available `From<Index> for Id<T>` impl, but did not sweep call-sites in TreeWriteNav that consume the inverse direction.
**Proposed fix shape:** Replace `from.into()` with `from.get()` (Identifier::get), drop the `I: Into<usize>` bound. ~6 LOC across 2 methods.
**Breaking-change risk:** None (relaxation of trait bound).
**Cross-vendor verdict:** —

---

## Summary

### Severity histogram
| Severity | Count | Finding IDs |
|---|---|---|
| **P0** | 3 | F2, F6, F19 |
| P1 | 5 | F5, F11, F15, F17, F24 |
| P2 | 8 | F4, F10, F12, F13, F14, F21, F25, F30 |
| P3 | 14 | F1, F3, F7, F8, F9, F16, F18, F20, F22, F23, F26, F27, F28, F29 |
| **Total** | **30** | |

### Dimension histogram
| Dim | Count | Finding IDs |
|---|---|---|
| D1 Soundness | 5 | F1, F2, F3, F6, F19 |
| D2 Concurrency | 6 | F4, F5, F6, F19, F20, F24 |
| D3 Variance / lifetime | 2 | F7, F8 |
| D4 Edition 2024 / 1.95 | 9 | F8, F9, F21, F22, F23, F26, F28, F29, F30 |
| D5 `.flutter/` parity | 8 | F5, F6, F10, F11, F12, F13, F25, F27 |
| D6 `.gpui/` patterns | 0 | (narrative only — GPUI's EventEmitter vs FLUI's ChangeNotifier diverge by intent; no specific finding) |
| D7 `bon` builder | 1 | F14 |
| D8 Diagnosticable derive | 1 | F15 |
| D9 Inline storage | 1 | F16 |
| D10 Test gap | 3 | F17, F18, F19 |

### Cross-vendor contradictions
**0** — all broadcasts FAILED (child harness lacks shell/intercom access). All P0 findings carry single-model analysis only. **Top-3 (F2, F6, F19) MUST be re-broadcast to Codex + Gemini from a write-and-shell-capable harness before sdd-design accepts the spec.**

### Estimated total fix workload (LOC bands)
| Band | Count | Finding IDs |
|---|---|---|
| **small** (<100) | 16 | F1, F2, F3, F4, F8, F9, F11, F16, F20, F22, F23, F26, F27, F28, F29, F30 |
| **medium** (100-400) | 11 | F5, F6, F7, F12, F14, F17, F18, F19, F21, F24, F25 |
| **large** (400-1000) | 3 | F10 (MergedListenable), F13 (ObserverList migration doc), F15 (Diagnosticable derive macro) |
| **mega** (>1000) | 0 | — |

**Net estimate:** ~2,500-4,000 LOC of churn (including tests + macro), of which ~600 LOC is net-additive (Diagnosticable derive macro + MergedListenable + DiagnosticsProperty variants) and the rest is in-place tightening / parity fixes / test additions.

### Top-3 must-fix candidates (severity-graded)

1. **F6 — Panic in one listener aborts the rest of `notify_listeners`** (P0, D1+D2+D5).
   *Why must-fix:* Constitution Principle 6 violation in operation (a buggy user listener can take down the entire UI via cascading unwinds). Flutter has the documented try/catch pattern; FLUI silently regressed on parity. Fix is ~10 LOC (`catch_unwind` per callback + tracing::error!) + ~30 LOC of regression tests. **Cycle 3 closed I-4 (per-frame alloc) but never audited the panic axis.**

2. **F2 — `Key::new()` overflow assert is off-by-one; post-wrap call is UB** (P0, D1).
   *Why must-fix:* `NonZeroU64::new_unchecked(0)` on the second post-wrap call is UB by construction. Practically unreachable in real production, but Rust's UB-elimination discipline (Constitution Principle 3) does not accept "unreachable in practice" as a defense. Fix is ~5 LOC (replace `new_unchecked` with `new().expect()` — eliminates the unsafe AND fixes the off-by-one). **Cycle 3's I-7 deferral proposed `try_new` but didn't trace the state machine across catch_unwind boundary.**

3. **F19 — `TreeWrite::remove` default cascade has no cycle-detection guard; corrupted tree OOMs** (P0, D1+D10).
   *Why must-fix:* Defense-in-depth missing. Cycle 3 T-12 added cycle-bound to `Ancestors::next` precisely for the corrupted-tree case; the cascade walk is the symmetric direction and was overlooked. Cycle 3 PR #103 hardened against deep-but-valid trees but never wrote the corrupted-but-cyclic test. Fix is ~15 LOC (cycle counter + visited set) + ~30 LOC of regression tests. **The fix also closes F24 (Vec → SmallVec) in the same edit.**

---

## Notes on the chain-run environment

- **Coordination/shell tools unavailable in this child harness** (bg_task → "Tool not found"; intercom + contact_supervisor → "Broker failed to start within timeout").
- **Multi-agent broadcasts could not be attempted.** Per the task escape hatch, top-3 verdicts are marked `BROADCAST FAILED — child session lacks shell access; finding stands on single-model analysis.`
- **Parent action required for sdd-design entry:** re-dispatch from a harness with shell + write access; broadcast F2, F6, F19 to Codex (Rust unsafe/UB pathways) and Gemini (large-context `.flutter` + `.gpui` cross-check); update findings inventory with cross-vendor verdicts before sdd-spec accepts the requirements.

---

*End of exploration.md.*


---

## Cross-vendor verdicts — Codex (OpenAI gpt-5-codex) broadcast appended by parent

Parent harness re-broadcast F2 / F6 / F19 via `codex exec` (the explore-step's child harness had no shell access). Verdicts below are the actual Codex output; the design phase must INTEGRATE them — note especially that Codex changed the F2 fix shape and challenged F19's P0 severity.

### F2 — Codex verdict (Key::new UB)

Verdict: **F2 is valid as a Rust soundness bug, but practical exploitability is near-zero unless the counter can actually be driven to overflow.** If your policy is “safe API must never reach UB,” it survives review.

1. **Yes.** `fetch_add` returns the old value and wraps the stored atomic. At `u64::MAX`, the call panics after storing `0`. If that panic is caught, the next call returns `0`, passes `id != u64::MAX`, then calls `NonZeroU64::new_unchecked(0)`, which is UB.

2. **Yes, catch-unwind is realistic.** Rust test harnesses, Tokio task boundaries, plugin/hot-reload shells, and panic-containment wrappers can all catch panics. The unrealistic part is reaching `u64::MAX` organic allocations. That lowers practical severity, not soundness validity.

3. **Partial fix only.** `NonZeroU64::new(id).expect(...)` eliminates UB, so it is an acceptable emergency safety patch. But it is not a complete correctness fix: after `MAX` panic, retry returns `0` and panics, then retry returns `1`, creating duplicate keys. That violates uniqueness.

4. **Use a saturating/sentinel CAS loop or `fetch_update`.** Best shape: never let the counter wrap back into the valid key range. Use `0` as a permanent exhausted sentinel.

```rust
let id = COUNTER
    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        if current == 0 {
            None
        } else {
            Some(current.wrapping_add(1)) // MAX -> 0 exhausted sentinel
        }
    })
    .expect("Key counter overflow");

Self(NonZeroU64::new(id).expect("counter returned zero"))
```

This returns `u64::MAX` once, then all future calls panic without mutation, UB, or duplicates.

---

### F6 — Codex verdict (listener panic)

Validated. Current FLUI diverges from Flutter here.

1. Yes. `callback()` is called directly inside the loop. With normal `panic = unwind`, listener N panics, unwinding exits `notify_listeners`, so N+1.. do not fire. With `panic = abort`, the process aborts.

2. `AssertUnwindSafe` is justified if the policy is Flutter parity: isolate each listener. It is an explicit promise that `ChangeNotifier` itself has no borrowed mutable invariants across the unwind boundary. It does not make listener-captured state safe; a listener can leave its own `Mutex`/atomic/app state inconsistent. That is inherent in continuing after panic.

3. The fail-fast argument is valid in Rust generally, but not for a Flutter-compatible `ChangeNotifier`. This API is user callback dispatch; Flutter catches per-listener. The proposed catch region should include only `callback()`, so direct notifier invariants are outside it. But no, Rust cannot reliably distinguish “user listener panic” from “internal invariant panic reached through user listener calling FLUI” unless FLUI uses structured panic payloads/error boundaries. Strings are not a contract.

4. `set_hook` is not an alternative. It is global, observational, runs before unwind handling, cannot resume iteration, and cannot scope diagnostics to this notifier. At most use it to reduce duplicate panic noise in tests/apps.

5. The proposed fix is Rust-idiomatic enough for Rust 1.95. Prefer:
```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

for callback in &callbacks {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| callback())) {
        tracing::error!(?payload, "ChangeNotifier listener panicked; continuing");
    }
}
```

Cheaper alternative: none with same semantics. `catch_unwind` has real overhead, but only around notification callbacks. If hot-path cost matters, add a compile/runtime policy flag, but default should be Flutter parity.

Verdict: finding is valid. Fix it, and add a test proving listener 2 fires after listener 1 panics.

---

### F19 — Codex verdict (cascade cycle)

F19 is valid. I would fix it in the trait default, but I would not call it P0 unless corrupted storage is attacker-reachable through deserialization, plugin input, FFI, or unsafe code.

1. **OOM risk:** Real for corrupted/user-implemented trees. Public `add_child` no-cycle enforcement reduces normal API exploitability, but does not protect the public trait default. The default must be defensive.

2. **`visited.contains`:** Fine for small typical trees, but it makes large valid removals potentially O(N²). Prefer `HashSet` if `I: Hash + Eq` is already available. If adding `Hash` is a breaking bound, `SmallVec` plus a hard step cap is acceptable as a compatibility patch.

3. **`max_steps = len * 2`:** Not too tight for a valid tree: reachable nodes cannot exceed `len()`. It is loose enough for minor duplication, but still bounds cyclic traversal. Use `checked_mul(2).unwrap_or(usize::MAX)` or saturating as proposed. The stronger guard is `visited`; the step counter is a fail-closed backstop.

4. **`None` vs `Err`:** Semantically, this should be `Err(TreeError::CycleDetected)`. Practically, changing `remove` from `Option` to `Result` is breaking. Best path: add `try_remove(...) -> Result<Option<Node>, TreeError>`, make `remove` call it and return `None` on cycle with `tracing::warn!`.

5. **Guard location:** Put it in the trait default. `remove_shallow` has no traversal context and cannot reliably detect cascade cycles. Override implementations must document and test equivalent cycle protection.

Recommended patch shape: visited set before pushing children, step cap, no mutation until traversal completes, warn/error on detection.

---

## Impact of cross-vendor verdicts on the design phase

- **F2 fix shape changes**: the original `NonZeroU64::new(id).expect(...)` fix is INCOMPLETE per Codex — eliminates UB but introduces a NEW correctness bug (duplicate keys after catch_unwind+retry). The correct fix is the `fetch_update` sentinel pattern (COUNTER=0 marks permanent exhaustion; retries panic without producing duplicates). Design phase MUST adopt this shape, not the partial `.expect()`.
- **F6 fix validated unchanged**: Codex confirms the proposed `catch_unwind + AssertUnwindSafe + tracing::error!` pattern is correct AND mandates a regression test 'listener 2 fires after listener 1 panics'.
- **F19 severity may downgrade and fix shape changes**: Codex disputes P0 unless corrupted storage is attacker-reachable through deserialization/plugin/FFI/unsafe. Severity rationale must be re-justified. Fix shape: prefer HashSet for visited (avoid O(N²) on large valid removals) IF `I: Hash + Eq` is available; SmallVec + step cap as compatibility fallback. New API recommendation: add `try_remove() -> Result<Option<Node>, TreeError>` and have `remove` call it (returning None+warn on cycle) — preserves existing `Option<Node>` signature.

