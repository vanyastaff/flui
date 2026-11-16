### Added

- `AdmissionError::Unpublished`: `WindowCommand::admit` refuses capabilities that are no
  publication (built by hand, taken apart and edited, or withdrawn). Admitting again never helps;
  admit against the window's snapshot.
- `WindowCapabilities::withdraw` and `is_withdrawn`: what a window publishes once it can publish
  no more of what it honours. Every command is absent as withdrawn (`CapabilityAbsence::WITHDRAWN`,
  the normal mode included); the reports and per-frame verbs no command reads keep their values.
  The snapshot that takes a window's last revision, and a window whose capabilities change after
  the process ran out of publications, publish withdrawn capabilities, so readers see the change;
  the window admits no command and opens no token, instead of carrying out what it may have lost.
  A window opened when the process has no publication left opens with them, and its other facts
  publish as usual.

### Changed

- A window carries out only what its own published capabilities honour (ADR-0341). Before, a
  command admitted against an edited copy of a window's capabilities was carried out, or answered
  `Ok` without effect.
- `WindowCapabilities` fields are read through accessors: `capabilities.material()` for
  `capabilities.material`. Writing a field no longer compiles. A backend or test that names
  capabilities builds `CapabilityParts` (`WindowCapabilities::NONE.into_parts()`, set fields, then
  `into_capabilities()`).
- A host publishes a window's first capabilities with `WindowCapabilities::publish`, and
  `WindowState::successor` issues a new publication whenever published capabilities change. A
  successor a caller computes from a window's snapshot opens no command on the window, and a
  successor of unpublished capabilities stays unpublished.
- A window double must publish its capabilities once and admit against those. Admitting against
  `WindowCapabilities::NONE` or other hand-built capabilities is refused `Unpublished`.
- The Android and iOS windows and the headless windows answer `ApplyError::Platform` for a command
  they do not carry out, never `Ok`.
