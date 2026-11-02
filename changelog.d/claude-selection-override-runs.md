### Changed
- A selectable paragraph inside a `SelectionArea` whose span sets an accessibility override publishes text runs: each overridden span is a region read as its override that holds its authored runs, a silenced span is a hidden region holding its runs, and a selection across it covers its characters (ADR-0283).
