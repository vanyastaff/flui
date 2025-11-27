# Compositor Layers Specification

## ADDED Requirements

### Requirement: ShaderMaskLayer Support

The compositor SHALL support rendering shader masks that apply GPU shaders as masks to child content, enabling effects like gradient fades and vignettes.

#### Scenario: Render child with linear gradient mask

- **GIVEN** a RenderShaderMask with a linear gradient shader
- **WHEN** the paint phase executes
- **THEN** the child SHALL be rendered to an offscreen texture
- **AND** the shader SHALL be applied as a mask to the texture
- **AND** the masked result SHALL be composited to the framebuffer

#### Scenario: Render child with radial gradient mask

- **GIVEN** a RenderShaderMask with a radial gradient shader
- **WHEN** the paint phase executes
- **THEN** the child SHALL be rendered with the radial mask applied
- **AND** the visual output SHALL match Flutter's RenderShaderMask

#### Scenario: Handle blend modes correctly

- **GIVEN** a ShaderMaskLayer with a custom blend mode
- **WHEN** compositing the masked result
- **THEN** the specified blend mode SHALL be applied
- **AND** the result SHALL match the expected blending behavior

### Requirement: BackdropFilterLayer Support

The compositor SHALL support applying image filters to backdrop content, enabling effects like frosted glass and backdrop blur.

#### Scenario: Apply blur filter to backdrop

- **GIVEN** a RenderBackdropFilter with a blur filter (radius 10.0)
- **WHEN** the paint phase executes
- **THEN** the current framebuffer content in bounds SHALL be captured
- **AND** the blur filter SHALL be applied to the captured content
- **AND** the filtered backdrop SHALL be rendered
- **AND** the child (if present) SHALL be rendered on top

#### Scenario: Render backdrop filter without child

- **GIVEN** a RenderBackdropFilter with no child (pure backdrop filter)
- **WHEN** the paint phase executes
- **THEN** only the filtered backdrop SHALL be rendered
- **AND** no child rendering SHALL occur

#### Scenario: Handle complex filters correctly

- **GIVEN** a BackdropFilterLayer with multi-pass filter (e.g., Gaussian blur)
- **WHEN** filtering the backdrop
- **THEN** all filter passes SHALL execute correctly
- **AND** the visual output SHALL match Flutter's RenderBackdropFilter

### Requirement: Layer Composition

The compositor SHALL support composing multiple layer types into a coherent scene.

#### Scenario: Render nested shader masks

- **GIVEN** a ShaderMaskLayer containing another ShaderMaskLayer as child
- **WHEN** the paint phase executes
- **THEN** both masks SHALL be applied correctly in order
- **AND** the final output SHALL show both masking effects

#### Scenario: Mix canvas and compositor layers

- **GIVEN** a scene with CanvasLayer, ShaderMaskLayer, and BackdropFilterLayer
- **WHEN** the paint phase executes
- **THEN** all layers SHALL render in correct order
- **AND** the visual output SHALL be correct

#### Scenario: Handle layer bounds correctly

- **GIVEN** a layer tree with various bounds
- **WHEN** rendering layers
- **THEN** each layer SHALL render only within its bounds
- **AND** clipping SHALL be applied correctly

### Requirement: PaintContext API Extensions

The PaintContext SHALL provide methods for RenderObjects to push compositor layers.

#### Scenario: Push shader mask from RenderObject

- **GIVEN** a RenderShaderMask during paint phase
- **WHEN** calling `ctx.push_shader_mask(shader, blend_mode, paint_fn)`
- **THEN** a ShaderMaskLayer SHALL be created
- **AND** the paint_fn closure SHALL execute with child context
- **AND** the layer SHALL be added to the layer tree

#### Scenario: Push backdrop filter from RenderObject

- **GIVEN** a RenderBackdropFilter during paint phase
- **WHEN** calling `ctx.push_backdrop_filter(filter, blend_mode, paint_fn)`
- **THEN** a BackdropFilterLayer SHALL be created
- **AND** the paint_fn closure SHALL execute with child context
- **AND** the layer SHALL be added to the layer tree

### Requirement: Resource Management

The compositor SHALL manage GPU resources efficiently for layer rendering.

#### Scenario: Texture pooling for offscreen rendering

- **GIVEN** multiple ShaderMaskLayers rendered in sequence
- **WHEN** rendering each layer
- **THEN** textures SHALL be acquired from a pool
- **AND** textures SHALL be released back to the pool after use
- **AND** total texture allocation SHALL be minimized

#### Scenario: Texture cleanup on layer destruction

- **GIVEN** a ShaderMaskLayer that is no longer needed
- **WHEN** the layer is dropped
- **THEN** any associated textures SHALL be freed
- **AND** GPU memory SHALL be reclaimed

#### Scenario: Handle texture allocation failure gracefully

- **GIVEN** insufficient GPU memory for offscreen texture
- **WHEN** attempting to allocate texture
- **THEN** the system SHALL handle the failure gracefully
- **AND** an appropriate error SHALL be logged or returned

### Requirement: Thread Safety

All layer types SHALL be thread-safe (Send + Sync) per project constraints.

#### Scenario: Layer creation on background thread

- **GIVEN** a ShaderMaskLayer created on a background thread
- **WHEN** the layer is sent to the render thread
- **THEN** the layer SHALL be valid and renderable
- **AND** no data races SHALL occur

#### Scenario: Concurrent layer rendering

- **GIVEN** multiple layers being prepared concurrently
- **WHEN** rendering on the main thread
- **THEN** all layers SHALL render correctly
- **AND** no synchronization issues SHALL occur

### Requirement: Performance Characteristics

The compositor SHALL document and meet performance targets for layer operations.

#### Scenario: ShaderMask rendering performance

- **GIVEN** a ShaderMaskLayer at 1080p resolution
- **WHEN** rendering on modern GPU
- **THEN** rendering time SHALL be ≤ 2ms per layer
- **AND** memory usage SHALL be documented

#### Scenario: BackdropFilter performance warning

- **GIVEN** BackdropFilter API documentation
- **WHEN** developer reads the docs
- **THEN** performance warnings SHALL be clearly stated
- **AND** recommended usage patterns SHALL be provided

### Requirement: Flutter Parity

The visual output SHALL match Flutter's equivalent RenderObjects.

#### Scenario: ShaderMask matches Flutter output

- **GIVEN** identical widget configuration in FLUI and Flutter
- **WHEN** rendering a ShaderMask with linear gradient
- **THEN** the FLUI output SHALL visually match Flutter
- **AND** any deviations SHALL be documented with rationale

#### Scenario: BackdropFilter matches Flutter output

- **GIVEN** identical widget configuration in FLUI and Flutter
- **WHEN** rendering a BackdropFilter with blur
- **THEN** the FLUI output SHALL visually match Flutter
- **AND** blur quality SHALL be comparable
