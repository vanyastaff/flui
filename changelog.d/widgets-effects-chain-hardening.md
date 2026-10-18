### Fixed

- **`flui-widgets`**: `Animate` effects interpolate endpoint-exactly, multiply opacities and
  scales without intermediate overflow, leave a non-finite effect out instead of degenerating
  the pose, repaint when a caller's controller switches a transition's side at an unchanged
  value, and an empty `Transition` no longer drives frames.
