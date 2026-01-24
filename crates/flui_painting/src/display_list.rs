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
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Size},
    painting::{Image, Path},
    styling::Color,
    typography::{InlineSpan, TextStyle},
};
use std::sync::Arc;
use std::time::Duration;

/// A pointer event for hit testing in display lists.
///
/// This is a minimal event type used for hit region handlers.
/// The full event system is in `flui_interaction`.
#[derive(Debug, Clone)]
pub struct PointerEvent {
    /// The type of pointer event.
    pub kind: PointerEventKind,
    /// The position of the event in local coordinates.
    pub position: Offset<Pixels>,
    /// The pointer ID.
    pub pointer: i32,
    /// The button state (for mouse events).
    pub buttons: i32,
    /// Time of the event.
    pub time_stamp: Duration,
}

/// The kind of pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEventKind {
    /// Pointer entered a region.
    Enter,
    /// Pointer exited a region.
    Exit,
    /// Pointer button pressed.
    Down,
    /// Pointer moved.
    Move,
    /// Pointer button released.
    Up,
    /// Pointer interaction cancelled.
    Cancel,
}

impl PointerEvent {
    /// Create a new pointer event.
    pub fn new(kind: PointerEventKind, position: Offset<Pixels>, pointer: i32) -> Self {
        Self {
            kind,
            position,
            pointer,
            buttons: 0,
            time_stamp: Duration::ZERO,
        }
    }
}

// Re-export types that are part of the public API
pub use flui_types::painting::{
    effects::ImageFilter, image::ColorFilter, image::ImageRepeat, BlendMode, FilterQuality, Paint,
    PointMode, Shader, TextureId,
};

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
    pub fn contains(&self, point: Point<Pixels>) -> bool {
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayList {
    /// Drawing commands in order
    commands: Vec<DrawCommand>,

    /// Cached bounds of all drawing
    bounds: Rect,

    /// Hit-testable regions with event handlers
    #[cfg_attr(feature = "serde", serde(skip))]
    hit_regions: Vec<HitRegion>,
}

// ===== Sealed Trait Pattern =====

/// Internal module for sealing traits.
///
/// This module is public for technical reasons (trait bounds) but should not be
/// used directly. It's hidden from documentation.
#[doc(hidden)]
pub mod private {
    /// Sealed trait to prevent external implementations of DisplayListCore.
    ///
    /// This ensures only types in this crate can implement DisplayListCore,
    /// allowing us to add methods to DisplayListExt without breaking changes.
    pub trait Sealed {}
}

/// Core DisplayList API providing fundamental access methods.
///
/// This trait defines the minimal interface for accessing display list data.
/// It is sealed to prevent external implementations, allowing the library to add
/// methods to [`DisplayListExt`] in the future without breaking changes.
///
/// # Implementation
///
/// This trait is automatically implemented for:
/// - [`DisplayList`]
/// - `Arc<DisplayList>`
/// - `Box<DisplayList>`
/// - `&DisplayList`
///
/// All implementors automatically receive extension methods from [`DisplayListExt`]
/// via a blanket implementation.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::prelude::*;
///
/// fn process_commands(dl: &impl DisplayListCore) {
///     println!("Commands: {}", dl.len());
///     for cmd in dl.commands() {
///         // Process each command
///     }
/// }
/// ```
///
/// # See Also
///
/// - [`DisplayListExt`] for convenient filtering and query methods
pub trait DisplayListCore: private::Sealed {
    /// Returns an iterator over all commands in this display list.
    ///
    /// The iterator yields references to [`DrawCommand`]s in the order they were recorded.
    fn commands(&self) -> impl Iterator<Item = &DrawCommand>;

    /// Returns the bounding rectangle containing all drawing operations.
    ///
    /// The bounds are calculated incrementally as commands are added and represent
    /// the union of all command bounds.
    fn bounds(&self) -> Rect;

    /// Returns the total number of commands in this display list.
    fn len(&self) -> usize;

    /// Returns `true` if this display list contains no commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let display_list = DisplayList::new();
    /// assert!(display_list.is_empty());
    /// ```
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Extension trait providing convenient filtering and query methods.
///
/// This trait is automatically implemented for all types implementing [`DisplayListCore`]
/// via a blanket implementation. It provides zero-cost methods for filtering, counting,
/// and analyzing drawing commands.
///
/// # Automatic Implementation
///
/// You don't need to implement this trait manually. Any type implementing
/// [`DisplayListCore`] automatically gets all these methods.
///
/// # Performance
///
/// All filtering methods return lazy iterators that don't allocate. They only
/// iterate through commands when consumed.
///
/// # Examples
///
/// ## Filtering Commands
///
/// ```rust,ignore
/// use flui_painting::prelude::*;
///
/// let display_list = canvas.finish();
///
/// // Get only drawing commands (excludes clips, layers, effects)
/// for cmd in display_list.draw_commands() {
///     // Process drawing commands
/// }
///
/// // Get only shape commands
/// let shape_count = display_list.shape_commands().count();
/// ```
///
/// ## Statistics
///
/// ```rust,ignore
/// let stats = display_list.stats();
/// println!("Shapes: {}, Text: {}, Images: {}",
///          stats.shapes, stats.text, stats.images);
/// ```
///
/// # See Also
///
/// - [`DisplayListCore`] for the core API
/// - [`DisplayListStats`] for statistics structure
pub trait DisplayListExt: DisplayListCore {
    /// Returns an iterator over drawing commands only.
    ///
    /// This excludes clipping commands, layer commands, and effect commands,
    /// returning only commands that produce visible output.
    ///
    /// # Performance
    ///
    /// Returns a lazy iterator - no allocation until consumed.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for cmd in display_list.draw_commands() {
    ///     // Only shapes, text, and images
    /// }
    /// ```
    fn draw_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_draw())
    }

    /// Returns an iterator over clipping commands only.
    ///
    /// Includes `ClipRect`, `ClipRRect`, and `ClipPath` commands.
    fn clip_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_clip())
    }

    /// Returns an iterator over shape drawing commands.
    ///
    /// Includes rectangles, circles, paths, ovals, and other geometric primitives.
    /// Excludes text and images.
    fn shape_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_shape())
    }

    /// Returns an iterator over image and texture commands.
    ///
    /// Includes `DrawImage`, `DrawTexture`, and image-related commands.
    fn image_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_image())
    }

    /// Returns an iterator over text rendering commands.
    fn text_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_text())
    }

    /// Counts commands grouped by their category.
    ///
    /// Returns a tuple of `(draw, clip, effect, layer)` command counts.
    ///
    /// # Performance
    ///
    /// O(n) where n is the number of commands. Iterates through all commands once.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let (draws, clips, effects, layers) = display_list.count_by_kind();
    /// println!("Drawing: {}, Clipping: {}, Effects: {}, Layers: {}",
    ///          draws, clips, effects, layers);
    /// ```
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `draw`: Number of drawing commands (shapes, text, images)
    /// - `clip`: Number of clipping commands
    /// - `effect`: Number of effect commands (shader masks, filters)
    /// - `layer`: Number of layer commands (save/restore layer)
    fn count_by_kind(&self) -> (usize, usize, usize, usize) {
        let mut draw = 0;
        let mut clip = 0;
        let mut effect = 0;
        let mut layer = 0;

        for cmd in self.commands() {
            match cmd.kind() {
                CommandKind::Draw => draw += 1,
                CommandKind::Clip => clip += 1,
                CommandKind::Effect => effect += 1,
                CommandKind::Layer => layer += 1,
            }
        }

        (draw, clip, effect, layer)
    }

    /// Computes comprehensive statistics about this display list.
    ///
    /// Returns a [`DisplayListStats`] structure containing detailed counts
    /// of different command types.
    ///
    /// # Performance
    ///
    /// O(n) - iterates through all commands to count by type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let stats = display_list.stats();
    /// println!("{}", stats); // Pretty-printed: "DisplayList: 42 commands..."
    ///
    /// // Access individual counts
    /// assert_eq!(stats.shapes, 10);
    /// assert_eq!(stats.text, 5);
    /// ```
    ///
    /// # See Also
    ///
    /// - [`DisplayListStats`] for the statistics structure
    /// - [`count_by_kind`](Self::count_by_kind) for category counts only
    fn stats(&self) -> DisplayListStats {
        let (draw, clip, effect, layer) = self.count_by_kind();
        let shapes = self.commands().filter(|c| c.is_shape()).count();
        let images = self.commands().filter(|c| c.is_image()).count();
        let text = self.commands().filter(|c| c.is_text()).count();

        DisplayListStats {
            total: self.len(),
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions: 0, // Will be overridden in DisplayList impl
        }
    }
}

