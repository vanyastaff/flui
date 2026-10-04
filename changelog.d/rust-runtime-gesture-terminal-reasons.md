### Changed

- DragEndDetails and InteractionEndDetails report GestureEndReason::Completed or Cancelled. Accepted pointer cancellation continues to deliver an end callback; rejection before acceptance still delivers the cancel callback. The reason is exposed through the facade, widgets and package SDK.

### Fixed

- Cancelled scroll drags recover without release inertia, and cancelled threshold pulls do not start refreshes. Cancelled dismissal drags restore their original location, and cancelled back swipes preserve the current route. A subsequent completed gesture retains its normal behavior.
