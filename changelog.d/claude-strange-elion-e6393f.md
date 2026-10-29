### Added

- **Inline links**: `TextSpan::with_link` marks a span's text as a link to a `TextLink` (a URL
  or an app tag), and `RichText::on_link` handles it with the activation's `EventCx`. A tap on
  the link's glyphs goes through the gesture arena, the pointer over it is a pointing hand, and
  each handled link is an accessible link node with a click action; hit testing and node bounds
  follow the text scale. `InlineSpan::links` lists a span tree's links in document order,
  leaving out a link that covers no text (`TextSpan::link_covers_text`).
- **Keyboard access to links**: each handled link is a focus stop measured by its glyphs, so
  Tab reaches links in reading order and a focused link scrolls into view. Enter and Select open
  it through the new `ActivateLinkIntent`, and Space does not (ADR-0079, amended for links). A
  visible focus draws the focus ring just outside each line of the link's glyphs, and
  assistive technology can focus a link node. A link that `max_lines` or an ellipsis cut takes
  no focus. A link keeps its focus, a press in flight and its accessible node across rebuilds
  that add or remove links before it. `RenderParagraph::{set_link_focus, link_bounds}` and
  `TextLinkFocus` carry the focus to the paragraph, and `ShownLinksListener`
  (`RenderParagraph::{with_shown_links_listener, set_shown_links_listener}`) reports which links
  a layout shows.
- **`HitTestContext::set_self_hit_pointer_target` and `set_self_hit_cursor`**: a box's
  `hit_test` routes the hit and picks its cursor for the part under the pointer, in place of
  the node-wide `pointer_target` and `mouse_cursor`; `HitTestOutcome` carries both.
- **`TextLinkRoute`**, `TextLinkKey` and `RenderParagraph::{with_link_routes, set_link_routes,
  link_routes}`: the lane identities a paragraph's links are reached through, and the identity
  that keys each link's accessible node.

### Changed

- `TextSpan::mouse_cursor` and `TextSpan::with_mouse_cursor` take `CursorIcon`, the type the
  platform cursor output takes; a span's cursor now applies over its glyphs and its children's.
- `FocusRingStyle` is defined in `flui-objects` (still re-exported as
  `flui_widgets::FocusRingStyle`) and gains `outer_band` and `inner_band`, the widths that
  paint.

### Removed

- `TextSpan::with_on_tap`, `TextSpan::on_tap` and `TextSpan::is_interactive`: a link is data on
  the span and `RichText::on_link` handles it.
- `flui_painting::typography::MouseCursor`: use `CursorIcon`.
