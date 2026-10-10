### Changed

- `Animation::is_animating` is required: owners report installed runs, combinators forward their source's answer and fixed values return `false`. Directional status no longer supplies a default answer.

### Fixed

- `ConstantAnimation` and combinators over it report no active run for every status, including `Forward` and `Reverse`.
