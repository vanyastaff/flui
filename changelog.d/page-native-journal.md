### Added

- **`flui-app`**: a page's debug build journals its backend for the driver agent's `native`
  recording layer, as a native host's does: its hello lists `native`, and a trace through the
  Chrome lane holds the page's lifecycle notices, input method events, reads of the preferences
  and the displays, and window snapshots
  ([ADR-0390](/docs/adr/ADR-0390-a-page-journals-its-native-layer-as-a-native-host-does.md)).
- **`flui-testing`**: the backend conformance table's recording rows (`RowGroup::Recordings`): a
  backend's recording of a composition, a lifecycle, preference reads and display reads replays
  on the headless host to what the application heard.
- **`flui-driver`**: `testing::ScriptedBrowser::start_with`, a stand-in page whose agent's hello
  lists the capabilities given, the layers it records among them.

### Changed

- **`flui-testing`**: `BackendUnderTest` requires `replay_recording`, with no default body: a
  backend writes a session as its native journal records it and answers what its replay on the
  headless host delivered, or says its journal records nothing of the kind.
