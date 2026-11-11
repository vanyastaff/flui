### Fixed

- An Android drop no longer copies every item's text, HTML and URIs into Rust when it lands, with
  no size limit: `FluiView` holds the `ClipData` and its URI permissions until the target
  concludes the drop, and each request reads one representation from it under the request's
  `TransferLimits`, its length counted before anything is copied. At most four content URIs are
  read at once; a read whose drop was concluded answers `SourceGone` and what it reads late is
  discarded. A refused drop, and every held drop when the view leaves the window, are let go of
  at once.
- An Android read of a dropped URI's bytes gets five seconds, as on Wayland and X11: past them it
  answers `TransferError::TimedOut`, and the view cancels the provider's open and closes the
  stream. At most 64 requests wait for one of the four reads; a request past them is refused
  `TransferError::Busy`, and a dropped request gives its place in the queue back at once.

### Added

- `TransferError::Busy` (a transport refused a request because as many as it holds already wait)
  and `TransferError::TimedOut` (a delivery took longer than the transport gives one).

### Changed

- A Wayland or X11 drop fetch that passes its five seconds answers `TransferError::TimedOut`
  instead of `SourceGone`.
