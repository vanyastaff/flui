### Added

- Agent protocol 0.18: a semantics read lists an `unpublished_identifier` diagnostic for an
  identifier on a node that publishes nothing else (no role, label or text), naming the node it
  is lifted into; a 0.17 client ignores it. The Tester fails such a `find::id` with
  `TestError::UnpublishedIdentifier`, which says why nothing selects it and gives the form that
  is found.

### Fixed

- `find`'s documentation, the Tester's failure hint, the book's testing pages and
  `Semantics::identifier` taught that a container with only an identifier is found by
  `find::id`; such a node is lifted into its parent, so they now give the form that is:
  `Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`.
