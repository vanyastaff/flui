### Added

- `FocusTraversalGroup` orders descendants as a policy boundary in the existing focus tree, with inherited text direction, group edges, and weak explicit next/previous links that fall back when a target becomes unavailable.
- Geometric four-way focus navigation follows finite nonzero rectangles, prefers overlapping direction beams, and resolves ties in tree order. Default unmodified arrow shortcuts run after focused controls have declined them.

### Changed

- `FocusScope::edge_behavior` wires scope traversal edges through widget configuration.

### Fixed

- A rectangle provider that withdraws itself and panics preserves its first failure before capture retirement during both linear and directional focus traversal.
