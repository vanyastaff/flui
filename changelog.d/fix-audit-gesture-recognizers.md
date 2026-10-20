### Changed

- **`PrimaryContact::finish` takes a `ContactEnd`** (`flui-interaction`, `flui-sdk`): a custom
  recognizer says whether its lifted contact stays in the contest for the Up sweep
  (`ContactEnd::Contending`) or leaves it because it recognized nothing (`ContactEnd::Withdrawn`).
- **A tap shows its press before release** (`flui-interaction`): `on_tap_down` runs when the
  arena gives the press to the tap or 100 ms after Down while it is still undecided, whichever
  comes first, instead of together with `on_tap_up` at release.

### Fixed

- **An inner long press, force press or swipe silenced the outer tap** (`flui-interaction`): a
  recognizer that recognized nothing by its release (a long press released early, a force press
  without its start pressure, a swipe that never crossed its slop) leaves the contest instead of
  winning the Up sweep by its route position.
- **Slops scaled with zoom** (`flui-interaction`): every recognizer measures its tap, hold, pan
  and double-tap tolerances in root pixels, so taps survive finger jitter and lists scroll inside
  a zoomed `InteractiveViewer`.
- **A staggered two-finger release lost the fling** (`flui-interaction`): the scale's focal
  velocity carries across contact changes instead of resetting on each lift.
- **A drag started on a successor finger that never moved** (`flui-interaction`): with
  `DragPointerStrategy::ContinueWithRemaining`, the next finger starts the drag only past its own
  slop.
- **Axial drags flung on their off-axis speed** (`flui-interaction`, `flui-widgets`): the fling
  threshold and cap read a horizontal or vertical drag's own component, so a page view or the
  back gesture settles instead of flipping on a mostly perpendicular release.
- **A long press released past its timeout before the frame polled it was lost**
  (`flui-interaction`): the release fires the long press first.
- **A bounced second click produced two taps in reverse order** (`flui-interaction`): the bounce
  holds its own contact, so only the first tap fires, once.
- **Scale factors jumped by orders of magnitude** (`flui-interaction`): a baseline span within
  the span slop (fingers in one column, coincident Downs) is re-measured past the slop instead of
  divided by.
- **A trackpad pan railed on its first stray pixel** (`flui-interaction`): under
  `AxisLock::Rails` the rail is picked once the pan crosses its slop, as for touch.
- **The iOS and macOS velocity estimators under-reported batched moves** (`flui-interaction`):
  samples sharing a timestamp count as one instant.
- **A two-finger tap fired for fingers that never overlapped** (`flui-interaction`): a
  multi-contact tap needs its contacts down together.
