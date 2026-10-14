### Fixed

- **Accessibility actions after a failed publication** (`flui-semantics`): when a semantics
  update callback panics, `SemanticsOwner::resolve_action` no longer admits an action, numeric
  value, text-as-number write or reveal that only the undelivered node offers. A request is now
  admitted only when the current tree serves it and the last delivered update offered it; a node
  that update did not carry is `NodeNotFound`. The next successful publication lifts the refusal.
