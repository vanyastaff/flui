### Added

- TalkBack on Android: every Activity window answers it through `accesskit_android`'s
  low-level adapter, which `FluiView`'s node provider forwards to on the Java main thread.
  Type-checked; no TalkBack run yet, and explore by touch is not routed.
