### Changed

- **A gap is named by its heading** (`flui-native`, `testing` feature): `testing::gap::GapId`
  replaces `testing::clipboard::KnownGap { number }`, and `expect_fail` moves to `testing::gap`.
  A `GapId` is the anchor of its entry's heading in `docs/gaps.md`, so reordering the entries no
  longer breaks a scenario that fails as one. The entries of `docs/gaps.md` lose their numbers and
  the "At a glance" table: link an entry by its heading's anchor (`gaps.md#no-system-back`).
