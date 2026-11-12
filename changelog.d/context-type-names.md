### Changed

- A context type is named `…Context` (ADR-0325): `EventCx` is `EventContext`, `PaintCx` is
  `PaintContext`, `TextCx` is `TextContextMut`, `ServeCx` is `ServeContext`, `QueryPoisonCx` is
  `QueryPoisonContext`; the render protocol's lower contexts are `RawBoxLayoutContext`,
  `RawSliverLayoutContext`, `RawBoxHitTestContext`, `RawSliverHitTestContext`, and
  `BoxIntrinsicsCtx`, `BoxDryLayoutCtx`, `BoxDryBaselineCtx` take the `Context` suffix.
