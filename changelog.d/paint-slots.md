### Changed

- A repaint boundary's retained paint output holds its own region only, with a
  slot where each nested boundary sits. A dirty nested boundary (a blinking
  caret in a list item, say) repaints alone: the boundaries around it are
  replayed from their captures instead of re-recorded, and capture memory no
  longer grows with nesting depth. The layer tree a frame emits is unchanged.
