### Added
- `SemanticsConfiguration::set_hosted_runs` names the range of a paragraph's text runs a semantics region of that paragraph holds (ADR-0282).

### Changed
- A selectable paragraph with links inside a `SelectionArea` publishes text runs: they are cut at link edges and listed under each link and under a `Label` node for the text between links, so a screen reader selects and reads across and inside links.
- The web host shows the runs a link holds as its element's text.
