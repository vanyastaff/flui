### Added
- `impl_render_view!(Type, site = field)` implements `View::creation_site` from a `CreationSite` field, and `flui_view` re-exports `CreationSite` (also in its prelude). `Padding` records the caller of its constructors, so the inspector shows where it was written.
