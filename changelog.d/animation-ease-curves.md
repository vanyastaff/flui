### Changed

- **Elastic and bounce curves take an `Ease`** (`flui-animation`): `ElasticInCurve`,
  `ElasticOutCurve` and `ElasticInOutCurve` become `Elastic::new(Ease::In | Ease::Out |
  Ease::InOut, period)`, and `BounceInCurve`, `BounceOutCurve` and `BounceInOutCurve` become
  `Bounce(Ease::…)`. `DecelerateCurve` is renamed `Decelerate` and `FlippedCurve` `Flipped`. The
  `curves::` presets keep their names and values. An `Elastic` serializes as
  `{ "ease": …, "period": … }` (a missing `ease` decodes as `Out`) and a `Bounce` as its `Ease`.
