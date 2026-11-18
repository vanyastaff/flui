### Changed

- The first window's or offscreen target's render pipelines are compiled on a worker thread from the moment it is attached, so a native application's first frame no longer waits for the whole pipeline build after its first layout (ADR-0305 §1).
