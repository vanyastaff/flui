### Added

- `Announcer`, from `LifecycleContext::announcer`, speaks a message through assistive technology
  without a visible change: `announcer.announce("Message sent", Urgency::Polite)`. Messages are
  published as the value of one of two live-region nodes the presentation keeps under its root,
  and an idle presentation produces the frame that publishes them.
- `Semantics::live_atomic` and `Semantics::busy` (`aria-atomic`, `aria-busy`); an `Alert` or
  `Status` is atomic unless declared otherwise.

### Changed

- `Semantics::live_region` and `SemanticsConfiguration::set_live_region` take a `LiveSetting`
  (`Off`, `Polite`, `Assertive`); `Off` keeps a region silent inside an enclosing one.

### Removed

- `Assertiveness`, replaced by `LiveSetting` for live regions and `Urgency` for announcements.
- `SemanticsEvent` and `SemanticsHost`'s announce, event and tooltip callbacks, which no
  platform delivered.
