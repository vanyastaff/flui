# Foundation — ID System Specification

## Purpose

Pin the canonical contract for FLUI's generic `Id<T: Marker>` system
— the `RawId` underlying representation, the `Identifier` blanket
trait, the `Marker` trait, the `markers::*` module of zero-sized
marker enums, the `ids!` declarative macro, and the per-domain ID
type aliases (`ViewId`, `ElementId`, `RenderId`, `LayerId`,
`SemanticsId`, `ListenerId`, `ObserverId`, `FrameId`, `FrameCallbackId`,
`TaskId`, `TickerId`) — at parity with no Flutter equivalent (Flutter
uses plain `int` IDs; FLUI's generic shape is a deliberate Rust-native
improvement) while documenting the **+1 offset pattern** that
underpins every Slab-backed tree storage.

Cycle 3 closed I-18 partially (`Marker + Debug` was retained per
audit deferral). Findings I-9, I-10, I-17, I-18 are in the
deferred-13 set; this spec assigns each a verdict.

Owner crate: `crates/flui-foundation` — module `id.rs`.

## Requirements

### Requirement: RawId is repr(transparent) over NonZeroUsize with compile-time size assertions

`RawId` MUST be declared `#[repr(transparent)]` over `NonZeroUsize`,
MUST derive `Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`,
and MUST be the niche-optimised carrier for every `Id<T>`.

Two compile-time `const _: () = { assert!(...) };` blocks MUST
enforce:
1. `size_of::<RawId>() == size_of::<usize>()` (pointer-sized
   for efficient passing).
2. `size_of::<RawId>() == size_of::<Option<RawId>>()` (niche
   optimisation makes `Option<RawId>` the same size as `RawId`).

**Audit ref:** Mythos verdict #2 (`Id<T: Marker>` generic discipline
is "Don't touch"; the compile-time asserts are explicitly cited as
load-bearing). I-10 covers `RawId` visibility — see verdict below.

**Flutter ref:** None — Flutter uses `int` IDs (Dart's tagged
pointer, 63-bit). FLUI's `NonZeroUsize` shape is a **Rust-native
improvement**.

**Rust-native divergence:**
- (a) Flutter: `int` IDs, no niche optimisation, no per-domain
  type discipline (Element / Render / Layer IDs are all `int`).
- (b) FLUI: `NonZeroUsize` wrapped in `RawId`, niched
  `Option<RawId>` (8 bytes), `Id<T>` adds per-domain type
  discipline at zero runtime cost.
- (c) No consumer breaks — `Id<T>` shape predates the audit.

#### Scenario: Compile-time size assertions hold

- GIVEN the workspace at HEAD
- WHEN `cargo check -p flui-foundation` is run
- THEN exit code MUST be 0 (proves the two `const _ = assert!(...)`
  blocks at `id.rs:54-62` evaluate true on the target platform)

#### Scenario: Option<RawId> is the same size as RawId

- GIVEN a runtime test
- WHEN `assert_eq!(std::mem::size_of::<Option<RawId>>(), std::mem::size_of::<RawId>())`
  is asserted
- THEN the assertion MUST pass (this is a runtime mirror of the
  compile-time check; ensures the niche optimisation is observable
  to tests too)

---

### Requirement: Id<T: Marker> + 1 offset pattern for Slab-backed storage

Every concrete `Id<T>` (`ElementId`, `RenderId`, `LayerId`,
`SemanticsId`, `ViewId`) MUST be constructed via the **+1 offset
pattern**: a Slab's 0-based index `i` becomes the ID `Id::new(i + 1)`,
and ID lookup uses `id.get() - 1` to index back into the Slab.

This pattern leverages the `NonZeroUsize` invariant: the slab's
"absent" sentinel is `0`, which `NonZeroUsize` reserves, so
`Option<Id<T>>` is one machine word and a missing-slot lookup
returns `None` without any extra discriminant.

`Id::new(n: usize)` MUST panic if `n == 0` (via `NonZeroUsize::new(n).expect(...)`).
`Id::new_checked(n: usize) -> Option<Self>` MUST return `None`
for `n == 0` (safe alternative).

**Audit ref:** Mythos verdict #2 (Constitutional ID Offset Pattern
as a generic). The audit explicitly cites this as the contract
that every consumer inherits for free.

**Flutter ref:** None — Flutter uses raw `int` indices into its
ChildList / Slab equivalents; no Rust-style niche optimisation.

