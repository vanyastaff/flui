### Added

- `Slider::identifier` (`flui-widgets`, ADR-0240) names the slider's node.
- **Duplicate identifier diagnostic** (`flui-protocol` 0.6, `flui-semantics`, ADR-0240 §4): siblings
  that share an identifier are reported, never repaired. A wire read's new `Tree.diagnostics` lists
  a `TreeDiagnostic::DuplicateIdentifier` naming the parent and the siblings, and in debug builds
  publishing such a tree queues a `SemanticsDiagnostic::DuplicateIdentifier` that the frame driver
  logs. Siblings are those an agent sees, after unlabelled containers are flattened. The
  identifiers are still published as written.

### Changed

- `SemanticsOwner::take_diagnostics` returns `SemanticsDiagnostic`, which carries an inapplicable
  attribute or an identifier clash; `report_inapplicable` is now `report_diagnostic`, and
  `flui-testing`'s `assert_semantics_applicable` is now `assert_semantics_clean`.
- `Slider`'s semantics wrap its focus instead of sitting inside it, so its own configuration forms
  its node; the published node is unchanged.
- ADR-0240 §4: an identifier must be unique among its siblings, as AccessKit requires of
  `author_id`; it need not be unique anywhere else.
