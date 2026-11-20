### Changed

- `flui-driver`'s Windows lane sends Control+Shift, Alt+Shift and Shift taps, a Shift-click
  included, and refuses only the presses the shell takes (the Windows key, Alt+Tab,
  Ctrl+Shift+Esc, a lock key): a release that would switch the input language or turn on Sticky
  Keys is preceded by a press and release of the unassigned virtual key `0xE8`.
- `flui_driver::Error::Gone` carries `effect`, read from the wire's error object; an element that
  went away while the Windows lane invoked it reports `may_have_run`.

### Fixed

- A lane's worker that ends, or a lane job that panics, after a failed release releases once
  more, and logs at error level what still stays held; the Windows lane's error names the keys.
