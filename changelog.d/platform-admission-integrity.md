### Changed

- A window carries out only what its own published capabilities honour. `WindowCapabilities` is
  read-only: its fields read as before (`capabilities.material`, through `Deref` to
  `CapabilityParts`), and writing one no longer compiles. A backend or test that names
  capabilities builds `CapabilityParts` (start from `WindowCapabilities::NONE.into_parts()`, set
  fields, then `into_capabilities()`). Capabilities taken apart, edited (`unavailable` included) or
  built by hand belong to no window: a command admitted against them answers
  `ApplyError::Superseded` on every window, where before a command admitted against an edited copy
  of a window's capabilities was carried out, or answered `Ok` without effect.
- A host publishes a window's first capabilities with `WindowCapabilities::publish`, and
  `WindowState::successor` issues a new publication whenever the capabilities change, so a
  successor a caller computes from a window's snapshot opens no command on the window.
  `WindowRevisionExhausted` also answers a capability change once the process has no publication
  left to issue.
- A window double that opens commands against `WindowCapabilities::NONE` or other capabilities
  built by hand now answers `Superseded` for every command; a double that should carry commands
  out publishes its capabilities once and opens against those.
- The Android and iOS windows and the headless windows answer `ApplyError::Platform` for a command
  they do not carry out, never `Ok`.

### Removed

- `CapabilitiesOwner` and `WindowCapabilities::owned_by`: a host publishes with
  `WindowCapabilities::publish`, and nothing outside the contract names a window's publication.
