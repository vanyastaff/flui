### Added

- **`flui-interaction`**: `DropReceiver::autoscroll` and `Autoscroll`. While a drag hovers, each
  `DragOwner::tick` asks the receivers under it innermost first for one autoscroll frame, until
  one scrolls; `DragView::pointer_kind` names the dragging pointer
  ([ADR-0211](/docs/adr/ADR-0211-unified-drag-and-drop.md) §8).
- **`flui-widgets`**: a drag hovering near a `Scrollable`'s edge scrolls it toward that edge as
  `ScrollCause::SideEffect` steps through the nested scroll channel.

### Fixed

- **`flui-interaction`**: a drop receiver whose widget unregistered it, or replaced it under the
  same ticket, mid-hover now gets its one leave and never spring-loads or takes the drop; a
  replacement receiver enters before it moves.
- **`flui-interaction`**: spring-loading of an OS drag, whose hover carries no event time, now
  comes due; before, it waited for an unrelated turn.
- **`flui-interaction`**: an OS offer announced twice over one window stays one hover and answers
  from the latest permitted actions.
