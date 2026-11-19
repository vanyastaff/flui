### Changed

- **`flui-view`**, **`flui-widgets`**: an element keeps its first two child ids inline, so a leaf,
  a single-child wrapper and a pair allocate no child list, and a control's press group allocates
  its recognizers' set at the first contact rather than holding it inline at mount. A `RawButton`
  over a `Text` mounts with 31 allocations and 4269 bytes, down from 33 and 4349
  (`perf_raw_button_over_text`).
