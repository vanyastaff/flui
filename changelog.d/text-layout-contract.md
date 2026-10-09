### Changed

- Text shaping, render-object layout, layout delegates and intrinsic, dry-layout
  and baseline queries now return typed errors that callers must handle.
- Viewport offsets provide detached layout proposals; custom offset
  implementations must publish them only after successful measurement.

### Fixed

- Reject unrepresentable text requests and nonfinite shaping results without
  publishing stale or fabricated text geometry.
- Retain rejected layout work for changed-input recovery without continuously
  requesting frames for unchanged invalid authored text.
- Preserve accepted viewport layout metrics and fractional-page resize mapping
  when descendant text measurement fails.
- Refuse stale viewport layout after reentrant scroll input and retry with the
  accepted position without poisoning the render object.
