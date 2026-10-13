### Added

- **Heading levels** (`flui-semantics`, `flui-widgets`): `HeadingLevel` (`H1`–`H6`),
  `SemanticsConfiguration::set_heading_level` and `Semantics::heading_level` give a header its
  outline level, which reaches the platform as AccessKit's level. `A11yNode::heading_level` in
  `flui-testing` reads it back one-based.

### Fixed

- **Headers announce as headings** (`flui-semantics`): a header node published AccessKit's
  `Header` role, HTML's `<header>` landmark, which UI Automation reports as a group with the
  `banner` landmark type, so screen readers never offered it to heading navigation. It now
  publishes `Heading`.
- **Bar titles are headings** (`flui-material`, `flui-cupertino`): `AppBar`'s title and
  `CupertinoNavigationBar`'s middle are announced as level-one headings.
