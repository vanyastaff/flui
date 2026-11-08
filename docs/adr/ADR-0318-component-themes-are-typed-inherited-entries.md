# ADR-0318: Component themes are typed inherited entries

- **Status:** Implemented (2026-11-08)
- **Date:** 2026-11-08
- **Supersedes:** [ADR-0074](ADR-0074-realm-scoped-signals.md) §5.5 only, its field-mask-only
  inherited-dependency record and dispatch rule
- **Related:** [ADR-0042](ADR-0042-theming-ownership.md) (design-system ownership and per-window
  theme publication, unchanged); [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (crate topology,
  unchanged)

## Context and problem statement

Material's `ThemeData` carried twenty fixed public component-theme slots. Each slot was repeated in
light/dark baselines, overrides and copying, and the aggregate had no typed extension point for a
downstream component. A whole-theme read cloned that aggregate and subscribed to every component;
static field masks could not represent an open set of component types. Material and Cupertino also
disagreed on whether a required theme lookup without an ancestor manufactured a default.

How can `flui-view` invalidate dynamically typed entries precisely, and how can Material expose one
extensible component-theme model without retaining the fixed fields or changing theme ownership?

## Decision drivers

- Field identity is static; component identity is an open set of Rust types. The two must not share
  an untyped numeric coordinate.
- Reading is subscribing. A dependency must survive a panic in the read callback or the rest of build.
- The frame path is synchronous and owner-local; component storage needs no lock or `Send + Sync`
  requirement.
- Additions, removals and replacements are changes; content equality suppresses invalidation.
- Build reads are re-derived, lifecycle reads persist, and whole-provider reads include future fields
  and entries.
- A replacement removes the old component-field model and its documentation in the same change.
- Material and Cupertino keep separate theme types and ownership; their required/optional lookup
  policy should nevertheless agree.

## Considered options

1. Keep Material's fixed component fields, allocate more `FieldMask` bits and add another field for
   every downstream component family.
2. Add a `TypeId`-keyed inherited-entry protocol beside `FieldMask`, store Material component themes
   as typed entries, and remove the fixed component fields.

## Decision

Chosen option: 2, because it preserves compile-time identity for static fields, supports an open set
of component types, and gives exact invalidation without duplicating Material's aggregate and patch
machinery for every component.

### 1. Inherited dependencies have field and entry coordinates

`flui-view` exports:

```rust,ignore
pub struct EntrySet { /* NONE, ALL, or a finite set of TypeId */ }

pub trait InheritedView: View {
    fn changed_entries(&self, old: &Self) -> EntrySet {
        EntrySet::NONE
    }
    // Existing associated data, field and child methods remain.
}

pub trait BuildContextExt {
    fn depend_on_entry<T, E, R>(
        &self,
        f: impl FnOnce(&T) -> R,
    ) -> Option<R>
    where
        T: InheritedView,
        E: 'static;
}
```

`FieldMask<T::Data>` remains the static coordinate. `EntrySet` is the dynamic coordinate and may be
`NONE`, `ALL`, or a finite set of entry `TypeId`s. Dynamic entries never occupy field bits. Providers
report added, removed and content-changed entries from `changed_entries`; its default reports no
entries, so existing field-only providers retain their behavior.

Each dependent stores build and lifecycle masks for both coordinates. `depend_on_field` selects only
fields, `depend_on_entry` selects only one entry, and `depend_on` selects `ALL` fields and `ALL`
entries. An update schedules a dependent when either coordinate intersects, and schedules a reader
matching both only once.

Registration precedes invocation of the user read callback. A missing provider returns `None` without
invoking it. Successful builds replace and prune build reads of both kinds; lifecycle reads persist
until unmount. Panic recovery retains previous reads and any newly registered read, so a recovered
element can be rebuilt by the next relevant change.

### 2. Material stores typed component entries, not public component fields

Material exports:

```rust,ignore
pub trait ComponentTheme: Clone + Debug + PartialEq + 'static {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        if t < 0.5 { self.clone() } else { other.clone() }
    }
}
```

`ThemeData` retains exactly seven public base fields: `color_scheme`, `text_theme`, `focus_ring`,
`selection`, `platform`, `style` and `page_transitions_theme`. Their `InheritedData` masks and
existing interpolation remain. Its private component store is:

```rust,ignore
BTreeMap<TypeId, Rc<dyn ErasedComponentTheme>>
```

The private erased contract supplies `Any` access, content equality and typed interpolation. Every
map operation constructs the value under the same `T` as its key; a downcast mismatch is therefore a
`BUG:` invariant failure, not a recoverable input case. Cloning `ThemeData` shares immutable `Rc`
entries. Owner-local storage imposes no cross-thread bound.

`with_component<T>` inserts or replaces one entry, `without_component<T>` removes it, and
`component<T>` borrows it. `ThemeDataOverrides` has matching typed operations: no patch preserves an
entry, insertion replaces it, and an explicit removal tombstone deletes it. The twenty former public
component fields, their override fields, baseline copies and parallel accessors are removed rather
than deprecated or retained as aliases. Light and dark baselines share one base-field constructor;
component defaults stay in component resolution.

`Theme::component_of<T>` calls `depend_on_entry::<Theme, T, _>` and clones only `T`. It returns `None`
when no `Theme` exists or the nearest theme lacks `T`; it never searches an outer theme for a missing
entry. Production component readers use this accessor. Base-only readers use exact static-field
accessors instead of `Theme::of`; `Theme::of` and `maybe_of` remain explicit whole-theme reads.

`Theme::changed_entries` compares the union of old and new keys by content. During interpolation,
two present values dispatch to `ComponentTheme::lerp_to`; a one-sided entry switches presence at
`t = 0.5`. Base-field interpolation is unchanged.

### 3. Required and optional theme lookup is consistent

`Theme::of` and `CupertinoTheme::of` panic without their respective ancestor and direct callers to
`maybe_of` for an optional result. Both `maybe_of` accessors return `None` in that case and otherwise
return the nearest installed theme, with Cupertino's normal resolution applied. Neither accessor
manufactures a default.

`MaterialApp` and `CupertinoApp` continue publishing their resolved defaults.
`CupertinoTheme::maybe_brightness_of` retains its separate optional-override plus `MediaQuery`
semantics. This decision does not merge Material and Cupertino theme data or move their ownership.

### Consequences

- Good, because downstream component themes use the same typed storage, patching, interpolation and
  invalidation path as built-in components.
- Good, because changing one component rebuilds its readers and whole-theme readers, not unrelated
  component or base-field readers.
- Good, because entry dependencies have the same build-reset, lifecycle-persistence and panic-recovery
  guarantees as static fields.
- Good, because seven base fields and one map replace twenty duplicated component slots and patches.
- Bad, because callers of removed fields must migrate to typed entry operations; no source-compatible
  field alias remains.
- Bad, because a dynamic entry provider must implement a correct content diff. Returning `NONE` for a
  real change suppresses its readers, so the production provider has focused regression coverage.
- Neutral, because cloning a component read clones its value while cloning the containing theme shares
  immutable entries; this is intentional owner-local value semantics.

### Confirmation

- The compiler ties `depend_on_entry`'s provider and entry types to the read, and `ComponentTheme`
  requires typed cloning, equality and `'static` identity. Existing sealed-context and field-selector
  trybuild fixtures continue to prevent untyped dependency forwarding.
- `crates/flui-view/tests/inherited_entries.rs` pins entry-set algebra,
  add/remove/replace/equal-content dispatch, field/whole-read separation, successful-build reset and
  pruning, lifecycle persistence, provider misses and callback/build panic recovery.
  `entry_read_reroutes_after_global_key_reparent` also proves that a moved reader leaves its old
  provider, reads the new one and responds only to later changes from the new provider.
- `packages/flui-material/tests/component_themes.rs` pins content comparison,
  insert/replace/remove/tombstone patching, typed and halfway interpolation, exact production invalidation
  and nearest-theme scope. Disabling `Theme::changed_entries` makes its replacement assertion fail.
- `packages/flui-cupertino/tests/theme.rs::missing_ancestor_is_optional_for_maybe_of_and_panics_for_of`
  and `nearest_ancestor_is_resolved_for_of_and_maybe_of` pin both lookup policies.
- Material's production tree has no component-field reads and no production `Theme::of` call in
  base-only widgets; its package tests and strict Clippy cover the migrated readers.

## Pros and cons of the options

### 1. Fixed component fields and more field bits

- Good, because static access and `InheritedData` derivation are already familiar.
- Good, because no dynamic set or erased storage is needed for the built-in catalog.
- Bad, because the field set is closed and finite, so downstream components cannot extend it without
  changing Material's public aggregate.
- Bad, because baselines, overrides, copying and equality repeat every slot and whole-theme readers
  subscribe to unrelated components.
- Bad, because retaining the fields beside an entry API would preserve two supported models and invite
  new callers to copy the old one.

### 2. Typed inherited entries and Material component storage (chosen)

- Good, because `TypeId` represents an open component set independently of the 64-bit static mask.
- Good, because one protocol supplies exact invalidation, lifecycle behavior and recovery to Material
  and other inherited providers.
- Good, because typed patches and interpolation remove duplicated per-component machinery.
- Bad, because erased storage requires private downcast invariants and providers must report content
  changes correctly.
- Bad, because removing fixed fields is a deliberate breaking API migration.

## More information

ADR-0042 remains authoritative for design-system ownership and per-window publication. ADR-0028's
crate topology is unchanged: official packages still depend on the SDK, not on each other. ADR-0074
§5.5 states the current shared scheduler and lifecycle rule; this ADR owns the entry coordinates and
Material component representation.

Revisit the storage shape only for a demonstrated need that `TypeId` identity, owner-local immutable
entries and typed interpolation cannot express. A new mechanism must replace both coordinates' users
and their records in one change rather than introducing a third theme model.
