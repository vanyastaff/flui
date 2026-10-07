### Fixed

- `ScaleGestureRecognizer` joins the gesture arena for every contact, starts once it has won an arena with two contacts down (including a lone member's default win before the second finger lands), keeps scale and rotation continuous when a contact is added or lifted, wraps rotation across ±π using the two earliest contacts, and never publishes a non-finite scale, axis scale, rotation or focal point (a zero baseline span holds the factor and re-measures from the first non-zero span).
- `ForcePressGestureRecognizer` no longer starts on sensor-less devices: the W3C `0.5` that mice and force-less touch report while pressed is not treated as pressure. A force press claims the arena and fires `on_start` only after acceptance; a press that never started withdraws on up so a competing tap can win.
- `TapAndDragGestureRecognizer` works in the binding's shared arena: a tap fires once the arena accepts it, a drag claims the arena before `on_drag_start`, and a sequence that loses after `on_tap_down` or mid-drag fires `on_cancel`.
- `EagerGestureRecognizer` forgets its contact on pointer up or cancel instead of reporting a finished pointer as primary.
- Scale, force-press and tap-and-drag callbacks run with no borrow or lock held, so a callback can dispose its recognizer; a panicking callback leaves the recognizer ready for the next gesture.

### Changed

- `TapDragDownDetails`, `TapDragUpDetails`, `TapDragStartDetails`, `TapDragUpdateDetails` and `TapDragEndDetails` carry `consecutive_tap_count` (1 for an isolated tap, 2 for a double click, 3 for a triple click), counted within the settings' double-tap timeout and slop.
- `ForcePressGestureRecognizer::with_start_pressure`/`with_peak_pressure` update the thresholds shared by every handle instead of forking the recognizer, and ignore non-finite values.
