### Added

- Native ScrollIntoView requests reveal materialized offscreen descendants through their actual scroll ancestors, including nested viewports, both axes and reversed directions.

### Fixed

- Reentrant reveal requests use published geometry and scroll position to expose the last accepted target without repeating stale movement. Automatic reveal snapshots cannot deliver to removed or replaced owners, and intentionally dropped reveal snapshots contain callback-capture retirement failures individually.
