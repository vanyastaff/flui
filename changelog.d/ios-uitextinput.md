### Added

- **`flui-native`**: text input on iOS. A focused field hands the system keyboard to an invisible
  `UITextInput` responder, and its edits reach the field as composition and commits, as AppKit's
  do; Japanese and Chinese input methods compose through it
  ([ADR-0255](/docs/adr/ADR-0255-mobile-and-web-hosts.md)).
