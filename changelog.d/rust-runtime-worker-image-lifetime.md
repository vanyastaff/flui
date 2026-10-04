### Fixed

- Keep admitted hot-reload worker images mapped until the development host exits, allowing escaped views, callbacks and plugin-defined destructors to survive worker replacement. Restart the host to reclaim old images and locked staged files.
