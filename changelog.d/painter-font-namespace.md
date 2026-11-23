### Fixed

- A hot-reload plugin's scene after a GPU device or surface-format rebuild, and a window's text after another window of the same GPU context drew a scene of the other font namespace, were drawn through a glyph atlas of the wrong namespace: the shared painter now owns its font namespace.
