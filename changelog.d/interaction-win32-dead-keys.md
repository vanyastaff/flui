### Fixed

- Win32: a dead key (an accent on an international layout) is reported as `NamedKey::Dead` instead of its unshifted character, so shortcuts bound to that character no longer fire while composing an accented letter.
