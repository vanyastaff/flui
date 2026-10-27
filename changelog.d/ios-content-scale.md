### Fixed
- An iOS app renders at the screen's scale: the content view took UIKit's default content scale of 1.0, so a 3x iPhone drew a third-resolution frame stretched to fit and `MediaQuery` reported a device pixel ratio of 1.
