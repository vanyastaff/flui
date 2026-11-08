### Added

- Render views can make typed inherited-data reads through `RenderObjectContext`; provider changes reconfigure the same render object.
- `Align`, `Stack`, and `IndexedStack` accept `AlignmentGeometry` and resolve directional alignment against the enclosing `Directionality`.

### Removed

- The public `resolve_alignment` helper; these widgets now resolve alignment during render configuration.

### Fixed

- Render-behavior wrappers configure their render objects in their authoritative builds. Persistent-header extent changes wait for freshly clamped shrink state instead of building the delegate with the retained pair.
