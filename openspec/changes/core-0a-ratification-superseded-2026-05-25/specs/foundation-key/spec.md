# Foundation — Key Specification

## Purpose

Pin the canonical contract for FLUI's widget-identity / reconciliation
key family — the `Key` simple type, `ValueKey<T>` value-equality key,
`UniqueKey` identity-only key, the `ViewKey` object-safe trait used
by the element-tree reconciliation pipeline, the `KeyRef` wrapper
used in `DynView`, the `Keyed` / `WithKey` helper traits, and the
related FLUI-side `ObjectKey<T>` / `GlobalKey<T>` types that live in
`flui-view::key` (cross-referenced here for completeness) — at parity
with Flutter's `key.dart` + `framework.dart::GlobalKey` while
documenting Rust-native shape divergences (const FNV-1a hash,
`NonZeroU64` niche optimisation, explicit `is_global_key()` cheap-skip).

Cycle 3 closed I-5 (deletion of surprising `Default for Key` /
`Default for UniqueKey` impls). The remaining audit findings on this
domain (I-6, I-7, I-8, I-21) are all in the deferred-13 set; this
spec assigns each a verdict.

Owner crate: `crates/flui-foundation` — module `key.rs`. Downstream
extensions: `crates/flui-view/src/key/` (`ObjectKey`, `GlobalKey<T>`).

## Requirements

### Requirement: Key is repr(transparent) over NonZeroU64 with niche-optimised Option

The `Key` type MUST be declared `#[repr(transparent)]` over
`NonZeroU64`, MUST derive `Clone, Copy, PartialEq, Eq, Hash`, and
MUST carry `#[must_use = "keys should be used for widget identification"]`.

`std::mem::size_of::<Option<Key>>()` MUST equal
`std::mem::size_of::<Key>()` MUST equal 8 bytes.

**Audit ref:** Mythos verdict (key family is a "Don't touch" gold-
standard); I-5 closed (no `Default` impl); I-21 deferred (covered
below).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/key.dart:29-48`
(`abstract class Key`).

**Rust-native divergence:**
- (a) Flutter's `Key` is a Dart class (`abstract class Key` with
  factory ctor); object identity is reference-based. Each Dart
  `Key.empty()` is a fresh allocation.
- (b) FLUI's `Key` is a `NonZeroU64` — Copy, `Option<Key>` is 8
  bytes via niche optimisation, equality is integer compare. This
  is the Rust-native equivalent of Flutter's reference identity,
  preserving the observable behaviour ("two keys are equal iff
  same source") at zero allocation cost.
- (c) No consumer breaks — `Key` shape has been stable since
  pre-cycle-1.

#### Scenario: Niche optimisation makes Option<Key> the same size as Key

- GIVEN the `Key` type definition in `flui-foundation`
- WHEN `std::mem::size_of::<Option<Key>>()` and
  `std::mem::size_of::<Key>()` are compared at compile time
- THEN both MUST equal 8 (`#[repr(transparent)]` over `NonZeroU64`
  enables the niche; this is enforced by a `const _: () = assert!(...)`
  in the test module or by a runtime `assert_eq!` in tests)

---

### Requirement: Key::from_str is const-evaluatable via FNV-1a hash

`Key::from_str(s: &str) -> Self` MUST be `const fn` so callers can
write `const HEADER: Key = Key::from_str("header");` and the hash
is computed at compile time with zero runtime cost. The hash
algorithm MUST be FNV-1a with `u64` output. If the FNV-1a hash of
the input is `0`, the implementation MUST substitute a non-zero
fallback (current: `1`) to honour the `NonZeroU64` invariant.

**Audit ref:** I-6 (deferred → accept-permanent in this spec).

**Flutter ref:** Flutter has no const-key equivalent —
`Key('header')` constructs at runtime. FLUI's const-hash is a
**deliberate Rust-native improvement** allowing compile-time keys.

**Rust-native divergence:**
- (a) Flutter: runtime string key, equality by string compare.
- (b) FLUI: compile-time u64 hash key, equality by integer
  compare. Trade-off: silent collision possible (audit I-6) —
  two distinct strings whose FNV-1a hashes happen to equal each
  other compare-equal. Probability ≈ 2^-64 per random string pair.
- (c) Existing call sites (e.g. `const HEADER: Key = Key::from_str("header");`
  patterns in tests + examples) do not break.

