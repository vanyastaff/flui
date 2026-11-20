### Added

- **`flui-app`**: a page's debug build journals its backend for the driver agent's `native`
  recording layer, as a native host's does: its hello lists `native`, and a trace through the
  Chrome lane holds the page's lifecycle notices, input method events, reads of the preferences
  and the displays, and window snapshots
  ([ADR-0390](/docs/adr/ADR-0390-a-page-journals-its-native-layer-as-a-native-host-does.md)).
- **`flui-testing`**: the backend conformance table's recording rows (`RowGroup::Recordings`): a
  backend delivers a composition, a lifecycle, preference reads and display reads with its
  native journal capturing them, and what the journal recorded replays on the headless host to
  what the application heard; a backend whose delivery cannot be driven fails the rows as
  `FailureKind::NotDriven`.
- **`flui-driver`**: `testing::ScriptedBrowser::start_with`, a stand-in page whose agent's hello
  lists the capabilities given, the layers it records among them.

### Changed

- **`flui-testing`**: `BackendUnderTest` requires `replay_recording`, with no default body: a
  backend drives a session through its delivery with a journal capture installed and answers
  what it delivered and what its recording replayed on the headless host, or `NotDriven`.

### Fixed

- **`flui-app`**: a debug build's runner ends its native journal when its platform fails to
  start or another instance takes the launch, as it does when no driver agent is served; the
  iOS and desktop runners left the capture installed.
