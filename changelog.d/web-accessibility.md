### Added

- Web: the host has accessibility. Every canvas window mirrors its semantics tree as a parallel
  ARIA DOM over the canvas, which a screen reader turns on through a placeholder button only it
  finds. Clicks and focus on the mirrored elements reach the application as AccessKit actions,
  keys on them reach the window, and the semantics identifier is the element's `data-flui-id`.
