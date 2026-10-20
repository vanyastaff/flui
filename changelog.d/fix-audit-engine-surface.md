### Fixed

- `flui-engine`: a surface that stays `Lost` after a reconfigure is now recreated on the same
  device instead of being reconfigured on every frame while the window stays blank; a recreation
  that fails is retried before the next acquire rather than leaving the window released, and one
  that can never succeed reports its own error instead of a retryable `SurfaceLost`.
- `flui-engine`: a frame presented through a `Suboptimal` acquisition now reconfigures the surface
  before the next acquire, once per suboptimal episode, instead of presenting mismatched until the
  next resize.
- `flui-engine`: a frame whose acquisition reconfigured the surface is planned again, so a partial
  frame no longer fails on the retained target the reconfigure invalidated.