**Verdict for the I-6 fallback-to-1-on-zero-hash issue:**
**accept-permanent**. The audit's deferral rationale stands:
"the silent collision is a hash-function property, not a flaw
in the wrapper". A future revisit could implement either
(i) `Key::try_from_str(s: &str) -> Option<Self>` returning `None`
on the zero-hash case, or (ii) the rotate-XOR variant
guaranteeing non-zero. Both are additive APIs worth a separate
RFC; neither is binding here.

Revival trigger: a real workspace consumer of `Key::from_str`
encounters a hash collision in production OR cycle-4 audit
re-litigates the silent-collision concern with new evidence.

#### Scenario: from_str is const-evaluatable

- GIVEN a file with `const HEADER: Key = Key::from_str("header");`
- WHEN it is compiled
- THEN compilation MUST succeed (`from_str` MUST be `pub const fn`)

#### Scenario: from_str of empty string returns a non-zero key

- GIVEN `Key::from_str("")`
- WHEN the result's inner `NonZeroU64` is inspected via `as_u64()`
- THEN the value MUST NOT equal 0 (FNV-1a offset basis
  `14_695_981_039_346_656_037` is non-zero, so the empty-string
  case bypasses the `if hash == 0 { 1 }` fallback; verifies the
  fallback path is correctly never-triggered for the empty input)

#### Scenario: Same string yields equal keys; distinct strings (with high probability) yield distinct keys

- GIVEN `let a = Key::from_str("header"); let b = Key::from_str("header"); let c = Key::from_str("footer");`
- WHEN equality is checked
- THEN `a == b` MUST be `true`; `a != c` MUST be `true` (note: this
  is a probabilistic guarantee for the second clause; a future
  hash-collision-discovery test would explicitly assert the
  collision-resolution behaviour)

---

### Requirement: Key::new generates unique runtime keys via atomic counter; counter overflow always-panics

`Key::new() -> Self` MUST generate a unique runtime key by
fetching-then-adding 1 to a `static AtomicU64 COUNTER` (initialised
to 1) with `Ordering::Relaxed`. The fetched pre-increment value
becomes the key's `NonZeroU64`.

After fetch, the implementation MUST `assert!(id != u64::MAX, ...)`
in both debug and release builds. The audit's I-7 note about the
off-by-one in the overflow path (post-wrap `fetch_add` returns 0,
which would be UB for `NonZeroU64::new_unchecked`) is acknowledged
as a deferred follow-up.

**Audit ref:** I-7 (deferred → accept-permanent in this spec).
The audit's "Add `Key::try_new() -> Result<Self, KeyOverflow>` and
have `Key::new` call it" recommendation is the natural next step
when a real recovery callsite materialises.

**Flutter ref:** Flutter `UniqueKey` constructor at
`.flutter/packages/flutter/lib/src/foundation/key.dart:61-83`
(no shared counter — each `UniqueKey()` constructor allocates a
fresh Dart object, identity via Object hash).

**Rust-native divergence:**
- (a) Flutter: per-instance allocation, identity via Dart `Object`
  hash. Cheap by Dart standards.
- (b) FLUI: 8-byte `Key` value, identity via integer compare.
  Counter exhaustion is a 584-year-at-1-ns-per-call concern, so
  the assertion-panic is the pragmatic shape.
- (c) No consumer breaks.

**Verdict for the I-7 try_new follow-up:** **revisit-later-with-trigger**.
Revival trigger: a workspace consumer materialises that needs to
recover from counter overflow (e.g. a long-running test that
calls `Key::new()` in a tight loop). Recorded in
`crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

#### Scenario: Two new() calls return distinct keys

- GIVEN no prior context
- WHEN `Key::new()` is called twice
- THEN the two returned `Key` values MUST be distinct (`a != b`)

#### Scenario: Counter overflow assertion is `assert!` not `debug_assert!`

- GIVEN the source `crates/flui-foundation/src/key.rs::Key::new`
- WHEN searched for the overflow guard
- THEN exactly one `assert!(id != u64::MAX, ...)` line MUST appear
  AND zero `debug_assert!(id != u64::MAX, ...)` occurrences MUST
  appear (proves the audit's "UB is never acceptable, even in
  release" stance is honoured)

---

### Requirement: Key does NOT implement Default (I-5 ratification)

`impl Default for Key` MUST NOT exist. Construction MUST route
through `Key::new()` (unique, explicit), `Key::from_u64(n)`
(deterministic from external ID), or `Key::from_str("name")`
(compile-time constant from string).

Same applies to `UniqueKey`: `impl Default for UniqueKey` MUST NOT
exist.

**Audit ref:** I-5 (closed Wave 3 — `Default` impls deleted; verdict
ratified as **permanent**). The audit's rationale: every `Default`
call returned a different value (counter-bumped), so
`#[derive(Default)]` on parent types silently broke round-trip
equality.

