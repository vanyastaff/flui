### Added

- The app's development agent answers every gesture (`element.tap`, `double_tap`, `long_press`,
  `drag_to`, `swipe`, `fling`, `pinch`, `type_text`, `press_keys`, `scroll_into_view`,
  `scroll_until`) on the synthetic and semantic paths: it holds the act at frame boundaries until
  the target is attached, visible, stable over two frames, enabled and receives the hit, or the
  act's timeout passes, then injects at that frame's bounds; a refusal is `not_actionable` naming
  the failed check, and nothing is sent after an act's answer. `scroll_until` scrolls a viewport at
  a time until the element shows under the scrollable, and ends `not_found` once the content stops
  moving. A window holds at most 64 acts (`busy` past it).
- `flui_protocol::input::gestures`: the one composition of each gesture into an action sequence,
  which the agent and the driver's OS lane both play; and `Plan::tick_ends`, where each tick of a
  plan ends.

### Changed

- The driver sends a gesture to the app's agent from its `Window`, `Element` and `ElementLocator`
  handles too, whenever the agent serves it and the path is not `os`.
- A gesture's reply deadline (`Addressed::holds_for`) counts the time its input takes to play after
  the act's timeout, so a long press, a slow swipe or a double tap is no longer cut short.
- `input.inject` plays `flui_protocol::input::plan`'s timeline: its bounds and refusals are the
  plan's, each step of a move is one event stamped at its planned instant, and a move that names no
  duration lasts a step (16 ms).
- A composed gesture's pointer and keyboard sources are named `gesture.mouse`, `gesture.touch`,
  `gesture.pinch.a`/`gesture.pinch.b` and `gesture.keys`, so a mouse gesture after a touch gesture on
  one input lease no longer collides on one source name.
