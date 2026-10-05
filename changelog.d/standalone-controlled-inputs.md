### Added
- Controlled slider and disclosure widgets with pointer, keyboard, focus and accessibility input.
- Checked numeric range metadata and distinct numeric, expand and collapse semantics actions.
### Changed
- `flui_semantics::ActionArgs` gains `SetNumericValue`, so an exhaustive `match` on it needs a new arm. `SemanticsAction` gains `Expand`, `Collapse` and `SetNumericValue`; it is `#[non_exhaustive]`, so existing matches still compile.
- Platform and agent `expand`/`collapse` requests reach the discrete `Expand`/`Collapse` actions; an expandable node with only a tap handler still receives the transition its state allows through that handler.
### Fixed
- Preserve numeric platform action payloads and refuse a numeric value outside the node's current range.
- A gesture group disposed while its pointer route is retained no longer receives that route's later events; the route still gets its terminal event.
