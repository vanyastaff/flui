### Fixed

- **`flui-engine`**: a partial frame under an antialiased clip resolved the clip's coverage over
  the whole clip, not over its damage, so it cost as much as a full frame and a window-sized clip
  whose mask was over the frame's work limit was refused on every frame, blinking caret included.
  A clip mask is now cut to the pixels the draws that read it can write
  ([ADR-0102](/docs/adr/ADR-0102-immutable-geometric-clip-composition.md)).
- **`flui-engine`**: an analytic shadow ignored a partial frame's damage, so it blended its
  penumbra a second time over the retained pixels outside the damage, darkening them frame by
  frame. Shadows now draw under their recorded scissor like every other draw.
