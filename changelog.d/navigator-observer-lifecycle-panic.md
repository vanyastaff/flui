### Fixed

- Navigator attachment and detachment now notify every registered observer when one lifecycle callback panics, then resume the first panic. A failing observer no longer leaves later observers without their corresponding `did_attach` or `did_detach` notification.
