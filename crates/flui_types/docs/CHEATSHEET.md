# flui_types Cheatsheet

Quick reference for all types and common operations.

## Geometry

### Point

```rust
// Create
let p = Point::new(10.0, 20.0);
let p = Point::ZERO;

// Access
p.x, p.y

// Operations
p.distance_to(other)      // Distance between points
p.to_offset()             // Convert to Offset
p + offset                // Move point by offset
```

### Offset

```rust
// Create
let o = Offset::new(10.0, 20.0);
let o = Offset::ZERO;
let o = Offset::from_direction(angle, distance);

// Access
o.dx, o.dy

// Operations
o.distance()              // Magnitude (length)
o.direction()             // Angle in radians
o.normalize()             // Unit vector
o.scale(2.0, 3.0)         // Scale x/y independently
o.lerp(other, t)          // Interpolate
-o                        // Negate
o + other, o - other      // Add/subtract
o * 2.0, o / 2.0          // Scale
```

### Size

```rust
// Create
let s = Size::new(100.0, 50.0);
let s = Size::ZERO;
let s = Size::square(100.0);

// Access
s.width, s.height

// Operations
s.area()                  // width * height
s.aspect_ratio()          // width / height
s.is_empty()              // width <= 0 || height <= 0
s.is_finite()             // Both dimensions finite
s.flipped()               // Swap width/height
s.shortest_side()         // min(width, height)
s.longest_side()          // max(width, height)
s.lerp(other, t)          // Interpolate
s * 2.0, s / 2.0          // Scale
```

### Rect

```rust
// Create
let r = Rect::from_xywh(x, y, width, height);
let r = Rect::from_ltrb(left, top, right, bottom);
let r = Rect::from_center_size(center, size);
let r = Rect::from_points(p1, p2);
let r = Rect::ZERO;

// Access
r.left(), r.top(), r.right(), r.bottom()
r.width(), r.height()
r.size()                  // Size
r.center()                // Point

// Operations
r.contains(point)         // Point inside?
r.contains_rect(other)    // Rect inside?
r.intersects(other)       // Rects overlap?
r.intersect(other)        // Intersection rect
r.union(other)            // Bounding rect
r.inflate(dx, dy)         // Expand
r.deflate(dx, dy)         // Shrink
r.translate(offset)       // Move
r.shift(dx, dy)           // Move by dx/dy
r.lerp(other, t)          // Interpolate
```

### RRect (Rounded Rect)

```rust
// Create
let rr = RRect::from_rect_xy(rect, rx, ry);
let rr = RRect::from_rect_radius(rect, radius);
let rr = RRect::from_rect_corners(rect, tl, tr, br, bl);

// Access
rr.rect()                 // Bounding rect
rr.is_rect()              // No rounding?
rr.is_circular()          // rx == ry for all?

// Operations
rr.contains(point)        // Point inside?
rr.inflate(delta)         // Expand
rr.deflate(delta)         // Shrink
rr.lerp(other, t)         // Interpolate
```

### RelativeRect

```rust
// Create (distances from parent edges)
let rr = RelativeRect::from_ltrb(left, top, right, bottom);
let rr = RelativeRect::from_rect(rect, parent_size);
let rr = RelativeRect::from_size(size, parent_size);
let rr = RelativeRect::FILL;  // Fill parent

// Access
rr.left, rr.top, rr.right, rr.bottom

// Operations
rr.to_rect(parent_size)   // Convert to Rect
rr.to_size(parent_size)   // Get Size
rr.shift(offset)          // Move
rr.inflate(delta)         // Expand
rr.lerp(other, t)         // Interpolate
```

### Matrix4

```rust
// Create
let m = Matrix4::IDENTITY;
let m = Matrix4::translation(x, y, z);
let m = Matrix4::scaling(sx, sy, sz);
let m = Matrix4::rotation_z(radians);

// Operations
m.translate(x, y, z)      // Add translation
m.scale(sx, sy, sz)       // Add scale
m.rotate_z(radians)       // Add rotation
m.transform_point(point)  // Transform point
m.transform_rect(rect)    // Transform rect
m.inverse()               // Inverse matrix
m.determinant()           // Determinant
m * other                 // Multiply matrices
```

