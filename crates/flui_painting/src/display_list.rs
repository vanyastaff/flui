//! DisplayList - Recorded sequence of drawing commands
//!
//! This module provides the `DisplayList` type which records drawing commands
//! from a Canvas for later execution by the GPU backend. This follows the
//! Command Pattern - record now, execute later.
//!
//! # Architecture
//!
//! ```text
//! Canvas::draw_rect() → DisplayList::push(DrawRect) → PictureLayer → WgpuPainter
//! ```

use flui_types::{
    events::PointerEvent,
    geometry::{Matrix4, Offset, Point, RRect, Rect, Size},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};
use std::sync::Arc;

// Re-export types that are part of the public API
pub use flui_types::painting::{BlendMode, Paint, PointMode, Shader};

/// Handler for pointer events in a hit region
///
/// Unlike flui_interaction's handler which returns EventPropagation,
/// this is a simpler callback that just receives the event.
pub type HitRegionHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// A hit-testable region with an event handler
///
/// HitRegions are added to DisplayList to enable event handling for
/// specific areas. When hit testing occurs, regions are checked in
/// reverse order (last added = topmost).
#[derive(Clone)]
pub struct HitRegion {
    /// Bounds of the hit-testable area
    pub bounds: Rect,
    /// Handler to call when pointer events occur in this region
    pub handler: HitRegionHandler,
}

impl HitRegion {
    /// Create a new hit region
    pub fn new(bounds: Rect, handler: HitRegionHandler) -> Self {
        Self { bounds, handler }
    }

    /// Check if a point is inside this region
    pub fn contains(&self, point: Point) -> bool {
        self.bounds.contains(point)
    }
}

impl std::fmt::Debug for HitRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitRegion")
            .field("bounds", &self.bounds)
            .field("handler", &"<handler>")
            .finish()
    }
}

/// A recorded sequence of drawing commands
///
/// DisplayList is immutable after recording and can be replayed multiple times
/// by the engine. It's the output of Canvas and the input to PictureLayer.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use flui_painting::{Canvas, DisplayList, Paint};
/// use flui_types::{Rect, Color};
///
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &Paint::fill(Color::RED));
/// let display_list: DisplayList = canvas.finish();
///
/// // Later, in engine:
/// for cmd in display_list.commands() {
///     match cmd {
///         DrawCommand::DrawRect { rect, paint, .. } => {
///             painter.rect(*rect, paint);
///         }
///         // ... other commands
///     }
/// }
/// ```
///
/// ## Using Transform API
///
/// ```rust,ignore
/// use flui_painting::Canvas;
/// use flui_types::geometry::Transform;
/// use std::f32::consts::PI;
///
/// let mut canvas = Canvas::new();
///
/// // Apply Transform (high-level API)
/// canvas.transform(Transform::translate(50.0, 50.0));
/// canvas.transform(Transform::rotate(PI / 4.0));
/// canvas.draw_rect(rect, &paint);
///
/// // Or compose transforms fluently
/// let composed = Transform::translate(50.0, 50.0)
///     .then(Transform::rotate(PI / 4.0))
///     .then(Transform::scale(2.0));
/// canvas.set_transform(composed);
///
/// let display_list = canvas.finish();
/// // DrawCommands now contain the transformed Matrix4
/// ```
#[derive(Debug, Clone)]
pub struct DisplayList {
    /// Drawing commands in order
    commands: Vec<DrawCommand>,

    /// Cached bounds of all drawing
    bounds: Rect,

    /// Hit-testable regions with event handlers
    hit_regions: Vec<HitRegion>,
}

