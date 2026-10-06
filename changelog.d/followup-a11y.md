### Changed
- `Semantics::expandable(expanded, on_expand, on_collapse)` replaces `Semantics::on_expand` and `Semantics::on_collapse`, so expand and collapse handlers are always published with the expanded state that makes them reachable.
### Fixed
- An `Expand` or `Collapse` request for the state a node already publishes is refused instead of running its handler.
- A numeric range whose value write reaches no handler is published read-only, and `SetValue` is advertised only when the handler chosen by the text-before-range precedence exists.