## Layout

### Alignment

```rust
// Constants (-1 to 1 coordinate system)
Alignment::TOP_LEFT       // (-1, -1)
Alignment::TOP_CENTER     // (0, -1)
Alignment::TOP_RIGHT      // (1, -1)
Alignment::CENTER_LEFT    // (-1, 0)
Alignment::CENTER         // (0, 0)
Alignment::CENTER_RIGHT   // (1, 0)
Alignment::BOTTOM_LEFT    // (-1, 1)
Alignment::BOTTOM_CENTER  // (0, 1)
Alignment::BOTTOM_RIGHT   // (1, 1)

// Create custom
let a = Alignment::new(0.5, -0.5);

// Operations
a.along_size(size)        // Get offset within size
a.along_offset(parent, child)  // Position child in parent
a.lerp(other, t)          // Interpolate
```

### FractionalOffset

```rust
// Constants (0 to 1 coordinate system)
FractionalOffset::TOP_LEFT      // (0, 0)
FractionalOffset::CENTER        // (0.5, 0.5)
FractionalOffset::BOTTOM_RIGHT  // (1, 1)

// Create
let f = FractionalOffset::new(0.25, 0.75);

// Convert
FractionalOffset::from_alignment(alignment)
f.to_alignment()

// Operations
f.along_size(size)        // Get offset
f.along_offset(parent, child)  // Position child
f.lerp(other, t)          // Interpolate
```

### Edges<Pixels>

```rust
// Create
let e = Edges::all(px(16.0));
let e = Edges::symmetric(px(horizontal), px(vertical));
let e = Edges::new(px(top), px(right), px(bottom), px(left));
let e = Edges::<Pixels>::ZERO;

// Access
e.left, e.top, e.right, e.bottom

// Operations
e.horizontal_total()      // left + right
e.vertical_total()        // top + bottom
e.total_size()            // Size(horizontal, vertical)
e.deflate_rect(rect)      // Shrink rect
e.inflate_rect(rect)      // Expand rect
e.deflate_size(size)      // Shrink size
e + other, e - other      // Add/subtract
e * other                 // Multiply
```

### BoxConstraints

```rust
// Create
let c = BoxConstraints::new(min_w, max_w, min_h, max_h);
let c = BoxConstraints::tight(size);       // Exact size
let c = BoxConstraints::loose(size);       // 0 to size
let c = BoxConstraints::expand();          // Infinite
let c = BoxConstraints::ZERO;

// Access
c.min_width, c.max_width, c.min_height, c.max_height

// Query
c.is_tight()              // min == max
c.is_bounded()            // max < infinity
c.has_tight_width()       // min_w == max_w
c.has_tight_height()      // min_h == max_h

// Operations
c.constrain(size)         // Clamp size to constraints
c.biggest()               // Size(max_w, max_h)
c.smallest()              // Size(min_w, min_h)
c.loosen()                // Set min to 0
c.tighten(size)           // Set to exact size
c.enforce(size)           // Clamp and return
c.deflate(insets)         // Subtract padding
```

### Axis & Direction

```rust
// Axis
Axis::Horizontal
Axis::Vertical
axis.flip()               // Opposite axis

// AxisDirection
AxisDirection::LeftToRight
AxisDirection::RightToLeft
AxisDirection::TopToBottom
AxisDirection::BottomToTop
dir.axis()                // Get Axis
dir.is_reversed()         // RTL or BTT?

// VerticalDirection
VerticalDirection::Up
VerticalDirection::Down
```

### Flex

```rust
MainAxisAlignment::Start
MainAxisAlignment::End
MainAxisAlignment::Center
MainAxisAlignment::SpaceBetween
MainAxisAlignment::SpaceAround
MainAxisAlignment::SpaceEvenly

CrossAxisAlignment::Start
CrossAxisAlignment::End
CrossAxisAlignment::Center
CrossAxisAlignment::Stretch
CrossAxisAlignment::Baseline

MainAxisSize::Min
MainAxisSize::Max

FlexFit::Tight
FlexFit::Loose
```