// Blanket implementation: all DisplayListCore types get DisplayListExt methods
impl<T: DisplayListCore> DisplayListExt for T {}

// Implement sealed trait for DisplayList
impl private::Sealed for DisplayList {}

// Implement core trait for DisplayList
impl DisplayListCore for DisplayList {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands.iter()
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn len(&self) -> usize {
        self.commands.len()
    }
}

// ===== Blanket Implementations for Smart Pointers =====

// Implement sealed trait for Arc<DisplayList>
impl private::Sealed for Arc<DisplayList> {}

// Implement core trait for Arc<DisplayList>
impl DisplayListCore for Arc<DisplayList> {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (**self).commands()
    }

    fn bounds(&self) -> Rect {
        (**self).bounds()
    }

    fn len(&self) -> usize {
        (**self).len()
    }
}

// Implement sealed trait for Box<DisplayList>
impl private::Sealed for Box<DisplayList> {}

// Implement core trait for Box<DisplayList>
impl DisplayListCore for Box<DisplayList> {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (**self).commands()
    }

    fn bounds(&self) -> Rect {
        (**self).bounds()
    }

    fn len(&self) -> usize {
        (**self).len()
    }
}

// Implement sealed trait for &DisplayList
impl private::Sealed for &DisplayList {}

// Implement core trait for &DisplayList
impl DisplayListCore for &DisplayList {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (*self).commands()
    }

    fn bounds(&self) -> Rect {
        (*self).bounds()
    }

    fn len(&self) -> usize {
        (*self).len()
    }
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

    /// Returns an iterator over command references.
    ///
    /// This method is provided to satisfy clippy's convention for types
    /// that implement `IntoIterator`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let display_list = canvas.finish();
    ///
    /// for cmd in display_list.iter() {
    ///     // process command
    /// }
    /// ```
    pub fn iter(&self) -> std::slice::Iter<'_, DrawCommand> {
        self.commands.iter()
    }

    /// Returns an iterator over mutable command references.
    ///
    /// This method is provided to satisfy clippy's convention for types
    /// that implement `IntoIterator`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut display_list = canvas.finish();
    ///
    /// for cmd in display_list.iter_mut() {
    ///     // modify command
    /// }
    /// ```
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, DrawCommand> {
        self.commands.iter_mut()
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

    /// Returns an iterator over mutable command references.
    pub fn commands_mut(&mut self) -> impl Iterator<Item = &mut DrawCommand> {
        self.commands.iter_mut()
    }

    /// Returns statistics about this display list (overrides extension trait).
    ///
    /// This implementation includes hit_regions count, which the default
    /// extension trait implementation cannot access.
    pub fn stats(&self) -> DisplayListStats {
        let (draw, clip, effect, layer) = self.count_by_kind();
        let shapes = self.commands().filter(|c| c.is_shape()).count();
        let images = self.commands().filter(|c| c.is_image()).count();
        let text = self.commands().filter(|c| c.is_text()).count();

        DisplayListStats {
            total: self.len(),
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions: self.hit_regions.len(),
        }
    }

    /// Applies a transform to all commands in this display list.
    ///
    /// Modifies commands in-place. Useful for positioning cached display lists.
    pub fn apply_transform(&mut self, transform: Matrix4) {
        for cmd in &mut self.commands {
            cmd.apply_transform(transform);
        }
        // Recalculate bounds
        self.recalculate_bounds();
    }

    /// Recalculates the bounds from all commands.
    fn recalculate_bounds(&mut self) {
        self.bounds = Rect::ZERO;
        for cmd in &self.commands {
            if let Some(cmd_bounds) = cmd.bounds() {
                if self.bounds == Rect::ZERO {
                    self.bounds = cmd_bounds;
                } else {
                    self.bounds = self.bounds.union(&cmd_bounds);
                }
            }
        }
    }

    /// Filters commands, keeping only those that satisfy the predicate.
    ///
    /// Returns a new DisplayList with filtered commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Keep only shape commands
    /// let shapes_only = display_list.filter(|cmd| cmd.is_shape());
    /// ```
    #[must_use = "filter returns a new DisplayList and does not modify the original"]
    pub fn filter<F>(&self, predicate: F) -> Self
    where
        F: Fn(&DrawCommand) -> bool,
    {
        let commands: Vec<_> = self
            .commands
            .iter()
            .filter(|cmd| predicate(cmd))
            .cloned()
            .collect();

        let mut result = Self {
            commands,
            bounds: Rect::ZERO,
            hit_regions: self.hit_regions.clone(),
        };
        result.recalculate_bounds();
        result
    }

    /// Maps each command through a function.
    ///
    /// Returns a new DisplayList with transformed commands.
    #[must_use = "map returns a new DisplayList and does not modify the original"]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: Fn(&DrawCommand) -> DrawCommand,
    {
        let commands: Vec<_> = self.commands.iter().map(f).collect();

        let mut result = Self {
            commands,
            bounds: Rect::ZERO,
            hit_regions: self.hit_regions.clone(),
        };
        result.recalculate_bounds();
        result
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
    #[tracing::instrument(skip(self, other), fields(
        parent_len = self.commands.len(),
        child_len = other.commands.len(),
    ))]
    pub(crate) fn append(&mut self, mut other: DisplayList) {
        if self.commands.is_empty() {
            // Fast path: just swap the vectors (zero-cost)
            tracing::trace!("Using fast path: vector swap (O(1))");
            std::mem::swap(&mut self.commands, &mut other.commands);
            self.bounds = other.bounds;
        } else if !other.commands.is_empty() {
            // Slow path: append commands (still no cloning, just moves)
            tracing::trace!(
                commands_to_append = other.commands.len(),
                "Using append path (O(N))"
            );
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

        tracing::debug!(
            total_commands = self.commands.len(),
            "DisplayList append complete"
        );
        // other.commands and hit_regions are now empty (moved), will be dropped
    }

    /// Apply opacity to all commands, creating a new DisplayList
    ///
    /// Creates a new DisplayList where all Paint objects have their opacity
    /// multiplied by the given value. This is used for implementing opacity
    /// effects without needing a separate layer.
    ///
    /// # Naming
    ///
    /// Uses `to_` prefix following Rust API Guidelines (C-CONV) because this is
    /// an expensive borrowed-to-owned conversion (clones all commands).
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
    /// let semi_transparent = display_list.to_opacity(0.5);
    /// ```
    #[must_use = "to_opacity returns a new DisplayList and does not modify the original"]
    #[tracing::instrument(skip(self), fields(
        commands = self.commands.len(),
        opacity = opacity,
    ))]
    pub fn to_opacity(&self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);

        tracing::debug!(
            commands = self.commands.len(),
            clamped_opacity = opacity,
            "Applying opacity to DisplayList"
        );

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

