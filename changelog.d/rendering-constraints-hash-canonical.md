### Added

- `flui_foundation::geometry::canonical_bits` (`f32`) and `canonical_bits_f64`: the bits a hash stores for a float, with `-0.0` folded into `+0.0` and every NaN into one NaN.

### Fixed

- Values that compare equal now hash equal when they differ only in the sign of zero. This covers `BoxConstraints`, `SliverConstraints`, `SliverGeometry`, and the box and sliver parent data. Before, `BoxConstraints` with a bound of `0.0` and one of `-0.0` compared equal but hashed apart, so a hash set or map could miss or duplicate them.
