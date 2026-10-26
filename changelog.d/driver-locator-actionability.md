### Added

- **Locators and actionability facts** (`flui-semantics`, `flui-protocol`; ADR-0238): a
  `Locator` resolves over the published semantics tree: `SemanticsOwner::locate_wire` lists the
  matches (role, name, value, description, identifier, state, `within`, `has`, `pick`), and
  `resolve_wire_locator` answers the one element or `WireLocateError::{NotFound, Ambiguous}` with
  the nearest or the ambiguous elements as outline lines. `actionability_point` and
  `actionability_wire` read an element's attached, visible, stable, enabled and receives facts,
  with the render hit-test run by the caller in between. `SemanticsOwner::revision` counts the
  publications that delivered something. `TextMatch::matches` folds case by Unicode;
  `Actionability::new` builds the facts of an element.

### Changed

- `LocateParams` documents what the behaviour is: a strict locator lists every match, and
  strictness answers `ambiguous` where one element is needed.
