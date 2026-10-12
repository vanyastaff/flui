### Changed

- Animation value conversion and ownership use crate-root exports:
  `flui_animation::{AnimationVector, TwoWayConverter, AnimatedValue, AnimatedValueView}`.
  Their implementation modules and motion/registry update helpers are private;
  the former `spring` module is removed.
