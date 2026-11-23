### Added
- `impl_render_view!(Type, site = field)` implements `View::creation_site` from a `CreationSite` field. `Padding` records the caller of its constructors, so the inspector shows where it was written.