**Flutter ref:** None — Dart has no `Default` equivalent. `Key()`
in Dart is the factory constructor that returns `ValueKey<String>`.

**Rust-native divergence:**
- (a) Pre-cycle-3: `impl Default for Key { fn default() -> Self { Self::new() } }`
  was a surprising-Default violation of Rust API guidelines
  (`API-DEFAULT`: defaults should be deterministic).
- (b) Post-cycle-3: the impl is deleted; the explicit constructors
  remain. This is a **deliberate breaking change**; the audit
  verified zero in-workspace consumers.
- (c) No consumer breaks.

#### Scenario: Default::default()::<Key>() does not compile

- GIVEN a hypothetical downstream file with `let k: Key = Default::default();`
- WHEN the file is type-checked
- THEN compilation MUST fail with "the trait `Default` is not
  implemented for `Key`"

#### Scenario: Default::default()::<UniqueKey>() does not compile

- GIVEN a hypothetical downstream file with `let k: UniqueKey = Default::default();`
- WHEN the file is type-checked
- THEN compilation MUST fail with "the trait `Default` is not
  implemented for `UniqueKey`"

---

### Requirement: ViewKey object-safe trait drives reconciliation

The `ViewKey` trait MUST be object-safe (every method MUST be
dyn-compatible) and MUST require `Send + Sync + 'static`. It MUST
expose:
- `as_any(&self) -> &dyn Any` — downcast support.
- `key_eq(&self, other: &dyn ViewKey) -> bool` — type-and-value
  equality.
- `key_hash(&self) -> u64` — efficient hash for lookup.
- `clone_key(&self) -> Box<dyn ViewKey>` — boxed clone.
- `debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` —
  debug representation.
- `is_global_key(&self) -> bool` — cheap-skip method for the
  `BuildOwner` global-key registry (see below).

**Audit ref:** I-8 (deferred — verdict assigned below); Mythos
verdict (`ViewKey::is_global_key()` cheap-skip is "Don't touch").

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/key.dart:51-58`
(`abstract class LocalKey extends Key` — Flutter splits the trait
hierarchy into `Key` / `LocalKey` / `GlobalKey`; FLUI uses a single
`ViewKey` trait with `is_global_key()` discriminator because Rust
trait hierarchies don't support Dart-style `extends` chains for
downcast-friendly dispatch).

**Rust-native divergence:**
- (a) Flutter: `LocalKey` and `GlobalKey<T extends State<StatefulWidget>>`
  are distinct subclasses; reconciliation does an `is GlobalKey`
  Dart type check. FLUI: single `ViewKey` trait, `is_global_key()`
  is the cheap-skip equivalent of Flutter's `is` check.
- (b) Flutter has Dart's `identityHashCode` for object-identity
  hashing; FLUI requires explicit `key_hash() -> u64` per impl
  (FLUI has no Object-identity equivalent for trait objects).
- (c) Existing impls (`ValueKey<T>`, `UniqueKey`, `flui-view`'s
  `ObjectKey<T>` + `GlobalKey<T>`) all implement these methods.
  No consumer breaks.

#### Scenario: ViewKey is dyn-compatible

- GIVEN a function `fn accept(_: &dyn ViewKey)` declared in a
  downstream module
- WHEN it is called with `&ValueKey::new(42)` and `&UniqueKey::new()`
- THEN compilation MUST succeed for both call sites (proves trait
  object dispatch works for both impls)

#### Scenario: key_eq is type-and-value comparison

- GIVEN `let a = ValueKey::new(42_i32);` and
  `let b = ValueKey::new(42_i64);`
- WHEN `a.key_eq(&b as &dyn ViewKey)` is called
- THEN it MUST return `false` (different TypeId — i32 vs i64 — even
  though the numeric values would compare equal)

#### Scenario: clone_key produces an equivalent boxed key

- GIVEN `let k: Box<dyn ViewKey> = Box::new(ValueKey::new(42_i32));`
- WHEN `let cloned = k.clone_key();`
- THEN `k.key_eq(&*cloned)` MUST return `true` (proves clone
  preserves type + value)

---

### Requirement: ViewKey::is_global_key default returns false (I-8 ratification)

`ViewKey::is_global_key(&self) -> bool` MUST have a default
implementation returning `false`. Only the widget-layer
`GlobalKey<T>` impl in `flui-view::key::global_key` MUST override
to return `true`.

The default-false shape is the deferred I-8 verdict: forcing every
key implementor to explicitly write `fn is_global_key(&self) -> bool
{ false }` is more noise than safety. The audit's risk assessment
("the default-false safety net catches the 'forgot to override'
case identically") is ratified here.

**Audit ref:** I-8 (deferred → accept-permanent in this spec).

**Flutter ref:** `.flutter/packages/flutter/lib/src/widgets/framework.dart`
(`GlobalKey` registration via `_currentElement` field — Flutter uses
Dart's `is GlobalKey` type check at the registration call site,
not a virtual method).

**Rust-native divergence:**
- (a) Flutter: `if (key is GlobalKey) ...` at the registration call
  site (`Element.mount`).
- (b) FLUI: `if key.is_global_key() ...` (consumed at
  `flui-view/src/tree/element_tree.rs:497`). Same observable
  behaviour; Rust-native shape via virtual dispatch instead of
  type-check.
- (c) Currently 3 in-workspace impls: `ValueKey<T>` and `UniqueKey`
  use the default (returns `false`); `flui-view::GlobalKey<T>`
  overrides to `true`. `flui-view::ObjectKey<T>` should also use
  the default. **A forgotten override on a future `GlobalKey<T>`
  cousin would silently break global-registry tracking.** The
  cycle-3 audit deemed the silent-break risk lower than the
  noise of forcing explicit overrides; the verdict stands.

Revival trigger: a future cycle 4+ audit discovers a forgotten-
override regression in a real `GlobalKey<T>` cousin. Recorded in
`crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