### Table

```rust
// Column width
TableColumnWidth::Fixed(100.0)
TableColumnWidth::Flex(1.0)
TableColumnWidth::Intrinsic
TableColumnWidth::Fraction(0.5)

// Cell alignment
TableCellVerticalAlignment::Top
TableCellVerticalAlignment::Middle
TableCellVerticalAlignment::Bottom
TableCellVerticalAlignment::Fill
TableCellVerticalAlignment::Baseline
```

### Box & Stack

```rust
// BoxFit
BoxFit::Fill              // Stretch to fill
BoxFit::Contain           // Fit inside, keep ratio
BoxFit::Cover             // Cover, keep ratio
BoxFit::FitWidth          // Fit width
BoxFit::FitHeight         // Fit height
BoxFit::None              // No scaling
BoxFit::ScaleDown         // Contain but never upscale

// BoxShape
BoxShape::Rectangle
BoxShape::Circle

// StackFit
StackFit::Loose
StackFit::Expand
StackFit::Passthrough
```

## Colors

### Color

```rust
// Create
let c = Color::rgb(255, 128, 0);
let c = Color::rgba(255, 128, 0, 200);
let c = Color::from_hex(0xFF8000FF);  // RGBA
let c = Color::from_argb(255, 255, 128, 0);

// Constants
Color::WHITE, Color::BLACK
Color::RED, Color::GREEN, Color::BLUE
Color::TRANSPARENT

// Access
c.red(), c.green(), c.blue(), c.alpha()
c.to_u32()                // Packed RGBA

// Modify
c.with_red(200)
c.with_alpha(128)
c.with_opacity(0.5)       // 0.0-1.0
c.with_luminance(0.7)     // Adjust brightness

// Operations
c.blend_over(background)  // Alpha composite
Color::lerp(a, b, t)      // Interpolate
c.relative_luminance()    // 0.0-1.0
c.contrast_ratio(other)   // WCAG ratio
```

### Gradients

```rust
// Linear
LinearGradient::new(begin, end, colors, stops)

// Radial
RadialGradient::new(center, radius, colors, stops)

// Sweep
SweepGradient::new(center, start_angle, end_angle, colors, stops)
```

## Animation

### Curves

```rust
// Built-in curves
Curve::Linear
Curve::EaseIn
Curve::EaseOut
Curve::EaseInOut
Curve::FastOutSlowIn
Curve::BounceIn
Curve::BounceOut
Curve::ElasticIn
Curve::ElasticOut

// Use
let value = curve.transform(t);  // t: 0.0-1.0

// Modify
curve.reverse()           // Flip curve
Curve::Interval { begin: 0.2, end: 0.8, curve }
```

### Tween

```rust
// Create
let t = Tween::new(0.0, 100.0);

// Use
t.transform(0.5)          // 50.0
t.lerp(0.5)               // Same as transform
```

### AnimationStatus

```rust
AnimationStatus::Dismissed  // At start
AnimationStatus::Forward    // Playing forward
AnimationStatus::Reverse    // Playing backward
AnimationStatus::Completed  // At end

status.is_running()       // Forward or Reverse
status.is_completed()     // Completed
status.is_dismissed()     // Dismissed
```

## Physics

### Spring

```rust
// Create spring description
let spring = SpringDescription::new(mass, stiffness, damping);
let spring = SpringDescription::with_damping_ratio(mass, stiffness, ratio);

// Query
spring.damping_ratio()

// Simulation
let sim = SpringSimulation::new(spring, start, end, velocity);
sim.x(time)               // Position at time
sim.dx(time)              // Velocity at time
sim.is_done(time)         // Settled?
```

### Friction

```rust
// Create
let sim = FrictionSimulation::new(drag, position, velocity);

// Use
sim.x(time)               // Position at time
sim.dx(time)              // Velocity at time
sim.is_done(time)         // Stopped?
sim.final_x()             // Final position
```

