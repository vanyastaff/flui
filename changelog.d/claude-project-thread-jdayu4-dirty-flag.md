### Changed

- An element's dirty flag is a plain cell instead of a shared atomic: mounting an element allocates one block less. A callback from `ElementCore::create_mark_dirty_callback` only schedules the rebuild, and the build owner marks the element dirty when it drains the request; one created before mount does nothing.
