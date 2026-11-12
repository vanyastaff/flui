### Changed

- **A picture holds one render object's output** (`flui-rendering`): the paint composer seals the
  open picture whenever the render object painting the next run changes, so inline siblings and a
  parent painting around its children land in separate `PictureLayer`s (ADR-0304 §2). A frame
  carries more picture layers; its pixels are unchanged.
