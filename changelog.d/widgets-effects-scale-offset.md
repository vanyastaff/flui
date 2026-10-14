### Fixed

- **`flui-widgets`**: an `Animate` scale about a non-centre alignment keeps its alignment when an
  intermediate offset overflows `f64`, and a rebuild that leaves nothing to play (an empty
  `Transition` or `Effects`) settles the run in flight instead of driving frames.
