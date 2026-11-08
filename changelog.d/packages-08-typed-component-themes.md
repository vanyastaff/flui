### Added

- Add `EntrySet`, `InheritedView::changed_entries` and
  `BuildContextExt::depend_on_entry` for dynamically typed inherited entries
  with exact build and lifecycle invalidation.
- Add Material's extensible `ComponentTheme` contract and typed
  `ThemeData`/`ThemeDataOverrides` insertion, removal, lookup and patching.
  `Theme::component_of::<T>` reads one component entry from the nearest theme.

### Changed

- Material component readers now subscribe only to their own typed entry;
  base-only readers subscribe to the exact color-scheme or text-theme fields.
  Whole-theme reads explicitly include every field and entry.
- `CupertinoTheme::maybe_of` now returns `None` without an ancestor, while
  `CupertinoTheme::of` panics with guidance instead of manufacturing a default.
  Both accessors resolve the nearest installed theme.

### Removed

- Remove Material's twenty fixed public component-theme fields, their override
  patches and duplicated baseline storage. Use `with_component`,
  `without_component`, `component` and `Theme::component_of` instead; no legacy
  field aliases or parallel accessors remain.
