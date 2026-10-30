### Added

- On iPadOS a connected trackpad or mouse scrolls with phases, pinches, rotates and hovers through
  the shared pointer events, and its secondary button reaches the app as `PointerButton::Secondary`.
  `flui build` writes `UIApplicationSupportsIndirectInputEvents` into the app's Info.plist so UIKit
  delivers the pointer as a cursor rather than as touches.
