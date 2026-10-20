### Fixed

- Copy, cut, paste and select-all shortcuts (and every other `SingleActivator` chord with Control or Meta) now fire on layouts without Latin letters, such as Russian: the chord matches by the physical key when the layout produced a non-ASCII character.
- An input method that was refused a rect (`NoLayout`) during an edit session is told when layout exists, even if the caret rect did not move, so its candidate window repositions.
- The focused field's text store no longer clones and compares the controller every frame; it diffs only after the controller changed.
