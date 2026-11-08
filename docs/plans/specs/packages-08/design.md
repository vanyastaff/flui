# Design: typed component themes

## Inherited entries

`flui-view` adds a cloneable `EntrySet` containing either `ALL` or a set of `TypeId`s. Construction from `E` and union/intersection operations keep the existing sealed-context discipline: public callers use the typed extension, while object-safe recording remains crate-token gated.

`InheritedView` gains:

```rust
fn changed_entries(&self, old: &Self) -> EntrySet {
    let _ = old;
    EntrySet::NONE
}
```

`BuildContextExt` gains `depend_on_entry::<T, E, R>`. The existing recording path carries both `FieldSet` and `EntrySet` through `DependentRecord`, `DependentEntry`, both context implementations and lifecycle tagging. Whole `depend_on` supplies `ALL` for both. Reset clears build fields and build entries but not lifecycle fields or lifecycle entries. Pruning removes only a dependent with no reads of either kind.

`InheritedBehavior::on_view_updated` computes both change sets once, refreshes its caches before user callbacks, and schedules the union of matching readers in registration order. The user callback is still preceded by registration, including on the panic path.

## Material storage

`ComponentTheme` is the public typed contract. A private generic erased wrapper implements object-safe equality, `Any` access and typed interpolation. Maps hold `Rc<dyn ErasedComponentTheme>`; cloning a theme shares immutable entries, and insertion/removal clones only the map.

`ThemeData` has seven public base fields and a private component map. Its public builder-style API is:

```rust
ThemeData::light().with_component(MyTheme { .. })
theme.without_component::<MyTheme>()
theme.component::<MyTheme>()
```

`ThemeDataOverrides` retains patches for the seven base fields and a private typed patch map. `with_component` means replace/insert and `without_component` is an explicit tombstone, so `copy_with` can distinguish no patch from removal. Component lookup and patches are content-compared, not pointer-compared.

`ThemeData` manually implements `InheritedData` for its base fields. `Theme::changed_entries` diffs the union of old/new component keys. `Theme::component_of<T>` clones only `T` after recording only `TypeId::of::<T>()`. `Theme::of` remains the explicit whole-theme read.

`ThemeData::lerp_to` interpolates base fields as before. For component keys, two present values dispatch to `ComponentTheme::lerp_to`; one-sided presence switches at `t = 0.5`.

All production component readers migrate to `Theme::component_of`. There are no legacy component fields, aliases or duplicate accessors left behind.

## Missing ancestors

`CupertinoTheme::maybe_of` returns the resolved nearest theme or `None`. `of` calls it and documents the same missing-ancestor panic policy as Material. `maybe_brightness_of` retains its distinct optional-override and `MediaQuery` semantics. `CupertinoApp` remains the source of the ordinary default.

## Replacement and documentation

A new ADR-0318 states the complete entry/component/no-ancestor rule. ADR-0074 §5.5 is rewritten to describe two coordinate systems and their shared lifecycle. It must not retain an assertion that the current implementation is unasserted once tests land. Material, Cupertino and `flui-view` architecture documents name their behavioral tests. The changelog records removed fields and replacement APIs.
