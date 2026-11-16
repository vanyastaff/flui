### Added

- `UnsupportedReason::Withdrawn` and `CapabilityAbsence::WITHDRAWN`: the lasting absence of a
  command a window withdrew because it can publish no more of what it honours.
- Agent protocol 0.15: a recorded window capability can be absent as `withdrawn`. A client older
  than 0.15 is refused, as it would refuse the unknown value.
- `PublicationMint` (`process`), `WindowCapabilities::publish_from` and
  `WindowState::successor_from`: a host names the mint its publications come from; production
  passes the process's. Behind the `testing` feature, `PublicationMint::exhausted` issues nothing
  and `WindowRevision::from_raw` starts a test at any revision.

### Fixed

- A withdrawal no longer takes away a window's reports and per-frame verbs (frame-rate hint,
  presentation report, placement and observations), which hosts keep serving.
- No revision names two snapshots: a window freezes on the snapshot that takes its last revision,
  with its capabilities withdrawn, instead of announcing the withdrawal again at that revision.