**Rust-native divergence:**
- (a) Flutter: 0-based indices, separate "is set" bookkeeping.
- (b) FLUI: 1-based IDs, niched `Option<Id<T>>`, no separate
  bookkeeping needed. The +1 offset is the cost; the niched
  `Option` is the benefit.
- (c) Every workspace tree (RenderTree, LayerTree, SemanticsTree,
  ElementTree, ViewTree) uses this pattern. No consumer breaks.

#### Scenario: Id::new(0) panics

- GIVEN no prior context
- WHEN `std::panic::catch_unwind(|| { let _ = ElementId::new(0); })`
  is called
- THEN the closure MUST return `Err(_)` (panic propagated with
  message containing "non-zero" or "must be non-zero")

#### Scenario: Id::new_checked(0) returns None

- GIVEN no prior context
- WHEN `let v = ViewId::new_checked(0);` is called
- THEN `v` MUST equal `None`

#### Scenario: +1 offset pattern round-trips correctly

- GIVEN a slab index `i = 5_usize`
- WHEN `let id = ElementId::new(i + 1); let back = id.get() - 1;`
- THEN `back` MUST equal `5` (proves the canonical pattern; this
  is the form every Slab-backed tree uses)

---

### Requirement: Marker trait carries 'static + WasmNotSendSync (+ Debug)

The `Marker` trait MUST require `'static + WasmNotSendSync` so
`Id<T>` is `Send + Sync` (on native) and works on wasm-without-
threads.

The trait MUST currently retain the `+ Debug` supertrait bound
(I-18 deferral). The marker types generated by the `ids!` macro
MUST be uninhabited enums (`pub enum FooMarker {}`) with
`#[derive(Debug)]` for the bound.

**Audit ref:** I-18 (deferred → accept-permanent in this spec).
The audit's "Removing requires touching every concrete marker;
cost > benefit" rationale stands.

**Flutter ref:** None — FLUI-native plumbing for type-safe IDs.

**Rust-native divergence:** Pure Rust-native; no Flutter analog.

**Verdict for I-18 (drop `+ Debug` from Marker):**
**accept-permanent**. The `Id::fmt` implementation uses
`std::any::type_name::<T>()` which does NOT require `T: Debug`;
the bound is vestigial. But removing it requires touching every
marker (~11) plus any external consumer using the `Marker` bound
directly (`flui-scheduler::IdGenerator<M: Marker>`). Per the
audit deferral, the touch-cost outweighs the aesthetic gain.

Revival trigger: workspace-wide trait-bound cleanup pass where
`+ Debug` removal can be batched with similar drops elsewhere.
Recorded in `crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

#### Scenario: Marker is a Send + Sync supertrait of every concrete marker

- GIVEN a generic function `fn assert_marker<M: Marker>()` declared
  in a test
- WHEN it is instantiated with `markers::Element`, `markers::Render`,
  `markers::Layer`, `markers::Semantics`, `markers::View`,
  `markers::Listener`, `markers::Observer`, `markers::Frame`,
  `markers::FrameCallback`, `markers::Task`, `markers::Ticker`
- THEN compilation MUST succeed for all 11 markers (proves the
  bounds are satisfied workspace-wide)

#### Scenario: Marker types are uninhabited enums

- GIVEN any marker, e.g. `markers::Element`
- WHEN inspected at compile time via a `match` statement on a
  hypothetical value (which would require `unreachable!()`)
- THEN the type MUST be uninhabited: zero variants, zero size
  (proven by `size_of::<markers::Element>() == 0` assertion)

---

### Requirement: Id<T> type aliases declared via the ids! macro

The `ids!` declarative macro MUST be the canonical way to declare a
new ID family. Invoking it with `ids! { View, Element, Render, ... }`
MUST generate, for each name:
1. A `pub enum NameMarker {}` (uninhabited) in `pub mod markers`,
   with `#[derive(Debug)]`.
2. A `impl Marker for NameMarker {}` impl.
3. A `pub type NameId = Id<markers::NameMarker>` alias.

The five tree-architecture IDs (`ViewId`, `ElementId`, `RenderId`,
`LayerId`, `SemanticsId`), the two listener IDs (`ListenerId`,
`ObserverId`), and the four scheduler IDs (`FrameId`,
`FrameCallbackId`, `TaskId`, `TickerId`) MUST all be declared via
the macro.

