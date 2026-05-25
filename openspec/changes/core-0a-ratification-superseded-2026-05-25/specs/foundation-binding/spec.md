# Foundation — Binding Specification

## Purpose

Pin the canonical contract for FLUI's binding-singleton infrastructure —
the `BindingBase` trait, `HasInstance` marker trait,
`impl_binding_singleton!` declarative macro, and `check_instance<B>()`
free function — at parity with Flutter's `binding.dart` `BindingBase`
mixin chain while documenting the Rust-native shape (`OnceLock<Self>` +
`AtomicBool` for first-init signaling) and the cycle-3 init-after-panic
hazard fix (I-3).

Five workspace bindings (`SchedulerBinding`, `GestureBinding`,
`RendererBinding`, `SemanticsBinding`, `WidgetsBinding`) adopt this
contract via the macro. The spec pins the macro's correctness
guarantees so future bindings (e.g. Core.0d's pipeline binding wiring)
inherit them.

Owner crate: `crates/flui-foundation` — module `binding.rs`.

## Requirements

### Requirement: BindingBase trait shape

The `BindingBase` trait MUST require `Sized + Send + Sync + 'static`
on every implementor and MUST expose exactly two methods:
- `fn init_instances(&mut self);` — called once during construction,
  initialises every dependent singleton service.
- `fn is_initialized() -> bool where Self: HasInstance;` — provided
  default reads `<Self as HasInstance>::INITIALIZED.load(Acquire)`.

**Audit ref:** Mythos verdict #3 "Don't touch" — the `BindingBase` +
`HasInstance` + macro composition is the audit's gold-standard third
finding. This spec pins the shape so post-cycle-3 work does not drift.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/binding.dart:148-202`
(`abstract class BindingBase`) + `:289-302`
(`@protected void initInstances()`).

**Rust-native divergence:**
- (a) Flutter uses Dart mixin chains (`mixin FooBinding on BindingBase,
  BarBinding`) — initialisation order is the mixin chain order. FLUI
  uses Rust trait composition + the `impl_binding_singleton!` macro;
  initialisation order is the explicit order in which the binding's
  `new()` calls dependent `ensure_initialized()` methods.
- (b) Flutter's `BindingBase` is a class (with `@override` / `@mustCallSuper`
  enforcement); FLUI's is a trait (no Rust-native equivalent of
  `mustCallSuper`, but `init_instances` taking `&mut self` ensures
  the binding's `new()` is the only call site that can reach it).
- (c) No FLUI consumer breaks — every workspace binding already
  conforms.

#### Scenario: BindingBase trait can be used as a generic bound

- GIVEN a function `fn run<B: BindingBase + HasInstance>()`
- WHEN it is instantiated with each of the five workspace bindings
- THEN compilation MUST succeed for all five (proves the contract
  is satisfied workspace-wide)

#### Scenario: is_initialized reads INITIALIZED with Acquire ordering

- GIVEN the source file `crates/flui-foundation/src/binding.rs`
- WHEN searched for the body of `fn is_initialized` in the
  `BindingBase` trait definition
- THEN the body MUST call `INITIALIZED.load(Ordering::Acquire)`

---

### Requirement: HasInstance marker trait stores per-binding statics

The `HasInstance` trait MUST extend `BindingBase` and MUST expose:
- `const INITIALIZED: &'static AtomicBool;` — per-binding static
  flag tracking first-init completion.
- `fn instance() -> &'static Self;` — returns the singleton (
  initialises on first call via `OnceLock::get_or_init`).
- `fn ensure_initialized() -> &'static Self` — default-impl delegates
  to `instance()`; provided as the canonical caller-facing entry
  point.

Each binding's `impl HasInstance` MUST own its own `static INIT:
AtomicBool` and `static INSTANCE: OnceLock<Self>` — the macro
generates these as block-scoped statics so each binding type gets a
fresh pair.

**Audit ref:** Mythos verdict #3 (BindingBase composition).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/binding.dart:78-106`
(documentation of the `_instance` static field + `instance` getter
pattern; Flutter doesn't have an explicit `HasInstance` mixin
because Dart classes can declare statics directly).

**Rust-native divergence:**
- (a) Rust traits cannot carry associated `static`s — they carry
  associated `const`s. FLUI uses `const INITIALIZED: &'static
  AtomicBool` (a reference to a per-impl static), which is the
  Rust-native equivalent of Flutter's per-class `_instance` static.
- (b) The `OnceLock<Self>` static is inside the `instance()` body,
  not in the trait — it cannot be an associated const because each
  binding needs its own. The macro generates it.
- (c) No consumer breaks.

#### Scenario: Each binding's INITIALIZED flag is distinct

- GIVEN two binding types `A` and `B`, each invoking the macro
- WHEN `A::INITIALIZED` and `B::INITIALIZED` are dereferenced
  (yielding two `&'static AtomicBool` references)
- THEN the two pointer values MUST be distinct (each binding has its
  own static; flipping `A` does not flip `B`)

#### Scenario: instance() returns the same &'static reference across calls

- GIVEN any workspace binding `B` after first init
- WHEN `B::instance()` is called twice
- THEN both returns MUST be the same pointer (`std::ptr::eq` returns
  `true`)

---

### Requirement: impl_binding_singleton! macro defers INITIALIZED.store until after new() returns

The `impl_binding_singleton!($binding:ty)` macro MUST generate code
that:
1. Declares `static INIT: AtomicBool = AtomicBool::new(false);`
   inside the const-init block of `INITIALIZED`.
2. Declares `static INSTANCE: OnceLock<$binding> = OnceLock::new();`
   inside the `instance()` body.
3. Computes `let inst = INSTANCE.get_or_init(<$binding>::new);`
4. **AFTER** `get_or_init` returns successfully, fires
   `Self::INITIALIZED.store(true, Ordering::Release);`.
5. Returns `inst`.

The store MUST be on the path *after* `new()` succeeded — if `new()`
panics, control unwinds from `get_or_init` before the store fires,
and `INITIALIZED` stays `false` so the next caller's
`is_initialized()` correctly reports `false`.

**Audit ref:** I-3 (closed Wave 1+2 — store ordering flipped post-
new; regression test `init_panic_does_not_flip_initialized_flag` at
`binding.rs::tests`; verdict ratified as **permanent**).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/binding.dart:289-321`
(`initInstances` + `checkInstance` — Flutter is single-threaded, so
init-panic doesn't have the same race; Flutter's `initInstances`
throws and the framework reports diagnostics, leaving `_instance`
unset).

**Rust-native divergence:**
- (a) Flutter's single-threaded init never has a "partial state
  visible to other threads" race. FLUI's macro had a pre-cycle-3 bug
  where the store happened *inside* the `get_or_init` closure before
  `new()` returned: if `new()` panicked, `INITIALIZED` was `true`
  but `OnceLock` was empty, so the next `is_initialized() → true`
  caller observed incoherent state.
- (b) The cycle-3 fix flips the store to *after* `get_or_init`. The
  trade-off is one redundant atomic write per steady-state
  `instance()` call (the steady state already has `INITIALIZED ==
  true`). The audit's "future optimisation" note suggests
  `INITIALIZED.compare_exchange(false, true, Release, Relaxed).ok()`
  for the steady-state path; this spec leaves that as a future
  micro-optimisation, not a current requirement.
- (c) No FLUI consumer breaks — the contract is unchanged from the
  caller's perspective.

#### Scenario: Successful init flips INITIALIZED to true

- GIVEN a fresh binding `B` whose `new()` returns successfully
- WHEN `B::instance()` is called for the first time
- THEN after the call returns, `B::is_initialized()` MUST be `true`

#### Scenario: Panicking init leaves INITIALIZED at false (regression)

- GIVEN a binding `PanicBinding` whose `new()` always panics
- WHEN `std::panic::catch_unwind(|| { let _ = PanicBinding::instance(); })`
  is called
- THEN the closure MUST return `Err(_)` (panic propagated); AND
  `PanicBinding::is_initialized()` MUST be `false` (this is the
  audit I-3 regression test, verbatim from
  `binding.rs::tests::init_panic_does_not_flip_initialized_flag`)

#### Scenario: Macro generates per-binding scoped statics

- GIVEN two macro invocations for two distinct binding types in
  the same compilation unit (e.g. `impl_binding_singleton!(A);
  impl_binding_singleton!(B);`)
- WHEN both `A::instance()` and `B::instance()` are called
- THEN neither call MUST interfere with the other's `INITIALIZED`
  state (proves the `static INIT` is scoped to the macro expansion,
  not workspace-global)

---

### Requirement: check_instance<B>() panics with diagnostic if binding not initialized

`check_instance<B: HasInstance>() -> &'static B` MUST:
1. Call `B::is_initialized()`.
2. If `false`, panic with a message containing the binding type
   name (via `std::any::type_name::<B>()`) AND the suggested
   recovery (`Call <Name>::ensure_initialized() first.`).
3. Otherwise return `B::instance()`.

**Audit ref:** post-cycle ratification (no audit finding directly
covers `check_instance`; this requirement pins the existing
behaviour as the canonical contract).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/binding.dart:312-321`
(`static T checkInstance<T extends BindingBase>(T? instance)` —
asserts non-null, returns the unwrapped instance, throws
`FlutterError` otherwise).

**Rust-native divergence:**
- (a) Flutter's `checkInstance` operates on a `T? instance`
  argument (the `_instance` static). FLUI's reads `B::INITIALIZED`
  directly (the binding's own `AtomicBool`).
- (b) Flutter throws `FlutterError`; FLUI panics. The panic is the
  Rust idiom for "framework-startup contract violated" — equivalent
  observable behaviour: program halts with diagnostic.
- (c) No consumer breaks.

#### Scenario: check_instance on initialized binding returns the singleton

- GIVEN a workspace binding `B` that has been `ensure_initialized()`-ed
- WHEN `check_instance::<B>()` is called
- THEN the returned reference MUST point to the same singleton as
  `B::instance()` (proven by `std::ptr::eq`)

#### Scenario: check_instance on uninitialized binding panics with diagnostic

- GIVEN a never-initialised binding `B`
- WHEN `std::panic::catch_unwind(|| { check_instance::<B>(); })`
  is called
- THEN the caught panic message MUST contain the substring
  `"has not been initialized"` AND the binding's
  `std::any::type_name::<B>()` substring

---

### Requirement: Deferred audit finding I-12 — Flutter file:line refs in doc-comments (revisit-later-with-trigger)

Doc-comments on `BindingBase`, `HasInstance`, and `impl_binding_singleton!`
SHOULD cite Flutter `binding.dart` line ranges when describing parity
behaviour. New code in this domain MUST include `binding.dart:LINE-LINE`
cross-refs in any doc-comment that mentions "parity" or "mirrors
Flutter".

Existing doc-comments that lack file:line refs MAY remain (the
sweep is deferred per the cycle-3 deferral table: "~50 LOC doc
churn across multiple files — better done as a dedicated doc PR
with proper Flutter source verification").

**Verdict for this spec:** **revisit-later-with-trigger**.
Revival trigger: the parity-verification report
(`docs/research/2026-XX-XX-foundation-parity-verification.md`)
discovers a divergence whose root cause is a missing file:line
ref leading to drift. The trigger condition is recorded in
`crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

**Audit ref:** I-12 (deferred → revisit-later-with-trigger).

#### Scenario: New doc-comments in binding.rs cite Flutter ref

- GIVEN any new commit that touches `crates/flui-foundation/src/binding.rs`
  and adds a doc-comment containing the substring "parity",
  "Flutter", or "mirrors"
- WHEN a CI doc-lint step (or manual review) inspects the diff
- THEN at least one `binding.dart:` line-range citation MUST appear
  in the new doc-comment OR the comment explicitly notes "no Flutter
  equivalent"

---

### Requirement: BindingBase + HasInstance impls satisfy the workspace's five existing adopters without code change

This spec's requirements MUST be satisfied by the five current
workspace bindings without modification:
`SchedulerBinding` (`flui-scheduler`),
`GestureBinding` (`flui-interaction`),
`RendererBinding` (`flui-app/bindings/renderer_binding.rs`),
`SemanticsBinding` (`flui-semantics`),
`WidgetsBinding` (`flui-app/bindings/`).

**Audit ref:** Mythos verdict #3 evidence (audit Part I "BindingBase
singleton pattern" enumerates these five).

**Flutter ref:** Each FLUI binding has a Flutter counterpart; the
canonical reference is `binding.dart` plus the per-binding source
file (e.g. `services/binding.dart`, `gestures/binding.dart`).

#### Scenario: All five bindings build with the current macro

- GIVEN the workspace at HEAD
- WHEN `cargo check -p flui-scheduler -p flui-interaction -p flui-semantics
  -p flui-app` is run
- THEN exit code MUST be 0 (proves the five adopters satisfy the
  contract this spec pins)

#### Scenario: All five bindings respect the I-3 init-panic invariant

- GIVEN the test suite for the workspace
- WHEN `cargo test --workspace -- init_panic_does_not_flip_initialized_flag`
  or any equivalent regression test is run
- THEN the assertion MUST pass for the binding module's test
  (the regression test lives in `flui-foundation/src/binding.rs`
  using a synthetic `PanicBinding` because the five production
  bindings cannot be made to panic on demand)