/// Detailed statistics about a [`DisplayList`]'s contents.
///
/// Provides counts of different command types to help analyze rendering complexity
/// and optimize performance.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::prelude::*;
///
/// let stats = display_list.stats();
///
/// // Check rendering complexity
/// if stats.total > 1000 {
///     println!("Warning: Complex display list with {} commands", stats.total);
/// }
///
/// // Pretty print
/// println!("{}", stats);
/// // Output: "DisplayList: 42 commands (10 shapes, 5 images, ...)"
/// ```
///
/// # Field Categories
///
/// - **Total**: All commands
/// - **By Category**: `draw`, `clip`, `effect`, `layer`
/// - **By Content Type**: `shapes`, `images`, `text` (subsets of `draw`)
/// - **Other**: `hit_regions`
///
/// # See Also
///
/// - [`DisplayListExt::stats`] for computing statistics
/// - [`DisplayList`] for the main type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayListStats {
    /// Total number of commands
    pub total: usize,
    /// Number of drawing commands
    pub draw: usize,
    /// Number of clipping commands
    pub clip: usize,
    /// Number of effect commands
    pub effect: usize,
    /// Number of layer commands
    pub layer: usize,
    /// Number of shape commands (subset of draw)
    pub shapes: usize,
    /// Number of image/texture commands (subset of draw)
    pub images: usize,
    /// Number of text commands (subset of draw)
    pub text: usize,
    /// Number of hit regions
    pub hit_regions: usize,
}