### Gravity

```rust
// Create
let sim = GravitySimulation::new(acceleration, position, velocity, end);

// Use
sim.x(time)               // Position
sim.dx(time)              // Velocity
sim.is_done(time)         // Reached end?
```

## Typography

### TextStyle

```rust
let style = TextStyle::new()
    .with_color(Color::BLACK)
    .with_font_size(16.0)
    .with_font_weight(FontWeight::BOLD)
    .with_font_style(FontStyle::Italic)
    .with_letter_spacing(1.2)
    .with_word_spacing(2.0)
    .with_height(1.5);
```

### FontWeight

```rust
FontWeight::THIN          // 100
FontWeight::LIGHT         // 300
FontWeight::NORMAL        // 400
FontWeight::MEDIUM        // 500
FontWeight::SEMI_BOLD     // 600
FontWeight::BOLD          // 700
FontWeight::EXTRA_BOLD    // 800
FontWeight::BLACK         // 900

weight.is_bold()          // >= 600
```

### TextAlign

```rust
TextAlign::Left
TextAlign::Right
TextAlign::Center
TextAlign::Justify
TextAlign::Start          // LTR: Left, RTL: Right
TextAlign::End            // LTR: Right, RTL: Left
```

### TextSelection

```rust
// Create
let sel = TextSelection::collapsed(position);  // Cursor
let sel = TextSelection::range(start, end);    // Selection

// Query
sel.start(), sel.end()
sel.is_collapsed()        // Cursor (no selection)
sel.is_valid()
```

## Gestures

### Velocity

```rust
// Create
let v = Velocity::new(px_per_sec_x, px_per_sec_y);
let v = Velocity::ZERO;

// Access
v.pixels_per_second       // Offset

// Operations
v.clamp_magnitude(min, max)
```

### PointerData

```rust
let data = PointerData::builder()
    .position(Point::new(100.0, 200.0))
    .delta(Offset::new(5.0, 0.0))
    .pressure(0.8)
    .device(PointerDeviceKind::Touch)
    .build();
```

## Platform

### Brightness

```rust
Brightness::Light
Brightness::Dark

brightness.is_light()
brightness.is_dark()
brightness.invert()
```

### Locale

```rust
let locale = Locale::new("en", "US");
locale.language_code      // "en"
locale.country_code       // Some("US")
locale.to_language_tag()  // "en-US"
```

## Painting

### Paint

```rust
let paint = Paint::fill(Color::RED);
let paint = Paint::stroke(Color::BLACK, 2.0);

paint.with_color(Color::BLUE)
paint.with_alpha(128)
paint.with_blend_mode(BlendMode::Multiply)
paint.with_anti_alias(true)
```

### BlendMode

```rust
BlendMode::Clear
BlendMode::Src
BlendMode::Dst
BlendMode::SrcOver        // Default
BlendMode::DstOver
BlendMode::SrcIn
BlendMode::DstIn
BlendMode::Multiply
BlendMode::Screen
BlendMode::Overlay
// ... and more
```

### Clip

```rust
Clip::None                // No clipping
Clip::HardEdge            // Fast, jagged edges
Clip::AntiAlias           // Smooth edges
Clip::AntiAliasWithSaveLayer  // Smooth + transparency
```

## Constraints (Sliver)

### SliverConstraints

```rust
constraints.axis
constraints.axis_direction
constraints.growth_direction
constraints.scroll_offset
constraints.remaining_paint_extent
constraints.cross_axis_extent
constraints.viewport_main_axis_extent
```

### SliverGeometry

```rust
SliverGeometry::new()
    .with_scroll_extent(100.0)
    .with_paint_extent(100.0)
    .with_max_paint_extent(100.0)
    .with_layout_extent(100.0)

SliverGeometry::ZERO
```

### CacheExtentStyle

```rust
CacheExtentStyle::Pixel     // Cache extent in pixels
CacheExtentStyle::Viewport  // Cache extent as viewport fraction
```
