### Added

- Android: a node's semantics identifier (`Semantics::identifier`, ADR-0240) is published as the
  view's `viewIdResourceName`, so UI Automator, Espresso and Appium find it by resource name, as
  UI Automation, AppKit, AT-SPI and UIKit find it by their identifiers. It goes through a change
  to the vendored `accesskit_android` (`third_party/`); no device run yet.
