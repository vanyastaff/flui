### Added

- `AppHandler::opened(&OwnerPlatform, Opened)`: the report of the URLs and files the operating
  system opened the application with, queued in order with the other reports. `flui_platform::Opened`
  carries them with their `OpenTarget` (a window or the application) and `Launch` (`Cold` or
  `Running`); `Opened::new` drops items past `MAX_OPENED_ITEMS` and URLs past
  `MAX_OPENED_URL_LEN` bytes. The headless host reports one through `HeadlessHostEvents::open`.

### Changed

- Every `AppHandler` implements `opened`, a required method like the others.
