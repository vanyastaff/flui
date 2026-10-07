### Removed

- Removed the unused `EventRouter`, `CustomHitTestable`, `HitTestable`, and `HitTestTarget` APIs and their sealed marker module. Presentation-owned `GestureBinding` and `InteractionLane` continue routing pointers through render-protocol hit tests; `FocusManager` owns keyboard dispatch.
