### Fixed

- **`flui-native`**: an image dropped on an Android window resolves as `TransferPayload::Image`,
  the format its offer announced, instead of raw custom bytes.
- **`flui-native`**: each drop on an Android window holds its own content-URI permissions until
  its target concludes it; a second drop no longer releases the first's early, and a refused drop
  gives its permissions back at once
  ([ADR-0247](/docs/adr/ADR-0247-android-java-view.md)).
- **`flui-native`**: a dropped Android content URI larger than the request's limit fails with
  `TransferError::TooLarge` and the bytes read, not `SourceGone`.
