### Fixed

- Winit display discovery selects the primary monitor by handle identity, so monitors of the same model are not all marked primary. Discovery maps the upstream iterator directly without a second intermediate monitor vector.
