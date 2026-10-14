### Added

- **`flui-widgets`**: a `motion` module for composed motion on any view. `Effect` (fade,
  slide, scale about an alignment, rotate) is the shared vocabulary; `Transition` describes
  entry and exit with one effect per slot, merges with `+` (the right operand wins a slot),
  offers `Transition::asymmetric`, and reverses an interrupted exit from the progress it
  reached; `Effects` is an ordered timeline whose entries inherit delay, duration and curve,
  with `then()` sequencing and `stagger(..)` offsetting siblings by `Stagger::delay`.
  `Animate::new(effects, child)`, `Animate::transition(..)` and `view.animate(effects)`
  (`AnimateExt`, in the prelude) play them, started by `Trigger::OnMount`, `OnChange`,
  `Target` or `Controller`; reduced motion paints the end state on the first frame.
