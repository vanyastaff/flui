### Changed

- The focused field's text store builds the committed text of a session only when the session changed the text or the composition, and the Windows text services read only as many units as the caller's buffer holds.
