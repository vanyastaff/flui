### Fixed

- A repaint boundary that holds a `LeaderLayer` (a selection handle's anchor, an overlay's target) is retained and replayed like any other instead of repainting on every frame. Only a boundary that holds a `FollowerLayer` is still refused a capture, because a capture does not carry its hit-test correlation.
