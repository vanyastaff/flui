### Fixed

- flui-native (iOS): a window handler that panics while its scene backgrounds, foregrounds or disconnects no longer cuts the change short. The surface is still released before UIKit takes the layer, the rest of the queued lifecycle still applies, and the scene consumer still hears the disconnection.
