### Added

- Agent protocol 0.18: a semantics read lists an `unpublished_identifier` diagnostic for an
  identifier on a node the agent and accessibility clients lift into its parent (no role, label
  or text, or the `Generic` or `Presentation` role), naming the node it is lifted into, at most
  64 per read; a 0.17 client ignores it. A Tester lookup or expectation by `find::id` that finds
  nothing because of such an identifier fails with `TestError::UnpublishedIdentifier`, which
  says why and gives the form that is found: a role on the node, or the identifier on the
  control it sits inside.

### Fixed

- `find`'s documentation, the Tester's failure hint, the book's testing pages and
  `Semantics::identifier` taught that a container with only an identifier is found by
  `find::id`; such a node is lifted into its parent, so they now give the form that is:
  `Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`.
