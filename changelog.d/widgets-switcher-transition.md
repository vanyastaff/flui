### Added

- **`flui-widgets`**: `AnimatedSwitcher::transition(Transition)` shows each entry through a
  reversible `Transition`: the incoming child plays the entry, the outgoing one the exit, and
  the transition's duration and curve, where set, replace the switcher's. The custom
  `transition_builder` path is unchanged; its `animation` is now one handle for the entry's
  whole life.

### Changed

- **`flui-widgets`**: a child that matches a still-outgoing `AnimatedSwitcher` entry (A → B → A
  faster than the run) takes that entry back, reversing its exit from the progress it reached
  and keeping its element and state, instead of starting a fresh entry beside it.
- **`flui-widgets`**: `AnimatedSwitcher` and its builder aliases and state moved from the
  `animated` module to `motion`; the crate-root re-exports are unchanged.
