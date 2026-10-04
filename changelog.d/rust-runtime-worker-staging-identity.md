### Fixed

- Address staged hot-reload workers with SHA-256 so practical collisions cannot serve an earlier build. Verify existing staging contents and refuse corrupt copies without overwriting a potentially loaded image.
