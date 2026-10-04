### Fixed

- Report browser display bounds in device pixels while retaining logical CSS pointer coordinates.
- Preserve held browser back/forward buttons during pointer and wheel translation.
- Invoke browser platform callbacks and retire replaced callbacks outside platform state locks; explicit quit consumes its callback once.