#### Scenario: ValueKey and UniqueKey use the default-false impl

- GIVEN `let v = ValueKey::new(42_i32); let u = UniqueKey::new();`
- WHEN `v.is_global_key()` and `u.is_global_key()` are called
- THEN both MUST return `false`

#### Scenario: flui-view::GlobalKey overrides to true

- GIVEN a `GlobalKey<TestState>::new()` value (in a `flui-view`
  integration test)
- WHEN `is_global_key()` is called on it
- THEN it MUST return `true`

---

### Requirement: ValueKey<T> compares by type and value

`ValueKey<T: Send + Sync + Hash + Eq + Clone + Debug + 'static>`
MUST compare equal iff `TypeId::of::<T>()` matches AND the inner
values compare equal under `T::eq`.

**Audit ref:** Mythos verdict (key family preservation).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/key.dart:88-126`
(`class ValueKey<T> extends LocalKey` — equality is `other is
ValueKey<T> && other.value == value`).

**Rust-native divergence:**
- (a) Flutter: `other is ValueKey<T>` for the type check.
- (b) FLUI: `TypeId::of::<T>() == other.as_any().type_id()` for
  the type check. Equivalent observable behaviour.
- (c) No consumer breaks.

#### Scenario: Same type, same value compares equal

- GIVEN `let a = ValueKey::new(42_i32); let b = ValueKey::new(42_i32);`
- WHEN `a.key_eq(&b)` is called
- THEN it MUST return `true`

#### Scenario: Different types do not compare equal

- GIVEN `let a = ValueKey::new(42_i32); let b = ValueKey::new(42_i64);`
- WHEN `a.key_eq(&b)` is called
- THEN it MUST return `false`

---

### Requirement: UniqueKey compares equal only to itself

`UniqueKey::new()` MUST allocate a fresh unique identity (delegating
to `Key::new`). Two distinct `UniqueKey::new()` calls MUST compare
not-equal. A `UniqueKey` clone MUST compare equal to its source
(Copy semantics on the inner `Key`).

**Audit ref:** Mythos verdict (key family preservation); I-5
closed (no `Default for UniqueKey`).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/key.dart:61-83`
(`class UniqueKey extends LocalKey` — equality is reference identity).

**Rust-native divergence:** None at observable behaviour; the
underlying mechanism (counter-bumped `NonZeroU64`) vs Flutter's
object identity is a parity-preserving Rust-native shape.

#### Scenario: Two new() UniqueKeys are not equal

- GIVEN `let a = UniqueKey::new(); let b = UniqueKey::new();`
- WHEN `a.key_eq(&b)` is called
- THEN it MUST return `false`

#### Scenario: A cloned UniqueKey compares equal to its source

- GIVEN `let a = UniqueKey::new(); let b = a;` (Copy)
- WHEN `a.key_eq(&b)` is called
- THEN it MUST return `true`

---

### Requirement: KeyRef wraps Key for object-safe DynView passing (I-21 ratification)