impl DisplayList {
    /// Creates a new empty display list
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
            hit_regions: Vec::new(),
        }
    }

    /// Add a hit-testable region with an event handler
    ///
    /// Regions are tested in reverse order (last added = topmost).
    pub fn add_hit_region(&mut self, region: HitRegion) {
        self.hit_regions.push(region);
    }

    /// Get all hit regions
    pub fn hit_regions(&self) -> &[HitRegion] {
        &self.hit_regions
    }

    /// Adds a command to the display list (internal)
    pub(crate) fn push(&mut self, command: DrawCommand) {
        // Update bounds based on command
        if let Some(cmd_bounds) = command.bounds() {
            if self.commands.is_empty() {
                self.bounds = cmd_bounds;
            } else {
                self.bounds = self.bounds.union(&cmd_bounds);
            }
        }
        self.commands.push(command);
    }

    /// Returns an iterator over commands
    pub fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands.iter()
    }

    /// Returns the bounds of all drawing
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Returns the number of commands
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clears all commands and hit regions (for pooling/reuse)
    pub fn clear(&mut self) {
        self.commands.clear();
        self.bounds = Rect::ZERO;
        self.hit_regions.clear();
    }

    /// Appends all commands from another DisplayList (zero-copy move)
    ///
    /// This is much more efficient than cloning commands individually.
    /// Takes ownership of `other` and moves its commands into self.
    ///
    /// # Performance
    ///
    /// - O(1) if self is empty (just swap vectors)
    /// - O(N) otherwise where N = other.len() (but no cloning, just move)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut parent = DisplayList::new();
    /// parent.push(DrawCommand::DrawRect { ... });
    ///
    /// let mut child = DisplayList::new();
    /// child.push(DrawCommand::DrawCircle { ... });
    ///
    /// parent.append(child);  // Zero-copy move
    /// ```
    pub(crate) fn append(&mut self, mut other: DisplayList) {
        if self.commands.is_empty() {
            // Fast path: just swap the vectors (zero-cost)
            std::mem::swap(&mut self.commands, &mut other.commands);
            self.bounds = other.bounds;
        } else if !other.commands.is_empty() {
            // Slow path: append commands (still no cloning, just moves)
            self.commands.append(&mut other.commands);

            // Update bounds
            if !other.bounds.is_empty() {
                self.bounds = self.bounds.union(&other.bounds);
            }
        }

        // Also append hit regions
        if !other.hit_regions.is_empty() {
            self.hit_regions.append(&mut other.hit_regions);
        }
        // other.commands and hit_regions are now empty (moved), will be dropped
    }

    /// Apply opacity to all commands in this DisplayList
    ///
    /// Creates a new DisplayList where all Paint objects have their opacity
    /// multiplied by the given value. This is used for implementing opacity
    /// effects without needing a separate layer.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Value between 0.0 (fully transparent) and 1.0 (fully opaque)
    ///
    /// # Performance
    ///
    /// This method clones all commands and modifies their Paint objects.
    /// It's O(N) where N is the number of commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let display_list = canvas.finish();
    /// let semi_transparent = display_list.with_opacity(0.5);
    /// ```
    pub fn with_opacity(&self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);

        let commands = self
            .commands
            .iter()
            .map(|cmd| cmd.with_opacity(opacity))
            .collect();

        Self {
            commands,
            bounds: self.bounds, // Bounds don't change with opacity
            hit_regions: self.hit_regions.clone(), // Copy hit regions
        }
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new()
    }
}

