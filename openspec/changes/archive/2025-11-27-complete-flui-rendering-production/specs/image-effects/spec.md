# image-effects Spec Deltas

## ADDED Requirements

### Requirement: Image Repeat Modes

RenderImage SHALL support image tiling with configurable repeat modes.

#### Scenario: Repeat image in both directions

- **GIVEN** a RenderImage with ImageRepeat::Repeat
- **WHEN** image size is smaller than bounds
- **THEN** image SHALL tile in both X and Y directions
- **AND** tiles SHALL align seamlessly
- **AND** partial tiles at edges SHALL be clipped

#### Scenario: Repeat only horizontally

- **GIVEN** a RenderImage with ImageRepeat::RepeatX
- **WHEN** rendering the image
- **THEN** image SHALL tile only in X direction
- **AND** Y direction SHALL show single instance
- **AND** alignment SHALL be respected

#### Scenario: No repeat with alignment

- **GIVEN** a RenderImage with ImageRepeat::NoRepeat and alignment
- **WHEN** image size differs from bounds
- **THEN** image SHALL be positioned according to alignment
- **AND** no tiling SHALL occur
- **AND** empty space SHALL be transparent

### Requirement: 9-Patch Rendering (Center Slice)

RenderImage SHALL support 9-patch rendering for scalable UI elements.

#### Scenario: Render 9-patch with center slice

- **GIVEN** a RenderImage with centerSlice defined
- **WHEN** rendering at different sizes
- **THEN** corners SHALL maintain original size
- **AND** edges SHALL stretch in one dimension
- **AND** center SHALL stretch in both dimensions

#### Scenario: Handle edge cases in 9-patch

- **GIVEN** a 9-patch image with very small center slice
- **WHEN** rendering at small size
- **THEN** corners SHALL not overlap
- **AND** minimum size constraints SHALL be respected
- **AND** visual quality SHALL be maintained

### Requirement: Color Blending and Filters

RenderImage SHALL support color filters and blend modes.

#### Scenario: Apply color tint

- **GIVEN** a RenderImage with color and BlendMode::Multiply
- **WHEN** rendering the image
- **THEN** image SHALL be tinted with specified color
- **AND** blend mode SHALL be applied correctly
- **AND** alpha channel SHALL be preserved

#### Scenario: Apply color matrix filter

- **GIVEN** a RenderImage with ColorFilter matrix
- **WHEN** rendering the image
- **THEN** matrix transformation SHALL be applied to each pixel
- **AND** color values SHALL be calculated correctly
- **AND** performance SHALL be GPU-accelerated

### Requirement: Image Transformations

RenderImage SHALL support flipping and inversion transformations.

#### Scenario: Flip image horizontally

- **GIVEN** a RenderImage with flipHorizontal=true
- **WHEN** rendering the image
- **THEN** image SHALL be mirrored along vertical axis
- **AND** alignment SHALL be preserved
- **AND** repeat mode SHALL work with flipped image

#### Scenario: Flip image vertically

- **GIVEN** a RenderImage with flipVertical=true
- **WHEN** rendering the image
- **THEN** image SHALL be mirrored along horizontal axis
- **AND** alignment SHALL be preserved

#### Scenario: Invert colors

- **GIVEN** a RenderImage with invertColors=true
- **WHEN** rendering the image
- **THEN** RGB channels SHALL be inverted (255 - value)
- **AND** alpha channel SHALL be preserved
- **AND** visual output SHALL match inverted image
