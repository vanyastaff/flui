### Fixed

- **Save layers and opacity layers honour their Porter-Duff blend mode** (`flui-engine`): a
  layer composites its whole region with the mode it records, the pixels its content left
  transparent included, instead of compositing `SrcOver`. A `Src` layer now makes its region
  exactly its content, a `Clear` layer clears it even when empty, and an opaque `DstOver`
  opacity layer stays under the backdrop. An unbounded layer's region is its clip (ADR-0099).
- **A bounded save layer maps its bounds through the transform** (`flui-engine`): the bounds
  passed to `Canvas::save_layer` are in local space, as the damage producer already assumed. A
  translucent layer under a translation or scale composited over the unmapped rectangle and lost
  its content outside it.
- **Shader-mask and backdrop-filter composites respect ancestor clips** (`flui-engine`): they
  composite under the clip rect, rounded clip and partial-frame damage in force where they were
  recorded, at the top level and inside an opacity layer, so a mask no longer paints past its
  clip, and an unchanged translucent mask no longer blends a second time outside a partial
  frame's damage.
- **A shader mask composites its result `SrcOver`** (`flui-engine`): its blend mode no longer
  applies a second time between the masked child and the backdrop, which multiplied the child by
  the backdrop under the default `Modulate`. The mask pass still applies the shader's alpha
  alone, whatever the mode (ADR-0099).
- **A rotated or skewed save layer keeps its content inside its bounds** (`flui-engine`): the
  composite carries the bounds as a hard clip for every blend mode, the advanced ones included,
  so content drawn past them no longer shows in the corners of their bounding box.
