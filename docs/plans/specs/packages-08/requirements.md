# Typed component themes and exact inherited invalidation

- **Status:** In implementation
- **Date:** 2026-11-07
- **Source:** `packages-08`
- **Owner direction:** resolve the audit findings as one completed replacement, without retaining the old component-field model

## Problem

Material's `ThemeData` repeats twenty component slots in its value, baselines, patch and copy path. Whole-theme reads clone the aggregate and subscribe to every slot. There is no package extension point. Material and Cupertino also disagree on what a missing required theme means.

## Requirements

- **R1. Typed entries.** An `InheritedView` can report changed dynamically typed entries by `TypeId`, alongside statically typed `FieldMask` changes. Dynamic types never occupy field bits.
- **R2. Read-is-subscribe.** `BuildContextExt::depend_on_entry::<Provider, Entry, _>` records the entry before invoking user code. `depend_on` means all fields and all entries; `depend_on_field` selects only fields.
- **R3. Dispatch.** A dependent rebuilds when either its read fields intersect changed fields or its read entries intersect changed entries. Add, remove and replace are changes; content equality suppresses notification.
- **R4. Lifecycle parity.** Build reads are re-derived per successful build. Lifecycle reads persist until unmount. Reset, pruning, misses, reparenting and panic recovery preserve the existing field-dependency contract for entries too.
- **R5. Component extension.** Material exposes `ComponentTheme: Clone + Debug + PartialEq + 'static` with halfway interpolation by default. `ThemeData` stores owner-local `Rc` erased values keyed by `TypeId`, with typed insertion, removal, lookup, patching and interpolation.
- **R6. One component model.** All twenty old public component fields, their baseline copies, patch fields and production readers are removed in the same change. Components use `Theme::component_of::<T>`; a nearest `Theme` without that entry returns `None` rather than searching an outer theme.
- **R7. Base fields.** Color scheme, text theme, focus ring, selection, platform, style and page transitions remain ordinary fields with static masks. Baseline construction has one shared implementation. Existing static-field interpolation remains intact.
- **R8. Missing theme policy.** Required `Theme::of` and `CupertinoTheme::of` panic without their respective ancestor. Both catalogs expose `maybe_of`. Application shells continue publishing their resolved defaults.
- **R9. Tests.** Public-API regression tests cover exact invalidation, whole reads, build reset, lifecycle persistence, callback panic recovery, equal/add/remove/replace entries, typed lerp and nearest-scope behavior. Each new regression is checked against the unfixed behavior or an equivalent compile-time absence.
- **R10. Records.** A new ADR states the complete rule and supersedes the relevant theming/field-mask text. ADR-0074, crate architecture documents and `changelog.d` describe only the new model. The independent packaged-SDK `PackageTheme` fixture remains unchanged.

## Non-goals

- Copying Material's dynamic component map into Cupertino without a consumer.
- Renaming the catalog theme types as part of this finding.
- Per-field interpolation inside component themes; their existing halfway semantics remain the default.
- Replacing the retained-tree invalidation scheduler or signal registry.
