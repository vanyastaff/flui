### Fixed
- Image source cropping now reaches GPU replay from Decoration and RenderImage, including scale and aligned repeat placement.
- Image and atlas quads preserve affine transforms, optional Paint color/alpha, color filters and requested blend modes.
- Destination-replacing images under feathered clips preserve uncovered destination pixels while writing transparent covered texels.
- RenderImage `ImageFit::None` keeps natural scale and crops oversized content inside its allocated box.
