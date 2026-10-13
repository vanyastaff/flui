### Changed

- **A role WAI-ARIA forbids a name publishes its label as its description** (`flui-semantics`):
  caption, code, deletion, emphasis, insertion, paragraph and strong nodes carry no AccessKit
  label; the label leads the description, joined to the hint by a newline, and a `labelled_by`
  relation is published as `described_by`.
