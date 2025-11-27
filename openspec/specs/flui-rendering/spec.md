# flui-rendering Specification

## Purpose
TBD - created by archiving change refactor-canvas-api-usage. Update Purpose after archive.
## Requirements
### Requirement: Canvas API Usage Patterns

RenderObjects in flui_rendering SHALL use the modern Canvas API patterns from flui_painting for improved readability, safety, and performance.

#### Scenario: Chaining API for transforms

**GIVEN** a RenderObject using manual save()/restore() pattern
**WHEN** the transform is simple (translate, rotate, scale)
**THEN** code SHALL use chaining API with saved()/restored()
**AND** transforms SHALL use translated(), rotated(), scaled_xy() methods
**AND** code SHALL be more concise and readable

#### Scenario: Consistent API usage across branches

**GIVEN** a RenderObject with conditional transforms (multiple branches)
**WHEN** some branches use chaining and others use old API
**THEN** all branches SHALL use consistent API (chaining)
**AND** saved() SHALL be used in all transform branches
**AND** restored() SHALL be called once at the end

---

