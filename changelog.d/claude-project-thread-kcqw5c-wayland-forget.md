### Fixed

- The Wayland host's drag no longer leaks a caught panic: when a waker panics after an earlier
  failure, its payload is destroyed under containment instead of forgotten, and a failure nobody
  resumes is discarded the same way (ADR-0281).

### Changed

- `clippy::mem_forget` is on for the workspace: a `mem::forget` carries
  `#[expect(clippy::mem_forget, reason = "…")]` at the call.