impl DisplayListStats {
    /// Creates a new statistics object with all counts set to zero.
    ///
    /// This is a `const fn` and can be used in constant contexts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_painting::DisplayListStats;
    ///
    /// const EMPTY: DisplayListStats = DisplayListStats::zero();
    /// assert_eq!(EMPTY.total, 0);
    /// ```
    pub const fn zero() -> Self {
        Self {
            total: 0,
            draw: 0,
            clip: 0,
            effect: 0,
            layer: 0,
            shapes: 0,
            images: 0,
            text: 0,
            hit_regions: 0,
        }
    }

    /// Creates a new statistics object with the specified counts.
    ///
    /// This is a `const fn` and can be used in constant contexts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_painting::DisplayListStats;
    ///
    /// const STATS: DisplayListStats = DisplayListStats::new(
    ///     100, // total
    ///     80,  // draw
    ///     10,  // clip
    ///     5,   // effect
    ///     5,   // layer
    ///     50,  // shapes
    ///     20,  // images
    ///     10,  // text
    ///     0    // hit_regions
    /// );
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        total: usize,
        draw: usize,
        clip: usize,
        effect: usize,
        layer: usize,
        shapes: usize,
        images: usize,
        text: usize,
        hit_regions: usize,
    ) -> Self {
        Self {
            total,
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions,
        }
    }
}

