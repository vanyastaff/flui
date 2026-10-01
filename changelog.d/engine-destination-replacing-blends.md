### Fixed

- **Save layers and opacity layers honour their Porter-Duff blend mode** (`flui-engine`): a
  layer composites its whole region with the mode it records, the pixels its content left
  transparent included, instead of compositing `SrcOver`. A `Src` layer now makes its region
  exactly its content, a `Clear` layer clears it even when empty, and an opaque `DstOver`
  opacity layer stays under the backdrop. An unbounded layer's region is its clip.
- **A bounded save layer maps its bounds through the transform** (`flui-engine`): the bounds
  passed to `Canvas::save_layer` are in local space, as the damage producer already assumed. A
  translucent layer under a translation or scale composited over the unmapped rectangle and lost
  its content outside it.
- **Shader-mask and backdrop-filter composites respect ancestor clips** (`flui-engine`): they
  composite under the clip rect, rounded clip and partial-frame damage in force where they were
  recorded, so a mask no longer paints past its clip, and an unchanged translucent mask no longer
  blends a second time outside a partial frame's damage.
