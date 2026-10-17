### Added

- `ZoomIntent::{In, Out}`: zoom the enclosing `InteractiveViewer` one step
  (a factor of two) about its centre. The viewer answers it for its
  alternatives' commands, its zoom keys (`+`, `=`, `-`, alone or with the
  platform's command key) and its assistive increment and decrement.
- `InteractiveViewer` is a leaf of the nested scroll channel: a contact's pan
  is a sequence of steps the scroll chain above it shares, so a photo that
  cannot pan (not zoomed, or at its edge when the contact starts) turns the
  pager it is in, and a scrollable that took the remainder settles on release
  (a pager lands on a page, also after a cancel). Its inertia is the
  releasing sequence's side-effect motion on the axes it holds.
- `PanIntent::{Left, Right, Up, Down}`: pan the enclosing `InteractiveViewer`
  one step (53 logical pixels) toward a side — the single-pointer path of a
  pan, for the viewer's `GestureAlternatives` controls.
- `InteractiveViewer` snap-back: a contact that holds an axis pulls the scene
  past its boundary through a rubber band, and a pinch zooms past a scale
  bound with resistance (at most half an octave); released, the scene springs
  back. Inertia the boundary stops overshoots it and springs back.
- A contact on an `InteractiveViewer` focuses it, so its keys work next.
- `InteractiveViewer::double_tap_zoom(bool)` (on by default): a double tap
  performs the alternatives' first command about the tapped point, and zooms
  back out to the minimum at the maximum scale.
- `InteractiveViewer` keyboard and assistive paths: with focus in it the
  arrows pan one step (an arrow it cannot take bubbles to the enclosing
  scrollable); its semantics node publishes the scale as a numeric range that
  increment, decrement and set-value adjust, and the scroll actions pan.

### Changed

- `InteractiveViewer::new` takes the viewer's `GestureAlternatives`
  (ADR-0216): a pinch needs a single-pointer path. `InteractiveViewer` no
  longer implements `Default`. Build the alternatives with commands naming
  `ZoomIntent` and `PanIntent` values.
- Under `WheelScaleGate::CtrlWheel` a plain wheel tick or a phased trackpad
  scroll pans the viewer through the channel when it can move, instead of
  always reaching the enclosing scrollable; `pan_enabled(false)` restores the
  pass-through.