impl std::fmt::Display for DisplayListStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DisplayList: {} commands ({} shapes, {} images, {} text, {} clips, {} effects, {} layers), {} hit regions",
            self.total, self.shapes, self.images, self.text, self.clip, self.effect, self.layer, self.hit_regions
        )
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
        p1: Point<Pixels>,
        /// End point
        p2: Point<Pixels>,
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
        center: Point<Pixels>,
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
        offset: Offset<Pixels>,
        /// Pre-computed size of the text (for bounds calculation)
        size: Size<Pixels>,
        /// Text style (font, size, etc.)
        style: TextStyle,
        /// Paint style (color, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw rich text with inline spans
    DrawTextSpan {
        /// Rich text span (with nested styles)
        span: InlineSpan,
        /// Position offset
        offset: Offset<Pixels>,
        /// Text scale factor for accessibility
        text_scale_factor: f64,
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

    /// Draw an image with repeat (tiling)
    ///
    /// Tiles the image to fill the destination rectangle based on repeat mode.
    DrawImageRepeat {
        /// Image to tile
        image: Image,
        /// Destination rectangle to fill
        dst: Rect,
        /// How to repeat the image
        repeat: ImageRepeat,
        /// Optional paint (for tinting, opacity, etc.)
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an image with 9-slice/9-patch scaling
    ///
    /// Draws the image with a center slice that scales while corners and edges
    /// are drawn at their natural size. This is useful for resizable UI elements
    /// like buttons, panels, and chat bubbles.
    DrawImageNineSlice {
        /// Image to draw
        image: Image,
        /// Center slice rectangle within the image (in image coordinates)
        /// This area will be scaled; areas outside will maintain their size
        center_slice: Rect,
        /// Destination rectangle
        dst: Rect,
        /// Optional paint (for tinting, opacity, etc.)
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an image with a color filter
    ///
    /// Applies a color filter (tint, grayscale, sepia, etc.) when drawing.
    DrawImageFiltered {
        /// Image to draw
        image: Image,
        /// Destination rectangle
        dst: Rect,
        /// Color filter to apply
        filter: ColorFilter,
        /// Optional paint (for additional effects)
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Texture ===
    /// Draw a GPU texture referenced by ID
    ///
    /// This command renders an external GPU texture (e.g., video frame,
    /// camera preview, platform view) to the destination rectangle.
    /// The texture must be registered with the rendering engine before use.
    ///
    /// # Use Cases
    ///
    /// - Video playback (decoder output)
    /// - Camera preview streams
    /// - External rendering contexts
    /// - Platform views (native UI embedded in FLUI)
    DrawTexture {
        /// GPU texture identifier
        texture_id: TextureId,
        /// Destination rectangle
        dst: Rect,
        /// Source rectangle within the texture (None = entire texture)
        src: Option<Rect>,
        /// Filter quality for texture sampling
        filter_quality: FilterQuality,
        /// Opacity (0.0 = transparent, 1.0 = opaque)
        opacity: f32,
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

    // === Gradient Drawing Commands ===
    /// Draw a gradient-filled rectangle
    ///
    /// This command renders a rectangle filled with the specified gradient shader.
    /// Supports linear, radial, and sweep gradients.
    DrawGradient {
        /// Rectangle to fill
        rect: Rect,
        /// Gradient shader
        shader: Shader,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a gradient-filled rounded rectangle
    ///
    /// This command renders a rounded rectangle filled with the specified gradient.
    DrawGradientRRect {
        /// Rounded rectangle to fill
        rrect: RRect,
        /// Gradient shader
        shader: Shader,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Apply a shader as a mask to child content
    ///
    /// This command wraps a sub-DisplayList (child content) and applies a shader
    /// as an alpha mask. The shader determines the opacity at each pixel.
    ///
    /// # Shader Types
    ///
    /// - **Linear Gradient**: Fade along a line (e.g., fade-out edges)
    /// - **Radial Gradient**: Circular fade (e.g., vignette effect)
    /// - **Solid Color**: Uniform opacity mask
    ///
    /// # Example Use Cases
    ///
    /// - Image fade-out at edges
    /// - Vignette effects
    /// - Gradient overlays
    /// - Custom masking effects
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas::draw_shader_mask() → DisplayList::push(ShaderMask)
    ///     ↓
    /// CommandRenderer → OffscreenRenderer.render_masked()
    ///     ↓
    /// GPU shader execution
    /// ```
    ShaderMask {
        /// Child content to be masked (recorded commands)
        child: Box<DisplayList>,
        /// Shader specification (gradient type, colors, etc.)
        shader: Shader,
        /// Bounds of the masked region
        bounds: Rect,
        /// Blend mode for final compositing
        blend_mode: BlendMode,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Backdrop filter effect (frosted glass, blur)
    ///
    /// Applies an image filter to the backdrop content behind this layer.
    /// Common use cases:
    /// - Frosted glass effect (blur + transparency)
    /// - Background blur for modals
    /// - Color adjustments to backdrop
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas::draw_backdrop_filter() → DisplayList::push(BackdropFilter)
    ///     ↓
    /// CommandRenderer → capture framebuffer → apply filter → composite
    ///     ↓
    /// GPU filter execution (compute shader for blur)
    /// ```
    BackdropFilter {
        /// Child content to render on top of filtered backdrop (optional)
        child: Option<Box<DisplayList>>,
        /// Image filter to apply (blur, color adjustments, etc.)
        filter: ImageFilter,
        /// Bounds for backdrop capture
        bounds: Rect,
        /// Blend mode for final compositing
        blend_mode: BlendMode,
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
        points: Vec<Point<Pixels>>,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw custom vertices with optional colors and texture coordinates
    DrawVertices {
        /// Vertex positions
        vertices: Vec<Point<Pixels>>,
        /// Optional vertex colors (must match vertices length)
        colors: Option<Vec<Color>>,
        /// Optional texture coordinates (must match vertices length)
        tex_coords: Option<Vec<Point<Pixels>>>,
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

    // === Layer Commands ===
    /// Save the current canvas state and create a new compositing layer
    ///
    /// This is similar to `save()` but creates an offscreen buffer for the
    /// subsequent drawing commands. When `RestoreLayer` is called, the layer
    /// is composited back with the specified paint settings (opacity, blend mode, etc.).
    ///
    /// # Use Cases
    ///
    /// - **Opacity effects**: Apply uniform transparency to a group of drawings
    /// - **Blend modes**: Apply complex blending to multiple overlapping elements
    /// - **Anti-aliasing**: Get clean edges when clipping overlapping content
    ///
    /// # Performance
    ///
    /// `SaveLayer` is relatively expensive because it:
    /// 1. Forces GPU to switch render targets
    /// 2. Allocates an offscreen buffer
    /// 3. Requires copying framebuffer contents
    ///
    /// Use sparingly, especially on lower-end hardware.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Draw a group of shapes at 50% opacity
    /// canvas.save_layer(bounds, Paint::new().with_opacity(0.5));
    /// canvas.draw_rect(rect1, &red_paint);
    /// canvas.draw_rect(rect2, &blue_paint);
    /// canvas.restore(); // Composites the layer at 50% opacity
    /// ```
    SaveLayer {
        /// Bounds of the layer (None = unbounded, clips to current clip)
        bounds: Option<Rect>,
        /// Paint to apply when compositing the layer (opacity, blend mode, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Restore the canvas state and composite the saved layer
    ///
    /// This pops the save stack and composites the layer created by `SaveLayer`
    /// using the paint settings specified when the layer was saved.
    RestoreLayer {
        /// Transform at recording time (for consistency)
        transform: Matrix4,
    },
}

impl DrawCommand {
    /// Apply opacity to the Paint in this command.
    ///
    /// Creates a new DrawCommand with the Paint's opacity multiplied by the given value.
    /// This is used by DisplayList::to_opacity() to implement opacity effects.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Value between 0.0 and 1.0
    ///
    /// # Returns
    ///
    /// A new DrawCommand with modified Paint opacity.
    /// Clipping commands and commands without Paint are returned unchanged.
    ///
    /// # Opacity Handling by Command Type
    ///
    /// Commands are categorized by how they handle opacity:
    /// - **Paint commands**: Apply opacity to `paint.with_opacity(opacity)`
    /// - **Optional paint commands**: Map over Option<Paint> and apply opacity
    /// - **Color commands**: Apply opacity to color directly
    /// - **Child commands**: Recursively apply opacity to child DisplayList
    /// - **Texture commands**: Multiply existing opacity field
    /// - **Passthrough commands**: Return unchanged (clips, gradients, etc.)
    #[must_use = "with_opacity returns a new DrawCommand and does not modify the original"]
    pub fn with_opacity(&self, opacity: f32) -> Self {
        match self {
            // ─────────────────────────────────────────────────────────────────
            // Passthrough: Commands without opacity (clips, gradients, etc.)
            // ─────────────────────────────────────────────────────────────────
            Self::ClipRect { .. }
            | Self::ClipRRect { .. }
            | Self::ClipPath { .. }
            | Self::DrawTextSpan { .. }
            | Self::DrawGradient { .. }
            | Self::DrawGradientRRect { .. }
            | Self::RestoreLayer { .. } => self.clone(),

            // ─────────────────────────────────────────────────────────────────
            // Paint commands: Apply opacity to paint field
            // ─────────────────────────────────────────────────────────────────
            Self::DrawRect { rect, paint, transform } => Self::DrawRect {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawRRect { rrect, paint, transform } => Self::DrawRRect {
                rrect: *rrect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawCircle { center, radius, paint, transform } => Self::DrawCircle {
                center: *center,
                radius: *radius,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawOval { rect, paint, transform } => Self::DrawOval {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawLine { p1, p2, paint, transform } => Self::DrawLine {
                p1: *p1,
                p2: *p2,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawPath { path, paint, transform } => Self::DrawPath {
                path: path.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawArc { rect, start_angle, sweep_angle, use_center, paint, transform } => Self::DrawArc {
                rect: *rect,
                start_angle: *start_angle,
                sweep_angle: *sweep_angle,
                use_center: *use_center,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawDRRect { outer, inner, paint, transform } => Self::DrawDRRect {
                outer: *outer,
                inner: *inner,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawPoints { mode, points, paint, transform } => Self::DrawPoints {
                mode: *mode,
                points: points.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawVertices { vertices, colors, tex_coords, indices, paint, transform } => Self::DrawVertices {
                vertices: vertices.clone(),
                colors: colors.clone(),
                tex_coords: tex_coords.clone(),
                indices: indices.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawText { text, offset, size, style, paint, transform } => Self::DrawText {
                text: text.clone(),
                offset: *offset,
                size: *size,
                style: style.clone(),
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::SaveLayer { bounds, paint, transform } => Self::SaveLayer {
                bounds: *bounds,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },

            // ─────────────────────────────────────────────────────────────────
            // Optional paint commands: Map over Option<Paint>
            // ─────────────────────────────────────────────────────────────────
            Self::DrawImage { image, dst, paint, transform } => Self::DrawImage {
                image: image.clone(),
                dst: *dst,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            Self::DrawImageRepeat { image, dst, repeat, paint, transform } => Self::DrawImageRepeat {
                image: image.clone(),
                dst: *dst,
                repeat: *repeat,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            Self::DrawImageNineSlice { image, center_slice, dst, paint, transform } => Self::DrawImageNineSlice {
                image: image.clone(),
                center_slice: *center_slice,
                dst: *dst,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            Self::DrawImageFiltered { image, dst, filter, paint, transform } => Self::DrawImageFiltered {
                image: image.clone(),
                dst: *dst,
                filter: *filter,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },
            Self::DrawAtlas { image, sprites, transforms, colors, blend_mode, paint, transform } => Self::DrawAtlas {
                image: image.clone(),
                sprites: sprites.clone(),
                transforms: transforms.clone(),
                colors: colors.clone(),
                blend_mode: *blend_mode,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
                transform: *transform,
            },

            // ─────────────────────────────────────────────────────────────────
            // Color commands: Apply opacity to color field
            // ─────────────────────────────────────────────────────────────────
            Self::DrawShadow { path, color, elevation, transform } => Self::DrawShadow {
                path: path.clone(),
                color: color.with_opacity(opacity),
                elevation: *elevation,
                transform: *transform,
            },
            Self::DrawColor { color, blend_mode, transform } => Self::DrawColor {
                color: color.with_opacity(opacity),
                blend_mode: *blend_mode,
                transform: *transform,
            },

            // ─────────────────────────────────────────────────────────────────
            // Child commands: Recursively apply opacity to DisplayList
            // ─────────────────────────────────────────────────────────────────
            Self::ShaderMask { child, shader, bounds, blend_mode, transform } => Self::ShaderMask {
                child: Box::new(child.to_opacity(opacity)),
                shader: shader.clone(),
                bounds: *bounds,
                blend_mode: *blend_mode,
                transform: *transform,
            },
            Self::BackdropFilter { child, filter, bounds, blend_mode, transform } => Self::BackdropFilter {
                child: child.as_ref().map(|c| Box::new(c.to_opacity(opacity))),
                filter: filter.clone(),
                bounds: *bounds,
                blend_mode: *blend_mode,
                transform: *transform,
            },

            // ─────────────────────────────────────────────────────────────────
            // Texture command: Multiply opacity field
            // ─────────────────────────────────────────────────────────────────
            Self::DrawTexture { texture_id, dst, src, filter_quality, opacity: tex_opacity, transform } => Self::DrawTexture {
                texture_id: *texture_id,
                dst: *dst,
                src: *src,
                filter_quality: *filter_quality,
                opacity: *tex_opacity * opacity,
                transform: *transform,
            },
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
                let size = Size::new(Pixels(effective_radius * 2.0), Pixels(effective_radius * 2.0));
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
            DrawCommand::DrawImageRepeat { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawImageNineSlice { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawImageFiltered { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawTexture { dst, transform, .. } => Some(transform.transform_rect(dst)),
            DrawCommand::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => {
                // Account for stroke width
                let stroke_half = paint.effective_stroke_width() * 0.5;
                let min_x = p1.x.0.min(p2.x.0) - stroke_half;
                let min_y = p1.y.0.min(p2.y.0) - stroke_half;
                let max_x = p1.x.0.max(p2.x.0) + stroke_half;
                let max_y = p1.y.0.max(p2.y.0) + stroke_half;
                let local_bounds = Rect::from_ltrb(min_x, min_y, max_x, max_y);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawPath {
                path,
                paint,
                transform,
            } => {
                // Use compute_bounds() which works with &self
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = path.compute_bounds().expand(outset);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawShadow {
                path,
                elevation,
                transform,
                ..
            } => {
                // Shadow extends beyond path by elevation amount
                let local_bounds = path.compute_bounds().expand(*elevation);
                Some(transform.transform_rect(&local_bounds))
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
            DrawCommand::DrawGradient {
                rect, transform, ..
            } => {
                // Gradient fills rect exactly
                Some(transform.transform_rect(rect))
            }
            DrawCommand::DrawGradientRRect {
                rrect, transform, ..
            } => {
                // Gradient fills rrect bounds
                Some(transform.transform_rect(&rrect.bounding_rect()))
            }
            DrawCommand::ShaderMask {
                bounds, transform, ..
            } => {
                // Return transformed bounds
                Some(transform.transform_rect(bounds))
            }
            DrawCommand::BackdropFilter {
                bounds, transform, ..
            } => {
                // Return transformed bounds
                Some(transform.transform_rect(bounds))
            }
            // Clipping and text don't contribute to bounds directly
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipPath { .. } => None,
            DrawCommand::DrawText {
                offset,
                size,
                transform,
                ..
            } => {
                let local_bounds = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawTextSpan { .. } => None, // TextSpan doesn't have pre-computed size

            // Layer commands - SaveLayer bounds if specified, RestoreLayer has no bounds
            DrawCommand::SaveLayer {
                bounds, transform, ..
            } => bounds.map(|b| transform.transform_rect(&b)),
            DrawCommand::RestoreLayer { .. } => None,
        }
    }

    // ===== Type Discrimination =====

    /// Returns the kind/category of this command.
    ///
    /// Useful for filtering, statistics, or debugging.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for cmd in display_list.commands() {
    ///     match cmd.kind() {
    ///         CommandKind::Draw => println!("Drawing command"),
    ///         CommandKind::Clip => println!("Clipping command"),
    ///         CommandKind::Effect => println!("Effect command"),
    ///         CommandKind::Layer => println!("Layer command"),
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn kind(&self) -> CommandKind {
        match self {
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipPath { .. } => CommandKind::Clip,

            DrawCommand::SaveLayer { .. } | DrawCommand::RestoreLayer { .. } => CommandKind::Layer,

            DrawCommand::ShaderMask { .. } | DrawCommand::BackdropFilter { .. } => {
                CommandKind::Effect
            }

            _ => CommandKind::Draw,
        }
    }

    // ===== Type Checking Methods =====

    /// Returns `true` if this is a clipping command.
    #[inline]
    pub fn is_clip(&self) -> bool {
        matches!(self.kind(), CommandKind::Clip)
    }

    /// Returns `true` if this is a drawing command (shapes, text, images).
    #[inline]
    pub fn is_draw(&self) -> bool {
        matches!(self.kind(), CommandKind::Draw)
    }

    /// Returns `true` if this is an effect command (shader mask, backdrop filter).
    #[inline]
    pub fn is_effect(&self) -> bool {
        matches!(self.kind(), CommandKind::Effect)
    }

    /// Returns `true` if this is a layer command (save/restore layer).
    #[inline]
    pub fn is_layer(&self) -> bool {
        matches!(self.kind(), CommandKind::Layer)
    }

    /// Returns `true` if this command draws a shape (rect, circle, path, etc).
    #[inline]
    pub fn is_shape(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawRect { .. }
                | DrawCommand::DrawRRect { .. }
                | DrawCommand::DrawCircle { .. }
                | DrawCommand::DrawOval { .. }
                | DrawCommand::DrawPath { .. }
                | DrawCommand::DrawArc { .. }
                | DrawCommand::DrawDRRect { .. }
                | DrawCommand::DrawLine { .. }
                | DrawCommand::DrawPoints { .. }
        )
    }

    /// Returns `true` if this command draws an image or texture.
    #[inline]
    pub fn is_image(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawImage { .. }
                | DrawCommand::DrawImageRepeat { .. }
                | DrawCommand::DrawImageNineSlice { .. }
                | DrawCommand::DrawImageFiltered { .. }
                | DrawCommand::DrawTexture { .. }
                | DrawCommand::DrawAtlas { .. }
        )
    }

    /// Returns `true` if this command draws text.
    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawText { .. } | DrawCommand::DrawTextSpan { .. }
        )
    }

    // ===== Accessor Methods =====

    /// Returns the transform matrix for this command.
    ///
    /// Every command stores the transform that was active when it was recorded.
    #[inline]
    pub fn transform(&self) -> Matrix4 {
        match self {
            DrawCommand::ClipRect { transform, .. }
            | DrawCommand::ClipRRect { transform, .. }
            | DrawCommand::ClipPath { transform, .. }
            | DrawCommand::DrawLine { transform, .. }
            | DrawCommand::DrawRect { transform, .. }
            | DrawCommand::DrawRRect { transform, .. }
            | DrawCommand::DrawCircle { transform, .. }
            | DrawCommand::DrawOval { transform, .. }
            | DrawCommand::DrawPath { transform, .. }
            | DrawCommand::DrawText { transform, .. }
            | DrawCommand::DrawTextSpan { transform, .. }
            | DrawCommand::DrawImage { transform, .. }
            | DrawCommand::DrawImageRepeat { transform, .. }
            | DrawCommand::DrawImageNineSlice { transform, .. }
            | DrawCommand::DrawImageFiltered { transform, .. }
            | DrawCommand::DrawTexture { transform, .. }
            | DrawCommand::DrawShadow { transform, .. }
            | DrawCommand::DrawGradient { transform, .. }
            | DrawCommand::DrawGradientRRect { transform, .. }
            | DrawCommand::ShaderMask { transform, .. }
            | DrawCommand::BackdropFilter { transform, .. }
            | DrawCommand::DrawArc { transform, .. }
            | DrawCommand::DrawDRRect { transform, .. }
            | DrawCommand::DrawPoints { transform, .. }
            | DrawCommand::DrawVertices { transform, .. }
            | DrawCommand::DrawColor { transform, .. }
            | DrawCommand::DrawAtlas { transform, .. }
            | DrawCommand::SaveLayer { transform, .. }
            | DrawCommand::RestoreLayer { transform, .. } => *transform,
        }
    }

    /// Returns a reference to the Paint for this command, if it has one.
    ///
    /// Clipping commands and some special commands don't have Paint.
    #[inline]
    pub fn paint(&self) -> Option<&Paint> {
        match self {
            DrawCommand::DrawLine { paint, .. }
            | DrawCommand::DrawRect { paint, .. }
            | DrawCommand::DrawRRect { paint, .. }
            | DrawCommand::DrawCircle { paint, .. }
            | DrawCommand::DrawOval { paint, .. }
            | DrawCommand::DrawPath { paint, .. }
            | DrawCommand::DrawText { paint, .. }
            | DrawCommand::DrawArc { paint, .. }
            | DrawCommand::DrawDRRect { paint, .. }
            | DrawCommand::DrawPoints { paint, .. }
            | DrawCommand::DrawVertices { paint, .. }
            | DrawCommand::SaveLayer { paint, .. } => Some(paint),

            DrawCommand::DrawImage { paint, .. }
            | DrawCommand::DrawImageRepeat { paint, .. }
            | DrawCommand::DrawImageNineSlice { paint, .. }
            | DrawCommand::DrawImageFiltered { paint, .. }
            | DrawCommand::DrawAtlas { paint, .. } => paint.as_ref(),

            _ => None,
        }
    }

    /// Returns `true` if this command has a Paint that can be modified.
    #[inline]
    pub fn has_paint(&self) -> bool {
        self.paint().is_some()
    }

    /// Returns a mutable reference to the transform matrix.
    ///
    /// Useful for transforming commands after recording.
    #[inline]
    pub fn transform_mut(&mut self) -> &mut Matrix4 {
        match self {
            DrawCommand::ClipRect { transform, .. }
            | DrawCommand::ClipRRect { transform, .. }
            | DrawCommand::ClipPath { transform, .. }
            | DrawCommand::DrawLine { transform, .. }
            | DrawCommand::DrawRect { transform, .. }
            | DrawCommand::DrawRRect { transform, .. }
            | DrawCommand::DrawCircle { transform, .. }
            | DrawCommand::DrawOval { transform, .. }
            | DrawCommand::DrawPath { transform, .. }
            | DrawCommand::DrawText { transform, .. }
            | DrawCommand::DrawTextSpan { transform, .. }
            | DrawCommand::DrawImage { transform, .. }
            | DrawCommand::DrawImageRepeat { transform, .. }
            | DrawCommand::DrawImageNineSlice { transform, .. }
            | DrawCommand::DrawImageFiltered { transform, .. }
            | DrawCommand::DrawTexture { transform, .. }
            | DrawCommand::DrawShadow { transform, .. }
            | DrawCommand::DrawGradient { transform, .. }
            | DrawCommand::DrawGradientRRect { transform, .. }
            | DrawCommand::ShaderMask { transform, .. }
            | DrawCommand::BackdropFilter { transform, .. }
            | DrawCommand::DrawArc { transform, .. }
            | DrawCommand::DrawDRRect { transform, .. }
            | DrawCommand::DrawPoints { transform, .. }
            | DrawCommand::DrawVertices { transform, .. }
            | DrawCommand::DrawColor { transform, .. }
            | DrawCommand::DrawAtlas { transform, .. }
            | DrawCommand::SaveLayer { transform, .. }
            | DrawCommand::RestoreLayer { transform, .. } => transform,
        }
    }

    /// Applies an additional transform to this command.
    ///
    /// The new transform is multiplied with the existing one.
    #[inline]
    pub fn apply_transform(&mut self, additional: Matrix4) {
        *self.transform_mut() = additional * self.transform();
    }
}

/// Categories of drawing commands.
///
/// Used by `DrawCommand::kind()` for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandKind {
    /// Drawing commands (shapes, text, images)
    Draw,
    /// Clipping commands
    Clip,
    /// Effect commands (shader mask, backdrop filter)
    Effect,
    /// Layer commands (save/restore layer)
    Layer,
}

// ===== AsRef / AsMut Implementations =====

/// Allow zero-cost conversion from DisplayList to slice of commands
impl AsRef<[DrawCommand]> for DisplayList {
    fn as_ref(&self) -> &[DrawCommand] {
        &self.commands
    }
}

/// Allow zero-cost mutable conversion from DisplayList to slice of commands
impl AsMut<[DrawCommand]> for DisplayList {
    fn as_mut(&mut self) -> &mut [DrawCommand] {
        &mut self.commands
    }
}

// ===== IntoIterator Implementation =====

/// Allow iterating over DisplayList directly with `for cmd in &display_list`
///
/// This provides ergonomic iteration over commands without needing to call `.commands()`.
///
/// # Examples
///
/// ```rust,ignore
/// let display_list = canvas.finish();
///
/// // Before: display_list.commands()
/// for cmd in display_list.commands() {
///     // process command
/// }
///
/// // After: just iterate directly (more ergonomic)
/// for cmd in &display_list {
///     // process command
/// }
/// ```
impl<'a> IntoIterator for &'a DisplayList {
    type Item = &'a DrawCommand;
    type IntoIter = std::slice::Iter<'a, DrawCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter()
    }
}

/// Allow iterating over DisplayList mutably with `for cmd in &mut display_list`
impl<'a> IntoIterator for &'a mut DisplayList {
    type Item = &'a mut DrawCommand;
    type IntoIter = std::slice::IterMut<'a, DrawCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter_mut()
    }
}

// ===== Index Trait Implementation =====

use std::ops::{Index, IndexMut};

/// Allow indexing into DisplayList to get commands by index
///
/// # Examples
///
/// ```rust,ignore
/// let display_list = canvas.finish();
/// let first_cmd = &display_list[0];  // Get first command
/// ```
impl Index<usize> for DisplayList {
    type Output = DrawCommand;

    fn index(&self, index: usize) -> &Self::Output {
        &self.commands[index]
    }
}

/// Allow mutable indexing into DisplayList
impl IndexMut<usize> for DisplayList {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.commands[index]
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
