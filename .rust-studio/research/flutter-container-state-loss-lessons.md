# Flutter Container state-loss lesson

Date: 2026-09-14

Question:

- Do Flutter issue comments around `Container` state loss expose an
  architectural problem FLUI should avoid rather than inherit?

Answer:

- Yes. The problem is not only Flutter's `Container`: optional convenience
  widget layers can make a user's child move through a different Element
  topology, causing unkeyed state loss. FLUI's `Container` currently mirrors
  that conditional topology and reproduces the issue.

External signal:

- flutter/flutter#161698, open P2 framework issue: `Container` can lose its
  child's state.
- The comment thread discusses several architectural directions: a universal
  solution, a compressed/intermediate Element, moving composition into a render
  object, and local deactivate maps. Maintainers also call out performance
  costs of element inflation/deflation and the resemblance to GlobalKey
  reparenting.

FLUI mapping:

- `crates/flui-widgets/src/container.rs:18-22` documents Flutter-style
  conditional composition.
- `crates/flui-widgets/src/container.rs:142-209` changes the child wrapper
  chain depending on which options are present.
- Adding `color` introduces a `ColoredBox` wrapper, moving the unkeyed child
  from the container's direct built child to a wrapper's child slot.

Verification:

- Temporary probe added to `crates/flui-widgets/tests/container.rs`:
  `Container::new().child(StateProbe)` pumped to
  `Container::new().color(...).child(StateProbe)`.
- Expected state creation count: 1.
- Observed: the probe failed with creation count 2.
- Command:
  `cargo nextest run -p flui-widgets --test widgets_it -E 'test(audit_container_optional_color_preserves_unkeyed_child_state)' --no-fail-fast`
  => 0 passed / 1 failed / 503 skipped.
- The temporary probe was removed. Control tests passed afterward:
  `cargo nextest run -p flui-widgets --test widgets_it -E 'test(container_padding_shrink_wraps_child) or test(container_color_and_padding_compose_around_child) or test(animated_container_interpolates_size_over_frames)' --no-fail-fast`
  => 3 passed / 500 skipped.

Disposition:

- Filed https://github.com/vanyastaff/flui/issues/1096:
  `widgets: preserve child state when Container optional layers change`.

Open:

- The right fix is intentionally undecided. Candidate directions include a
  render-level composed container for stable child slots, an internal
  compressed-element abstraction, or an explicit documented decision to keep
  Flutter's state-loss behavior. The issue argues this should be a deliberate
  FLUI contract, not inherited accidentally.
