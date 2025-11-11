# Spec Delta: Painting API

**Change ID:** `migrate-canvas-api`
**Capability:** `painting-api`
**Status:** Implemented

## ADDED Requirements

### Requirement: Canvas API for Drawing Operations

The system SHALL provide a Canvas API for recording drawing operations in a backend-agnostic manner.

**Rationale**: Decouple rendering logic from GPU implementation details, enabling testing without GPU and following Flutter's proven Canvas pattern.

#### Scenario: Recording Rectangle Drawing

**Given** a Canvas instance
**When** `draw_rect(rect, paint)` is called
**Then** a DrawRect command is recorded in the DisplayList
**And** the command includes the current transform matrix
**And** the paint style is stored with the command

```rust
let mut canvas = Canvas::new();
let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
let paint = Paint::fill(Color::RED);

canvas.draw_rect(rect, &paint);

let display_list = canvas.finish();
assert_eq!(display_list.len(), 1);
```

#### Scenario: Recording Multiple Drawing Commands

**Given** a Canvas instance
**When** multiple drawing commands are executed (rect, circle, path)
**Then** all commands are recorded in order in the DisplayList
**And** each command preserves its transform state
**And** commands can be inspected without GPU execution

```rust
let mut canvas = Canvas::new();
canvas.draw_rect(rect1, &paint1);
canvas.draw_circle(center, radius, &paint2);
canvas.draw_path(&path, &paint3);

let display_list = canvas.finish();
assert_eq!(display_list.len(), 3);
```

#### Scenario: Transform State Management

**Given** a Canvas with identity transform
**When** transforms are applied (translate, scale, rotate)
**Then** subsequent commands use the modified transform
**And** save/restore preserves and restores transform state

```rust
let mut canvas = Canvas::new();

canvas.save();
canvas.translate(50.0, 50.0);
canvas.draw_rect(rect, &paint);  // Drawn at (50, 50)
canvas.restore();

canvas.draw_rect(rect, &paint);  // Drawn at (0, 0)
```

### Requirement: DisplayList Command Recording

The system SHALL provide a DisplayList structure for storing recorded drawing commands.

**Rationale**: Enable deferred execution of drawing commands by the GPU backend, allowing for optimization, caching, and inspection.

#### Scenario: Command Storage and Retrieval

**Given** a DisplayList
**When** commands are added
**Then** commands are stored in order
**And** commands can be iterated
**And** DisplayList can be cloned for caching

```rust
let mut display_list = DisplayList::new();
display_list.push(DrawCommand::DrawRect { rect, paint, transform });
display_list.push(DrawCommand::DrawCircle { center, radius, paint, transform });

assert_eq!(display_list.len(), 2);
let commands = display_list.commands();
// Can inspect commands without GPU
```

#### Scenario: Canvas Composition

**Given** a parent Canvas and child Canvas
**When** `append_canvas(child)` is called
**Then** all child commands are appended to parent DisplayList
**And** command order is preserved
**And** transforms are maintained correctly

```rust
let mut parent_canvas = Canvas::new();
parent_canvas.draw_rect(background, &bg_paint);

let mut child_canvas = Canvas::new();
child_canvas.draw_text("Hello", offset, &style, &paint);

parent_canvas.append_canvas(child_canvas);

let display_list = parent_canvas.finish();
assert_eq!(display_list.len(), 2);  // background + text
```

### Requirement: Comprehensive Drawing Primitives

The system SHALL support all common 2D drawing primitives matching Flutter's Canvas API.

**Rationale**: Provide complete feature parity with Flutter for familiar API and comprehensive rendering capabilities.

#### Scenario: Basic Shape Drawing

**Given** a Canvas instance
**When** drawing basic shapes (rect, circle, oval, rounded rect)
**Then** appropriate commands are recorded
**And** shapes support both fill and stroke styles

```rust
canvas.draw_rect(rect, &fill_paint);
canvas.draw_circle(center, radius, &stroke_paint);
canvas.draw_oval(bounds, &paint);
canvas.draw_rrect(rounded_rect, &paint);
```

#### Scenario: Path Drawing

**Given** a Canvas and arbitrary Path
**When** `draw_path(path, paint)` is called
**Then** a DrawPath command is recorded
**And** the path is cloned and stored

```rust
let mut path = Path::new();
path.move_to(Point::new(0.0, 0.0));
path.line_to(Point::new(100.0, 0.0));
path.line_to(Point::new(50.0, 100.0));
path.close();

canvas.draw_path(&path, &paint);
```

#### Scenario: Text Drawing

**Given** a Canvas, text string, and TextStyle
**When** `draw_text(text, offset, style, paint)` is called
**Then** a DrawText command is recorded
**And** text, style, and paint are stored

```rust
let text = "Hello, World!";
let style = TextStyle::new().font_size(24.0);
let offset = Offset::new(10.0, 10.0);

canvas.draw_text(text, offset, &style, &paint);
```

#### Scenario: Advanced Drawing Features

**Given** a Canvas instance
**When** using advanced features (arc, drrect, vertices, atlas)
**Then** corresponding commands are recorded
**And** all parameters are preserved

```rust
// Arc drawing
canvas.draw_arc(bounds, 0.0, std::f32::consts::PI, true, &paint);

// Double rounded rect (ring/border)
canvas.draw_drrect(outer_rrect, inner_rrect, &paint);

// Custom vertices
canvas.draw_vertices(vertices, Some(colors), None, indices, &paint);

// Sprite atlas
canvas.draw_atlas(atlas_image, sprites, transforms, None, BlendMode::SrcOver, None);
```

### Requirement: Clipping Operations

The system SHALL support rectangular, rounded rectangular, and path-based clipping.

**Rationale**: Enable masking of drawing operations to specific regions, essential for scroll views, cards, and complex layouts.

#### Scenario: Rectangle Clipping

**Given** a Canvas instance
**When** `clip_rect(rect)` is called
**Then** a ClipRect command is recorded
**And** subsequent drawing is clipped to the rectangle

```rust
canvas.clip_rect(clip_bounds);
canvas.draw_rect(large_rect, &paint);  // Only visible part drawn
```

#### Scenario: Nested Clipping

**Given** a Canvas with save/restore
**When** clipping operations are nested with save/restore
**Then** clip stack is properly managed
**And** restore pops clip state

```rust
canvas.save();
canvas.clip_rect(outer_clip);

canvas.save();
canvas.clip_rect(inner_clip);  // Intersection of both clips
canvas.draw_rect(rect, &paint);
canvas.restore();  // Back to outer clip

canvas.restore();  // Back to no clip
```

## MODIFIED Requirements

_No existing requirements modified_

## REMOVED Requirements

_No requirements removed_

## Dependencies

- **flui_types**: Provides geometry types (Rect, Point, Size, etc.)
- **Related Changes**: See `core-rendering` and `rendering-api` specs

## Notes

- All Canvas methods are designed to match Flutter's Canvas API for familiarity
- DisplayList is backend-agnostic - no GPU-specific code
- Commands are cloneable for caching and optimization
- Transform state is automatically applied to all commands
