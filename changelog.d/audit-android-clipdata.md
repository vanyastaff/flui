### Fixed

- An Android drop no longer copies every item's text, HTML and URIs into Rust when it lands, with
  no size limit: `FluiView` holds the `ClipData` and its URI permissions until the target
  concludes the drop, and each request reads one representation from it under the request's
  `TransferLimits`, its length counted before anything is copied. At most four content URIs are
  read at once; a read whose drop was concluded answers `SourceGone` and what it reads late is
  discarded. A refused drop, and every held drop when the view leaves the window, are let go of
  at once.
