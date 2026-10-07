### Fixed
- Drag, multi-drag, scale and tap-and-drag recognizers measure release velocity on the pointer event timeline, so a stationary pause before release suppresses the fling even when events are dispatched together.
