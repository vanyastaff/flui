### Added

- GestureDetector exposes scale start, update, end, and cancellation callbacks through one persistent recognizer for contact and native trackpad input.

### Changed

- Native pan/zoom claim callbacks receive both target-local and original root-space events; accepted native sessions keep their claimant through terminal delivery.
