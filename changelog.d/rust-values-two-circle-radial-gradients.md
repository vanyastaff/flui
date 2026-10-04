### Fixed

- Evaluate radial gradient focal points and initial radii in ordinary and advanced GPU paint, apply the configured tile mode, and preserve both circles when resolving box decoration gradients. Invalid or unrepresentable circle parameters return typed geometry errors; shader-mask support remains Clamp gradients without focal parameters.
