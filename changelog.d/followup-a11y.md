### Changed
- `Semantics::expandable(expanded, on_expand, on_collapse)` replaces `Semantics::on_expand` and `Semantics::on_collapse`, so expand and collapse handlers are always published with the expanded state that makes them reachable.
### Fixed
- An `Expand` or `Collapse` request for the state a node already publishes is refused instead of running its handler.
- A numeric range that no value write can reach is published read-only, and `SetValue` is advertised only when a handler some platform write reaches exists: a number reaches the numeric handler even when the range also shows text, and a numeric string written to a range without a text handler reaches its numeric handler.
