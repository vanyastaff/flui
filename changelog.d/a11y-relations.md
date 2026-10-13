### Added

- **Semantics relations** (`flui-semantics`, `flui-widgets`; ADR-0205): `SemanticsAnchor` names a
  node, and `SemanticsConfiguration::add_relation` with `SemanticsRelation::LabelledBy` or
  `Controls` relates another node to it; the owner resolves anchors when it publishes, including
  incrementally when a target remounts. `Semantics::anchor`, `labelled_by` and `controls` author
  them, and `EditableText::semantic_hint` describes a field.

### Fixed

- **Dialogs and field errors are announced in context** (`flui-material`): an `AlertDialog` is
  named by its title (merged into one node even when the title forms nodes of its own), and a
  `TextField` is described by its error or helper text.
- **A disclosure says what it opens** (`flui-widgets`): a `Disclosure` header controls its
  panel, a region named by the header, as WAI-ARIA's accordion pattern has it.