`KeyRef` MUST be `#[repr(transparent)]` over `Key`, MUST derive
`Clone, Copy, PartialEq, Eq, Hash`, and MUST expose:
- `const fn new(key: Key) -> Self` — constructor.
- `const fn as_u64(&self) -> u64` — raw access.
- `const fn key(&self) -> Key` — unwrap.
- `impl From<Key> for KeyRef` — idiomatic conversion.

Both `KeyRef::new` and `From<Key>` MUST remain — the audit's I-21
deprecation proposal is deferred per the cycle-3 deferral table:
"Both call sites exist; deprecation has a migration cost."

**Audit ref:** I-21 (deferred → accept-permanent in this spec).

**Flutter ref:** None — Dart has no `DynView`-equivalent object-
safe trait; `KeyRef` is a Rust-native plumbing type.

**Rust-native divergence:** Pure FLUI-internal scaffolding. No
consumer breaks; deferring deprecation avoids migration churn.

Revival trigger: a workspace-wide doc-cleanup pass standardises on
`Into<KeyRef>` everywhere AND zero remaining `KeyRef::new` call
sites exist. Recorded in
`crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

#### Scenario: Both KeyRef::new and Key.into() construct equivalent KeyRefs

- GIVEN `let k = Key::from_str("h");`
- WHEN `let a = KeyRef::new(k); let b: KeyRef = k.into();`
- THEN `a == b` MUST return `true` (proves both constructors
  produce the same value)

---

### Requirement: WithKey and Keyed helper traits expose key access on view types

The `WithKey` and `Keyed` traits MUST be exposed in the prelude
and MUST allow downstream view types to:
- `Keyed::key(&self) -> Option<KeyRef>` — read the optional key.
- `WithKey::with_view_key(self, key: impl Into<KeyRef>) -> Self` —
  builder-style attach a key to a view.

**Audit ref:** Mythos verdict (key family preservation; these
helpers are load-bearing for the `flui-view` reconciliation
pipeline).

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/key.dart`
+ `widgets/framework.dart::Widget.key` — Flutter exposes `key` as
a constructor parameter on `Widget`. FLUI's builder-style
`with_view_key` is the Rust-native equivalent for
move-by-value view construction.

**Rust-native divergence:**
- (a) Flutter: `MyWidget(key: ValueKey('a'))` — constructor param.
- (b) FLUI: `MyView::new().with_view_key(ValueKey::new("a"))` —
  builder. Equivalent observable behaviour.
- (c) Consumer crates (`flui-view`'s view-implementations) all
  use the trait; no breakage.

#### Scenario: with_view_key chains onto a view builder

- GIVEN a test view type `TestView` implementing `WithKey`
- WHEN `TestView::new().with_view_key(ValueKey::new(42_i32))` is
  called and the result's `Keyed::key()` is inspected
- THEN the returned `Option<KeyRef>` MUST be `Some(k)` where
  `k.key()` equals `KeyRef::from(ValueKey<i32>{42}).key()`

---

### Requirement: ObjectKey and GlobalKey live in flui-view::key, not flui-foundation

`ObjectKey<T>` (Flutter equivalent: `ObjectKey` in
`framework.dart`) and `GlobalKey<T>` (Flutter equivalent:
`framework.dart::GlobalKey`) MUST live in `crates/flui-view/src/key/`,
not in `crates/flui-foundation/src/key.rs`. They MUST implement
the `ViewKey` trait. `GlobalKey<T>` MUST override
`is_global_key()` to return `true`.

This is a layering invariant: foundation cannot depend on view-
layer types like the `BuildOwner` global-key registry that
`GlobalKey<T>` is bound to.

**Audit ref:** Architecture invariant (no specific finding; the
crate-graph contract — `flui-foundation` → no upward deps —
predates the audit).

**Flutter ref:** Flutter's `Key` / `LocalKey` / `UniqueKey` /
`ValueKey` live in `foundation/key.dart`; `ObjectKey` and
`GlobalKey` live in `widgets/framework.dart`. FLUI mirrors this
split.

#### Scenario: flui-foundation does not declare ObjectKey or GlobalKey

- GIVEN `crates/flui-foundation/src/key.rs`
- WHEN searched for `pub struct ObjectKey` and `pub struct GlobalKey`
- THEN zero matches MUST appear (proves the foundation-layer
  abstinence)

#### Scenario: flui-view::key declares ObjectKey and GlobalKey

- GIVEN the directory `crates/flui-view/src/key/`
- WHEN searched recursively for `pub struct ObjectKey` and
  `pub struct GlobalKey`
- THEN at least one match for each MUST appear
