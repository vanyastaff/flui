### Added

- **Event streams through the development agent** (`flui-view`, `flui-runtime`, `flui-semantics`,
  `flui-protocol`; ADR-0238): `AgentWindow::subscribe` streams a window's `frame`, `semantics`
  and `window` events to an `AgentEvents` reader, in the order the window emitted them, from the
  moment it is registered. A reader that falls behind loses events from a 256-event queue, never
  silently: the next event that fits is preceded by a `gap` with the count missed. A window admits
  64 live subscriptions and answers the next `busy`; dropping the stream ends a subscription, and
  a closing window ends every stream after a last `window` event.
- A wait on an event: `Condition::Event` holds once the window emitted an event of that kind
  after the wait began.
- Constructors for `FrameEvent`, `SemanticsEvent`, `WindowEvent` and `GapEvent`.