**Audit ref:** Mythos verdict #2 (the macro composes well — single-
line declaration of new ID families).

**Flutter ref:** None — no Flutter analog; Dart `int` IDs have no
per-domain type discipline.

#### Scenario: Macro-generated ID alias is the same size as Id<T>

- GIVEN runtime check
- WHEN `assert_eq!(std::mem::size_of::<ElementId>(), std::mem::size_of::<RawId>())`
- THEN the assertion MUST pass

#### Scenario: ID types are not interchangeable at compile time

- GIVEN `let e: ElementId = ElementId::new(1);`
- WHEN a downstream file tries `let r: RenderId = e;`
- THEN compilation MUST fail with a type-mismatch error (proves
  per-domain type discipline; `RenderId` is `Id<markers::Render>`,
  not `Id<markers::Element>`)

---

### Requirement: Identifier blanket trait — every Id<T> implements it; Index is a usize alias

The `Identifier` trait MUST be the public bound used by
`flui-tree`'s `TreeRead<I: Identifier>` / `TreeNav<I: Identifier>`
/ `TreeWrite<I: Identifier>` trait family.

`Identifier` MUST expose:
- `fn get(self) -> Index;` — extract the raw `usize`.
- `fn from_index(index: Index) -> Self;` — construct (panics on 0).

`Index` MUST be the type alias `pub type Index = usize;`.

A blanket `impl<T: Marker> Identifier for Id<T>` MUST exist so every
ID family member satisfies the bound for free.

**Audit ref:** I-10 (deferred — `RawId` + `Index` visibility — see
verdict below). T-14 closed (Wave 4+5 — `From<Index>` always
available, no longer `#[cfg(test)]`).

**Flutter ref:** None — FLUI-native trait.

#### Scenario: Identifier blanket impl satisfies the tree-trait bound

- GIVEN a generic function `fn use_id<I: Identifier>()` in a test
- WHEN it is instantiated with `ElementId`, `RenderId`, `LayerId`,
  `SemanticsId`, `ViewId`
- THEN compilation MUST succeed for all five (proves the
  blanket impl + per-tree consumer pattern)

#### Scenario: From<Index> for Id<T> works in production (not only tests)

- GIVEN production code (no `#[cfg(test)]` gate)
- WHEN `let id: ElementId = ElementId::from(42_usize);` is written
  in a non-test context
- THEN compilation MUST succeed (this is the T-14 close: the impl
  is unconditional, no longer gated to tests)

---

### Requirement: Deferred audit finding I-9 — Id<T> unsafe constructors stay public (revisit-later-with-trigger)

`Id::<T>::from_raw(raw: RawId)`, `Id::<T>::zip_unchecked(index: Index)`,
and `Id::<T>::new_unchecked(index: Index)` (the three `unsafe`
constructors) MUST remain `pub`. The audit's recommendation to
downgrade them to `pub(crate)` is deferred per the cycle-3
deferral table: "`flui-scheduler::id::*` actively re-exports
these. Locking them down would break the scheduler's public API
contract."

Each `unsafe` constructor MUST carry a `# Safety` doc-comment
explicitly stating the caller's `NonZeroUsize` invariant
obligation.

**Audit ref:** I-9 (deferred → revisit-later-with-trigger in this
spec).

**Flutter ref:** None — Flutter has no `unsafe` (Dart is memory-
safe by construction).

**Rust-native divergence:**
- (a) Pure Rust-native plumbing; Flutter has no analog.
- (b) The `unsafe` constructors are the escape hatch for the
  serde deserialize path (`id.rs::serde_impl`) and for
  `flui-scheduler::IdGenerator` — both have invariant-checked
  upstream code, so the `unsafe` is safety-audited.

**Verdict for I-9:** **revisit-later-with-trigger**.
Revival trigger: a future workspace-wide audit (cycle 4+) that
also covers `flui-scheduler::id::*` AND `flui-scheduler` agrees
to migrate off the public `unsafe` constructors. At that point,
the visibility downgrade is a single PR with no public-API break.
Recorded in `crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`.

#### Scenario: Public unsafe constructors remain accessible

- GIVEN a downstream test file
- WHEN `use flui_foundation::id::Id;` followed by
  `let _ = unsafe { ElementId::new_unchecked(1) };`
- THEN compilation MUST succeed (proves the `pub` visibility is
  preserved per I-9 deferral)

