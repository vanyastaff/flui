### Added
- Controlled slider and disclosure widgets with pointer, keyboard, focus and accessibility input.
- Checked numeric range metadata and distinct numeric, expand and collapse semantics actions.
### Fixed
- Preserve numeric platform action payloads and refuse a numeric value outside the node's current range.
- A gesture group disposed while its pointer route is retained no longer receives that route's later events; the route still gets its terminal event.