/// A single drawing command recorded by Canvas
///
/// Each variant contains all information needed to execute the command
/// later, including the transform matrix at the time of recording.
///
/// # Transform Field
///
/// Every command stores the active `Matrix4` transform when it was recorded.
/// This transform is captured from Canvas's transform stack via:
/// - `canvas.transform(Transform::rotate(...))` - Apply Transform (high-level)
/// - `canvas.set_transform(matrix)` - Set Matrix4 directly
/// - `canvas.save()` / `canvas.restore()` - Save/restore transform state
///
/// The GPU backend applies this transform when executing the command.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::Canvas;
/// use flui_types::geometry::Transform;
///
/// let mut canvas = Canvas::new();
///
/// // Commands recorded with Transform API
/// canvas.save();
/// canvas.transform(Transform::rotate(PI / 4.0));
/// canvas.draw_rect(rect, &paint);  // ← DrawCommand stores rotated Matrix4
/// canvas.restore();
/// ```
#[derive(Debug, Clone)]
pub enum DrawCommand {
    // === Clipping Commands ===
    /// Clip to a rectangle
    ClipRect {
        /// Rectangle to clip to
        rect: Rect,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Clip to a rounded rectangle
    ClipRRect {
        /// Rounded rectangle to clip to
        rrect: RRect,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Clip to an arbitrary path
    ClipPath {
        /// Path to clip to
        path: Path,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Primitive Drawing Commands ===
    /// Draw a line
    DrawLine {
        /// Start point
        p1: Point,
        /// End point
        p2: Point,
        /// Paint style (color, stroke width, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a rectangle
    DrawRect {
        /// Rectangle to draw
        rect: Rect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a rounded rectangle
    DrawRRect {
        /// Rounded rectangle to draw
        rrect: RRect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a circle
    DrawCircle {
        /// Center point
        center: Point,
        /// Radius
        radius: f32,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an oval (ellipse)
    DrawOval {
        /// Bounding rectangle
        rect: Rect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an arbitrary path
    DrawPath {
        /// Path to draw
        path: Path,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Text ===
    /// Draw text
    DrawText {
        /// Text content
        text: String,
        /// Position offset
        offset: Offset,
        /// Text style (font, size, etc.)
        style: TextStyle,
        /// Paint style (color, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Image ===
    /// Draw an image
    DrawImage {
        /// Image
        image: Image,
        /// Destination rectangle
        dst: Rect,
        /// Optional paint (for tinting, etc.)
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Effects ===
    /// Draw a shadow
    DrawShadow {
        /// Path casting shadow
        path: Path,
        /// Shadow color
        color: Color,
        /// Elevation (blur amount)
        elevation: f32,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Advanced Primitives ===
    /// Draw an arc segment
    DrawArc {
        /// Bounding rectangle for the ellipse
        rect: Rect,
        /// Start angle in radians
        start_angle: f32,
        /// Sweep angle in radians
        sweep_angle: f32,
        /// Whether to draw from center (pie slice) or just the arc
        use_center: bool,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw difference between two rounded rectangles (ring/border)
    DrawDRRect {
        /// Outer rounded rectangle
        outer: RRect,
        /// Inner rounded rectangle
        inner: RRect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a sequence of points
    DrawPoints {
        /// Point drawing mode
        mode: PointMode,
        /// Points to draw
        points: Vec<Point>,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw custom vertices with optional colors and texture coordinates
    DrawVertices {
        /// Vertex positions
        vertices: Vec<Point>,
        /// Optional vertex colors (must match vertices length)
        colors: Option<Vec<Color>>,
        /// Optional texture coordinates (must match vertices length)
        tex_coords: Option<Vec<Point>>,
        /// Triangle indices (groups of 3)
        indices: Vec<u16>,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Fill entire canvas with a color (respects clipping)
    DrawColor {
        /// Color to fill with
        color: Color,
        /// Blend mode
        blend_mode: BlendMode,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw multiple sprites from a texture atlas
    DrawAtlas {
        /// Source image (atlas texture)
        image: Image,
        /// Source rectangles in atlas (sprite locations)
        sprites: Vec<Rect>,
        /// Destination transforms for each sprite
        transforms: Vec<Matrix4>,
        /// Optional colors to blend with each sprite
        colors: Option<Vec<Color>>,
        /// Blend mode
        blend_mode: BlendMode,
        /// Optional paint for additional effects
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },
}

impl DrawCommand {
    /// Apply opacity to the Paint in this command
    ///
    /// Creates a new DrawCommand with the Paint's opacity multiplied by the given value.
    /// This is used by DisplayList::with_opacity() to implement opacity effects.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Value between 0.0 and 1.0
    ///
    /// # Returns
    ///
    /// A new DrawCommand with modified Paint opacity.
    /// Clipping commands and commands without Paint are returned unchanged.
    pub fn with_opacity(&self, opacity: f32) -> Self {
        match self {
            DrawCommand::DrawRect {
                rect,
                paint,
                transform,
            } => DrawCommand::DrawRect {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawRRect {
                rrect,
                paint,
                transform,
            } => DrawCommand::DrawRRect {
                rrect: *rrect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawCircle {
                center,
                radius,
                paint,
                transform,
            } => DrawCommand::DrawCircle {
                center: *center,
                radius: *radius,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawOval {
                rect,
                paint,
                transform,
            } => DrawCommand::DrawOval {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => DrawCommand::DrawLine {
                p1: *p1,
                p2: *p2,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawPath {
                path,
                paint,
                transform,
            } => DrawCommand::DrawPath {
                path: path.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawText {
                text,
                offset,
                style,
                paint,
                transform,
            } => DrawCommand::DrawText {
                text: text.clone(),
                offset: *offset,
                style: style.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawShadow {
                path,
                color,
                elevation,
                transform,
            } => DrawCommand::DrawShadow {
                path: path.clone(),
                color: color.with_opacity(opacity),
                elevation: *elevation,
                transform: *transform,
            },
            DrawCommand::DrawArc {
                rect,
                start_angle,
                sweep_angle,
                use_center,
                paint,
                transform,
            } => DrawCommand::DrawArc {
                rect: *rect,
                start_angle: *start_angle,
                sweep_angle: *sweep_angle,
                use_center: *use_center,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawDRRect {
                outer,
                inner,
                paint,
                transform,
            } => DrawCommand::DrawDRRect {
                outer: *outer,
                inner: *inner,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawPoints {
                mode,
                points,
                paint,
                transform,
            } => DrawCommand::DrawPoints {
                mode: *mode,
                points: points.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawVertices {
                vertices,
                colors,
                tex_coords,
                indices,
                paint,
                transform,
            } => DrawCommand::DrawVertices {
                vertices: vertices.clone(),
                colors: colors.clone(),
                tex_coords: tex_coords.clone(),
                indices: indices.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            DrawCommand::DrawColor {
                color,
                blend_mode,
                transform,
            } => DrawCommand::DrawColor {
                color: color.with_opacity(opacity),
                blend_mode: *blend_mode,
                transform: *transform,
            },
            DrawCommand::DrawImage {
                image,
                dst,
                paint,
                transform,
            } => DrawCommand::DrawImage {
                image: image.clone(),
                dst: *dst,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            DrawCommand::DrawAtlas {
                image,
                sprites,
                transforms,
                colors,
                blend_mode,
                paint,
                transform,
            } => DrawCommand::DrawAtlas {
                image: image.clone(),
                sprites: sprites.clone(),
                transforms: transforms.clone(),
                colors: colors.clone(),
                blend_mode: *blend_mode,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            // Clipping commands don't have paint, return unchanged
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipPath { .. } => self.clone(),
        }
    }

    /// Returns the bounding rectangle of this command (if applicable)
    ///
    /// Used to calculate the DisplayList's overall bounds.
    /// This returns transformed screen-space bounds (local bounds transformed by the command's matrix).
    fn bounds(&self) -> Option<Rect> {
        match self {
            DrawCommand::DrawRect {
                rect,
                paint,
                transform,
            } => {
                // Account for stroke width if stroking
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawRRect {
                rrect,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rrect.bounding_rect().expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawCircle {
                center,
                radius,
                paint,
                transform,
            } => {
                // Circle radius + stroke outset
                let stroke_outset = paint.effective_stroke_width() * 0.5;
                let effective_radius = radius + stroke_outset;
                let size = Size::new(effective_radius * 2.0, effective_radius * 2.0);
                let local_bounds = Rect::from_center_size(*center, size);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawOval {
                rect,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawImage { dst, transform, .. } => Some(transform.transform_rect(dst)),
            DrawCommand::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => {
                // Account for stroke width
                let stroke_half = paint.effective_stroke_width() * 0.5;
                let min_x = p1.x.min(p2.x) - stroke_half;
                let min_y = p1.y.min(p2.y) - stroke_half;
                let max_x = p1.x.max(p2.x) + stroke_half;
                let max_y = p1.y.max(p2.y) + stroke_half;
                let local_bounds = Rect::from_ltrb(min_x, min_y, max_x, max_y);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawPath { .. } => {
                // Path bounds calculation requires mutable access
                // We'll compute DisplayList bounds without Path bounds for now
                None
            }
            DrawCommand::DrawShadow { .. } => {
                // Shadow bounds calculation requires path.bounds() which needs &mut Path
                // (for caching), but we only have &Path in this method.
                // Could be solved by:
                // 1. Pre-computing and storing bounds in DrawCommand
                // 2. Using interior mutability (Cell/RefCell) in Path
                // 3. Making bounds() work with &self (recompute each time)
                None
            }
            DrawCommand::DrawArc {
                rect,
                paint,
                transform,
                ..
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawDRRect {
                outer,
                paint,
                transform,
                ..
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = outer.bounding_rect().expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawPoints {
                points,
                paint,
                transform,
                ..
            } => {
                if points.is_empty() {
                    return None;
                }
                let stroke_half = paint.effective_stroke_width() * 0.5;
                let mut min_x = points[0].x;
                let mut min_y = points[0].y;
                let mut max_x = points[0].x;
                let mut max_y = points[0].y;

                for point in points.iter().skip(1) {
                    min_x = min_x.min(point.x);
                    min_y = min_y.min(point.y);
                    max_x = max_x.max(point.x);
                    max_y = max_y.max(point.y);
                }

                let local_bounds = Rect::from_ltrb(
                    min_x - stroke_half,
                    min_y - stroke_half,
                    max_x + stroke_half,
                    max_y + stroke_half,
                );
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawVertices {
                vertices,
                transform,
                ..
            } => {
                if vertices.is_empty() {
                    return None;
                }
                let mut min_x = vertices[0].x;
                let mut min_y = vertices[0].y;
                let mut max_x = vertices[0].x;
                let mut max_y = vertices[0].y;

                for vertex in vertices.iter().skip(1) {
                    min_x = min_x.min(vertex.x);
                    min_y = min_y.min(vertex.y);
                    max_x = max_x.max(vertex.x);
                    max_y = max_y.max(vertex.y);
                }

                let local_bounds = Rect::from_ltrb(min_x, min_y, max_x, max_y);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawAtlas {
                sprites,
                transforms: sprite_transforms,
                transform,
                ..
            } => {
                // Compute bounds of all transformed sprites
                if sprites.is_empty() || sprite_transforms.is_empty() {
                    return None;
                }

                // Each sprite has:
                // 1. Source rect in atlas (sprites[i])
                // 2. Destination transform (sprite_transforms[i])
                // 3. Overall command transform (transform)

                let mut combined_bounds: Option<Rect> = None;

                for (sprite_rect, sprite_transform) in sprites.iter().zip(sprite_transforms.iter())
                {
                    // Transform sprite rect by its local transform
                    let local_transformed = sprite_transform.transform_rect(sprite_rect);

                    // Then apply the overall command transform
                    let screen_bounds = transform.transform_rect(&local_transformed);

                    // Union with existing bounds
                    combined_bounds = match combined_bounds {
                        Some(existing) => Some(existing.union(&screen_bounds)),
                        None => Some(screen_bounds),
                    };
                }

                combined_bounds
            }
            DrawCommand::DrawColor { .. } => {
                // DrawColor fills entire canvas, no specific bounds
                None
            }
            // Clipping and text don't contribute to bounds directly
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipPath { .. }
            | DrawCommand::DrawText { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_list_creation() {
        let display_list = DisplayList::new();
        assert!(display_list.is_empty());
        assert_eq!(display_list.len(), 0);
        assert_eq!(display_list.bounds(), Rect::ZERO);
    }

    #[test]
    fn test_display_list_push() {
        let mut display_list = DisplayList::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::fill(Color::RED);

        display_list.push(DrawCommand::DrawRect {
            rect,
            paint,
            transform: Matrix4::identity(),
        });

        assert_eq!(display_list.len(), 1);
        assert_eq!(display_list.bounds(), rect);
    }

    #[test]
    fn test_display_list_clear() {
        let mut display_list = DisplayList::new();
        display_list.push(DrawCommand::DrawRect {
            rect: Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
            paint: Paint::default(),
            transform: Matrix4::identity(),
        });

        assert!(!display_list.is_empty());

        display_list.clear();
        assert!(display_list.is_empty());
        assert_eq!(display_list.bounds(), Rect::ZERO);
    }

    // Paint tests are now in flui_types
}

// ===== Command Pattern Implementation (Visitor Pattern) =====