#### Scenario: Each unsafe constructor documents its safety invariant

- GIVEN the source `crates/flui-foundation/src/id.rs`
- WHEN inspected near each of `pub const unsafe fn from_raw`,
  `pub const unsafe fn zip_unchecked`, `pub const unsafe fn new_unchecked`
- THEN each MUST be preceded by a `/// # Safety` section
  describing the caller's `NonZeroUsize`-non-zero obligation

---

### Requirement: Deferred audit finding I-10 — RawId and Index stay public (revisit-later-with-trigger)

`pub struct RawId(NonZeroUsize)` and `pub type Index = usize` MUST
remain in the public API surface. The audit's recommendation to
downgrade to `pub(crate)` is deferred for the same reason as I-9
(scheduler consumer).

**Audit ref:** I-10 (deferred → revisit-later-with-trigger in this
spec).

**Flutter ref:** None.

**Rust-native divergence:** Pure FLUI-native; pinned for
`flui-scheduler::IdGenerator` interop.

**Verdict for I-10:** **revisit-later-with-trigger**.
Revival trigger: same as I-9 (workspace-wide audit cycle 4+ that
also covers `flui-scheduler::id::*`).

#### Scenario: RawId and Index are publicly importable

- GIVEN a downstream test file
- WHEN `use flui_foundation::{RawId, Index};` is written
- THEN compilation MUST succeed (proves the public visibility is
  preserved per I-10 deferral)

#### Scenario: flui-scheduler re-exports RawId and Index

- GIVEN `crates/flui-scheduler/src/id.rs`
- WHEN searched for the import line
- THEN it MUST contain a `use flui_foundation::{...}` that
  enumerates `Index` and/or `RawId` (proves the deferral
  rationale's "scheduler consumer" claim is currently true)

---

### Requirement: Deferred audit finding I-17 — ValueNotifier mutation methods kept (accept-permanent)

`ValueNotifier::take(&mut self) where T: Default`, `replace(&mut self, T) -> T`,
and `value_mut(&mut self) -> &mut T` MUST remain in the API
surface. The audit's "audit / mark unused" recommendation is
deferred — these methods are used internally by tests and
provide an escape hatch for future consumers.

**Audit ref:** I-17 (deferred → accept-permanent in this spec).
This requirement also appears in `foundation-listenable-changenotifier/spec.md`
because the I-17 finding is on ValueNotifier; reproduced here only
for traceability completeness. The canonical home is the listenable
spec.

#### Scenario: take / replace / value_mut compile and round-trip values

- GIVEN `let mut v = ValueNotifier::new(42_i32);`
- WHEN `let prev = v.replace(100);`, then `*v.value_mut() += 1;`,
  then `let taken = v.take();`
- THEN `prev == 42`, `taken == 101`, and post-take
  `*v.value() == 0` (default for i32) MUST all hold

---

### Requirement: Deferred audit finding I-18 — Marker + Debug supertrait kept (accept-permanent)

The `pub trait Marker: 'static + WasmNotSendSync + Debug {}`
declaration MUST retain the `+ Debug` supertrait bound. The
audit's recommendation to drop it is deferred per the cycle-3
deferral rationale: "cost > benefit".

**Audit ref:** I-18 (deferred → accept-permanent in this spec).
Already covered above in the "Marker trait carries..." requirement;
this entry is the explicit verdict record.

#### Scenario: Marker trait declaration retains + Debug

- GIVEN `crates/flui-foundation/src/id.rs`
- WHEN searched for the `pub trait Marker` declaration
- THEN it MUST contain the substring `+ Debug` in its supertrait
  list (verifies the I-18 accept-permanent verdict is honoured)

---

### Requirement: Index type alias is publicly re-exported but discouraged in new APIs

The `pub use id::{Index, ...}` line in `crates/flui-foundation/src/lib.rs`
MUST remain so existing consumers (`flui-scheduler`) keep building.
New public API surfaces in `flui-foundation` or its consumers
SHOULD use `Identifier::get()` to obtain a `usize` rather than
exposing `Index` directly.

**Audit ref:** I-10 (related — already covered above with
revisit-later-with-trigger verdict).

#### Scenario: Existing re-export remains in lib.rs

- GIVEN `crates/flui-foundation/src/lib.rs`
- WHEN searched for `Index,` inside a `pub use id::{ ... }` block
- THEN exactly one match MUST appear (proves the public re-export
  is preserved)
