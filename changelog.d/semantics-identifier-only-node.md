### Added

- Agent protocol 0.18: a semantics read lists an `unpublished_identifier` diagnostic for an
  identifier on a node the agent and accessibility clients lift into its parent (no role, label
  or text, or the `Generic` or `Presentation` role), naming the first node it is lifted into,
  each identifier once and at most 64 per read; a 0.17 client ignores it. A Tester lookup or
  expectation that matches nothing because of such an identifier, which no published node
  carries, fails with `TestError::UnpublishedIdentifier`, `to_be_gone` included. The error says
  why and gives the form that is found: a role on the node, or the identifier on the control it
  sits inside.

### Fixed

- `find`'s documentation, the Tester's failure hint, the book's testing pages and
  `Semantics::identifier` taught that a container with only an identifier is found by
  `find::id`; such a node is lifted into its parent, so they now give the form that is:
  `Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`.
