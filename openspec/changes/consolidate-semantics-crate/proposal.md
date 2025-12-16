# Change: Consolidate Semantics into flui-semantics Crate

## Why

Currently there are two implementations of semantics types:
1. `flui-semantics` crate - basic SemanticsNode, SemanticsTree, SemanticsOwner
2. `flui_rendering/src/semantics/` - more complete implementation with SemanticsConfiguration, SemanticsAction handlers, SemanticsEvent, etc.

This duplication causes:
- Confusion about which types to use
- Inconsistent APIs between the two implementations
- Missing optimizations in both implementations
- No integration with platform accessibility APIs (accesskit)

## What Changes

1. **BREAKING**: Migrate all semantics code from `flui_rendering/src/semantics/` to `flui-semantics` crate
2. **BREAKING**: Remove duplicate types from `flui_types/src/semantics/` that are now in `flui-semantics`
3. Add optimizations:
   - `smol_str` for label/hint/value strings (O(1) clone)
   - `smallvec` for children and actions collections
   - `rustc-hash` for fast HashMap operations
4. Add `accesskit` integration for platform accessibility APIs
5. Update `flui_rendering` to depend on `flui-semantics` instead of having its own implementation

## Impact

- Affected specs: flui-rendering, flui-semantics (new)
- Affected crates:
  - `flui-semantics` - major additions
  - `flui_rendering` - remove semantics module, add dependency
  - `flui_types` - potentially remove duplicate semantics types
- Breaking changes for any code importing from `flui_rendering::semantics`

## Migration Path

1. All imports from `flui_rendering::semantics::*` should change to `flui_semantics::*`
2. `SemanticsNodeId` renamed to `SemanticsId` (consistent with other ID types)
3. String fields like `label`, `hint`, `value` change from `String` to `SmolStr`
4. Vec fields like `children`, `actions` change to `SmallVec`
