### Changed

- **A role WAI-ARIA forbids a name publishes its label as its description** (`flui-semantics`):
  caption, code, deletion, emphasis, insertion, paragraph and strong nodes carry no AccessKit
  label; the label leads the description, joined to the hint by a newline; the text of the nodes
  a `labelled_by` relation names leads the description too.

### Added

- **Agent protocol 0.4: a node's `description`** (`flui-protocol`, `flui-semantics`,
  `flui-desktop-mcp`): UI Automation's `FullDescription`, read in process from the published
  AccessKit node and on the desktop from UI Automation, so a paragraph's text reaches an agent.
