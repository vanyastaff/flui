### Added

- `Slider::identifier` (`flui-widgets`, ADR-0240) names the slider's node.
- **Duplicate identifier diagnostic** (`flui-protocol` 0.6, `flui-semantics`, ADR-0240 §4): siblings
  that share an identifier are reported, never repaired. A wire read's new `Tree.diagnostics` lists
  a `TreeDiagnostic::DuplicateIdentifier` naming the parent and the siblings, and debug builds log
  a warning when such a tree is published. The identifiers are still published as written.

### Changed

- `Slider`'s semantics wrap its focus instead of sitting inside it, so its own configuration forms
  its node; the published node is unchanged.
- ADR-0240 §4: an identifier must be unique among its siblings, as AccessKit requires of
  `author_id`; it need not be unique anywhere else.
