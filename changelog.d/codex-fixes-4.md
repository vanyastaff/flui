### Fixed

- **A panicking drag payload stranded a web drag** (`flui-platform`): when a lazy payload
  provider panicked during the browser's `dragstart`, the session never ended and its source was
  never told. The panic is now contained and reported, the session ends cancelled and the
  browser's drag is refused.
- **Dropped images arrived as raw bytes on the web** (`flui-platform`): a dropped `image/*` file,
  or an image a page stored as a string, is announced as `TransferFormat::Image` and now resolves
  as `TransferPayload::Image` instead of `TransferPayload::Custom`.
