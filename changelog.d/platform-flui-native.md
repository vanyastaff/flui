### Changed

- The core host crate `flui-platform` is now `flui-native`
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md)). The contract crate keeps
  the name `flui-platform-api` until it takes `flui-platform`; a branch written before this rename
  replaces `flui-platform` with `flui-native` and `flui_platform` with `flui_native` everywhere
  except in the contract's name.
