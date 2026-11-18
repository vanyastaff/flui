### Added

- On iOS, a focused text field gets UIKit's selection gestures, handles and loupe: a
  `UITextInteraction` is put over the field while it has the keyboard and taken off when it loses
  it, and it begins only over the field, so touches elsewhere stay the application's.

### Fixed

- On iOS, `selectionRectsForRange:` answers one rectangle per line of the range, with its writing
  direction and which rectangles hold the range's start and end, instead of none.
