### Added

- **Inline links**: `TextSpan::with_link` marks a span's text as a link to a `TextLink` (a URL
  or an app tag), and `RichText::on_link` handles it with the activation's `EventCx`. A tap on
  the link's glyphs goes through the gesture arena, the pointer over it is a pointing hand, and
  each handled link is an accessible link node with a click action; hit testing and node bounds
  follow the text scale. `InlineSpan::links` lists a span tree's links in document order.
- **`HitTestContext::set_self_hit_pointer_target` and `set_self_hit_cursor`**: a box's
  `hit_test` routes the hit and picks its cursor for the part under the pointer, in place of
  the node-wide `pointer_target` and `mouse_cursor`; `HitTestOutcome` carries both.
- **`TextLinkRoute`** and `RenderParagraph::{with_link_routes, set_link_routes, link_routes}`:
  the lane identities a paragraph's links are reached through.

### Changed

- `TextSpan::mouse_cursor` and `TextSpan::with_mouse_cursor` take `CursorIcon`, the type the
  platform cursor output takes; a span's cursor now applies over its glyphs and its children's.

### Removed

- `TextSpan::with_on_tap`, `TextSpan::on_tap` and `TextSpan::is_interactive`: a link is data on
  the span and `RichText::on_link` handles it.
- `flui_painting::typography::MouseCursor`: use `CursorIcon`.
