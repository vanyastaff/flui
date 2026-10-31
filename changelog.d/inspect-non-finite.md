### Changed

- **Infinity and NaN in an inspected tree are values, not gaps** (`flui-protocol` 0.8,
  `flui-foundation`, `flui-objects`): `InspectValue::NonFinite(NonFinite)` carries a float that is
  `pos_inf`, `neg_inf` or `nan` as `{"type": "non_finite", "value": "pos_inf"}` instead of `null`
  or a missing field, so an agent tells an unbounded maximum from a field that is not there and
  sees a NaN as the bug it usually is. A size, offset, rectangle, inset or matrix holding one is
  its `Debug` text. `Fields` no longer rewrites a non-finite number to an absent value, and
  constraints, `ConstrainedBox`, `LayoutBuilder`, overflow boxes and `CustomPaint` describe their
  unbounded sides as infinity.
