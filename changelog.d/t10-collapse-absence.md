### Changed

- `flui_platform::CapabilityAbsence` is no longer an enum of its own: it is an opaque one-byte
  `UnsupportedReason` narrowed to the reasons a lasting capability can be absent for. Name one with
  `CapabilityAbsence::NOT_ON_THIS_PLATFORM`, `NOT_ON_THIS_DEVICE` or `NOT_YET_ON_THIS_HOST`
  (formerly the variants of the same names), read it with `reason()`, and convert a reason with
  `CapabilityAbsence::try_from(reason)`, which refuses `Unavailable`, `NotDeclared`,
  `NotRegistered` and `NoWindow`. `WindowCapabilities`, `DisplayInfo` and the `*Parts` structs
  keep their size.
