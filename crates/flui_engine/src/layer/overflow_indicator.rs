//! OverflowIndicatorLayer - Debug-only layer that wraps content with overflow visualization
//!
//! This layer automatically adds diagonal stripe indicators when content overflows
//! its container. Zero cost in release builds - completely compiled out.

#[cfg(debug_assertions)]
use crate::layer::picture::{DrawCommand, PictureLayer};
#[cfg(debug_assertions)]
use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
#[cfg(debug_assertions)]
use crate::painter::{Paint, Painter};
#[cfg(debug_assertions)]
use flui_types::events::{Event, HitTestResult};
#[cfg(debug_assertions)]
use flui_types::painting::PaintingStyle;
#[cfg(debug_assertions)]
use flui_types::{Color, Offset, Point, Rect, Size};

/// Layer that adds overflow indicators around overflowing content (debug only)
///
/// This creates Flutter-style overflow indicators with:
/// - 45° diagonal red/yellow warning stripes (like warning tape)
/// - Red border around the overflow area
/// - Clipped to show only where content actually overflows
///
/// # Example
///
/// ```rust,ignore
/// #[cfg(debug_assertions)]
/// let layer = if overflow_detected {
///     let indicator = OverflowIndicatorLayer::new(child_layer)
///         .with_overflow(overflow_h, overflow_v, container_size);
///     Box::new(indicator)
/// } else {
///     child_layer
/// };
/// ```
#[cfg(debug_assertions)]
pub struct OverflowIndicatorLayer {
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

    /// Horizontal overflow in pixels
    overflow_h: f32,

    /// Vertical overflow in pixels
    overflow_v: f32,

    /// Container size (for positioning indicators)
    container_size: Size,

    /// Offset for positioning (from RenderFlex.paint offset parameter)
    offset: Offset,
}

#[cfg(debug_assertions)]
impl OverflowIndicatorLayer {
    /// Create a new overflow indicator layer
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to wrap with indicators
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            base: SingleChildLayerBase::new(child),
            overflow_h: 0.0,
            overflow_v: 0.0,
            container_size: Size::ZERO,
            offset: Offset::ZERO,
        }
    }

    /// Set overflow amounts, container size, and offset
    #[must_use]
    pub fn with_overflow(
        mut self,
        overflow_h: f32,
        overflow_v: f32,
        container_size: Size,
        offset: Offset,
    ) -> Self {
        self.overflow_h = overflow_h.max(0.0);
        self.overflow_v = overflow_v.max(0.0);
        self.container_size = container_size;
        self.offset = offset;
        self
    }

    /// Paint diagonal stripes directly to painter (warning tape pattern)
    fn paint_diagonal_stripes_direct(painter: &mut dyn Painter, rect: Rect) {
        // Flutter-style diagonal stripes
        const STRIPE_SPACING: f32 = 12.0; // Расстояние между полосками (как ты сказал!)
        const STRIPE_WIDTH: f32 = 4.0; // Тонкие полоски (как ты сказал!)
        const BG_COLOR: Color = Color {
            r: 255,
            g: 235,
            b: 59,
            a: 100,
        }; // Желтый #FFEB3B
        const STRIPE_COLOR: Color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 100,
        }; // Черный

        // Контроль угла полосок:
        // 1.0 = -45° (диагональ /)
        // 0.5 = более пологий (~-27°)
        // 0.73 = -36°
        // 1.5 = более крутой (~-56°)
        const ANGLE_FACTOR: f32 = 1.0; // Текущий угол

        // Fill background with yellow
        let bg_paint = Paint {
            color: BG_COLOR,
            style: PaintingStyle::Fill,
            anti_alias: false,
            ..Default::default()
        };
        painter.rect(rect, &bg_paint);

        // Draw diagonal red stripes as thick lines
        let stripe_paint = Paint::stroke(STRIPE_COLOR)
            .with_stroke(crate::painter::Stroke::new(STRIPE_WIDTH))
            .with_anti_alias(false);

        let left = rect.min.x;
        let right = rect.max.x;
        let top = rect.min.y;
        let bottom = rect.max.y;
        let width = rect.width();
        let height = rect.height();

        // For -45° diagonal stripes (top-right to bottom-left, like Flutter)
        // Calculate diagonal distance and number of stripes needed
        let diagonal_distance = width + height;
        let num_stripes = ((diagonal_distance / STRIPE_SPACING).ceil() as i32) * 2;

        // Start from well before the rectangle to ensure full coverage
        // Нужно начать с отрицательной позиции чтобы покрыть верхний правый угол
        let start_offset = -(width + height);

        // Draw diagonal stripes (angle controlled by ANGLE_FACTOR)
        for i in 0..num_stripes {
            let offset = start_offset + (i as f32) * STRIPE_SPACING;

            // Calculate line endpoints for constant angle
            // Для постоянного угла: horizontal_shift = height * tan(angle)
            // ANGLE_FACTOR = tan(angle в радианах)
            let horizontal_shift = height * ANGLE_FACTOR;

            let x_start = right + offset;
            let x_end = x_start - horizontal_shift; // Смещаем на постоянное расстояние

            let p1 = Point::new(x_start, top); // Верх справа
            let p2 = Point::new(x_end, bottom); // Низ левее

            // Draw if line intersects with rect (более мягкое условие)
            // Линия видна если хотя бы одна точка в пределах rect
            if x_start >= left || x_end >= left {
                painter.line(p1, p2, &stripe_paint);
            }
        }
    }

    /// Paint red border directly to painter
    fn paint_border_direct(painter: &mut dyn Painter, rect: Rect) {
        const BORDER_COLOR: Color = Color {
            r: 211,
            g: 47,
            b: 47,
            a: 255,
        };

        let border_paint = Paint::stroke(BORDER_COLOR)
            .with_stroke(crate::painter::Stroke::new(3.0))
            .with_anti_alias(true);

        painter.rect(rect, &border_paint);
    }

    /// Paint overflow text label directly to painter (like Flutter)
    fn paint_overflow_text(
        painter: &mut dyn Painter,
        rect: Rect,
        overflow_h: f32,
        overflow_v: f32,
        is_horizontal: bool,
    ) {
        // Определяем текст в зависимости от направления overflow
        let text = if is_horizontal {
            format!("RIGHT OVERFLOWED BY {:.0} PIXELS", overflow_h)
        } else {
            format!("BOTTOM OVERFLOWED BY {:.0} PIXELS", overflow_v)
        };

        // Настройки текста как в Flutter
        const TEXT_COLOR: Color = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }; // Красный текст
        const TEXT_SIZE: f32 = 10.0; // Мелкий шрифт

        let text_paint = Paint {
            color: TEXT_COLOR,
            style: PaintingStyle::Fill,
            anti_alias: true,
            ..Default::default()
        };

        // Позиционируем текст по центру прямоугольника
        let text_x = rect.min.x + (rect.width() / 2.0) - (text.len() as f32 * TEXT_SIZE / 3.0);
        let text_y = rect.min.y + (rect.height() / 2.0) + (TEXT_SIZE / 2.0);
        let position = Point::new(text_x, text_y);

        painter.text(&text, position, TEXT_SIZE, &text_paint);
    }

    /// Paint 45° diagonal stripes (warning tape pattern) - PictureLayer version
    #[allow(dead_code)]
    fn paint_diagonal_stripes(picture: &mut PictureLayer, rect: Rect) {
        const STRIPE_WIDTH: f32 = 1.0;
        const RED: Color = Color {
            r: 211,
            g: 47,
            b: 47,
            a: 255,
        };
        const YELLOW: Color = Color {
            r: 255,
            g: 193,
            b: 7,
            a: 255,
        };

        let paint_red = Paint {
            color: RED,
            style: PaintingStyle::Fill,
            anti_alias: true,
            ..Default::default()
        };

        let paint_yellow = Paint {
            color: YELLOW,
            style: PaintingStyle::Fill,
            anti_alias: true,
            ..Default::default()
        };

        let left = rect.min.x;
        let right = rect.max.x;
        let top = rect.min.y;
        let bottom = rect.max.y;
        let width = rect.width();
        let height = rect.height();

        // For 45° diagonal stripes, we need to cover from top-left to bottom-right
        // The diagonal distance is sqrt(width² + height²), but at 45° we can use width + height
        let diagonal_span = width + height;

        // Start from far enough left to cover the entire rectangle
        // At 45°, we need to start at (left - height) to cover the top-left corner
        let start_offset = -height;

        // Calculate how many stripes we need to cover the entire diagonal span
        let num_stripes = ((diagonal_span + height) / (STRIPE_WIDTH * 2.0)).ceil() as i32 + 2; // Extra stripes for safety

        // Draw diagonal stripes at 45° angle
        for i in 0..num_stripes {
            let stripe_offset = start_offset + (i as f32 * STRIPE_WIDTH * 2.0);
            let is_red = i % 2 == 0;
            let paint = if is_red { &paint_red } else { &paint_yellow };

            // Create a parallelogram for each stripe
            // At 45°, moving right by 1 means moving down by 1
            // Points form a parallelogram: top-left, top-right, bottom-right, bottom-left
            let points = vec![
                Point::new(left + stripe_offset, top),
                Point::new(left + stripe_offset + STRIPE_WIDTH, top),
                Point::new(right + stripe_offset + STRIPE_WIDTH, bottom),
                Point::new(right + stripe_offset, bottom),
            ];

            picture.add_command(DrawCommand::Polygon {
                points: std::sync::Arc::new(points),
                paint: paint.clone(),
            });
        }
    }

    /// Paint red border around overflow region
    #[allow(dead_code)]
    fn paint_border(picture: &mut PictureLayer, rect: Rect) {
        const BORDER_COLOR: Color = Color {
            r: 211,
            g: 47,
            b: 47,
            a: 255,
        };

        let border_paint = Paint::stroke(BORDER_COLOR)
            .with_stroke(crate::painter::Stroke::new(3.0))
            .with_anti_alias(true);

        picture.add_command(DrawCommand::Rect {
            rect,
            paint: border_paint,
        });
    }

    /// Paint overflow region when both axes overflow (L-shaped area)
    #[allow(dead_code)]
    fn paint_overflow_region(
        picture: &mut PictureLayer,
        overflow_h: f32,
        overflow_v: f32,
        container_size: Size,
        offset: Offset,
    ) {
        // Right side overflow (vertical strip)
        let right_rect = Rect::from_ltrb(
            offset.dx + container_size.width,
            offset.dy,
            offset.dx + container_size.width + overflow_h,
            offset.dy + container_size.height,
        );
        Self::paint_diagonal_stripes(picture, right_rect);
        Self::paint_border(picture, right_rect);

        // Bottom overflow (horizontal strip)
        let bottom_rect = Rect::from_ltrb(
            offset.dx,
            offset.dy + container_size.height,
            offset.dx + container_size.width,
            offset.dy + container_size.height + overflow_v,
        );
        Self::paint_diagonal_stripes(picture, bottom_rect);
        Self::paint_border(picture, bottom_rect);

        // Corner overflow (where both meet)
        let corner_rect = Rect::from_ltrb(
            offset.dx + container_size.width,
            offset.dy + container_size.height,
            offset.dx + container_size.width + overflow_h,
            offset.dy + container_size.height + overflow_v,
        );
        Self::paint_diagonal_stripes(picture, corner_rect);
        Self::paint_border(picture, corner_rect);
    }
}

#[cfg(debug_assertions)]
impl Layer for OverflowIndicatorLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        let Some(child) = self.base.child() else {
            return;
        };

        // Paint child first
        child.paint(painter);

        // Early exit if no overflow
        if self.overflow_h <= 0.0 && self.overflow_v <= 0.0 {
            return;
        }

        // Use the offset from RenderFlex.paint() to position overflow regions correctly
        let offset = self.offset;

        // Paint overflow region(s) with diagonal stripes (with clipping)
        if self.overflow_h > 0.0 && self.overflow_v > 0.0 {
            // Both axes overflow - paint three regions (right, bottom, corner)

            // Right side overflow
            let right_rect = Rect::from_ltrb(
                offset.dx + self.container_size.width,
                offset.dy,
                offset.dx + self.container_size.width + self.overflow_h,
                offset.dy + self.container_size.height,
            );
            painter.save();
            painter.clip_rect(right_rect);
            Self::paint_diagonal_stripes_direct(painter, right_rect);
            Self::paint_border_direct(painter, right_rect);
            painter.restore();
            // Текст поверх right overflow
            Self::paint_overflow_text(painter, right_rect, self.overflow_h, self.overflow_v, true);

            // Bottom overflow
            let bottom_rect = Rect::from_ltrb(
                offset.dx,
                offset.dy + self.container_size.height,
                offset.dx + self.container_size.width,
                offset.dy + self.container_size.height + self.overflow_v,
            );
            painter.save();
            painter.clip_rect(bottom_rect);
            Self::paint_diagonal_stripes_direct(painter, bottom_rect);
            Self::paint_border_direct(painter, bottom_rect);
            painter.restore();
            // Текст поверх bottom overflow
            Self::paint_overflow_text(
                painter,
                bottom_rect,
                self.overflow_h,
                self.overflow_v,
                false,
            );

            // Corner overflow
            let corner_rect = Rect::from_ltrb(
                offset.dx + self.container_size.width,
                offset.dy + self.container_size.height,
                offset.dx + self.container_size.width + self.overflow_h,
                offset.dy + self.container_size.height + self.overflow_v,
            );
            painter.save();
            painter.clip_rect(corner_rect);
            Self::paint_diagonal_stripes_direct(painter, corner_rect);
            Self::paint_border_direct(painter, corner_rect);
            painter.restore();
        } else if self.overflow_h > 0.0 {
            // Horizontal overflow only - paint right side
            let overflow_rect = Rect::from_ltrb(
                offset.dx + self.container_size.width,
                offset.dy,
                offset.dx + self.container_size.width + self.overflow_h,
                offset.dy + self.container_size.height,
            );
            painter.save();
            painter.clip_rect(overflow_rect);
            Self::paint_diagonal_stripes_direct(painter, overflow_rect);
            Self::paint_border_direct(painter, overflow_rect);
            painter.restore();
            // Текст для horizontal overflow
            Self::paint_overflow_text(
                painter,
                overflow_rect,
                self.overflow_h,
                self.overflow_v,
                true,
            );
        } else {
            // Vertical overflow only - paint bottom
            let overflow_rect = Rect::from_ltrb(
                offset.dx,
                offset.dy + self.container_size.height,
                offset.dx + self.container_size.width,
                offset.dy + self.container_size.height + self.overflow_v,
            );
            painter.save();
            painter.clip_rect(overflow_rect);
            Self::paint_diagonal_stripes_direct(painter, overflow_rect);
            Self::paint_border_direct(painter, overflow_rect);
            painter.restore();
            // Текст для vertical overflow
            Self::paint_overflow_text(
                painter,
                overflow_rect,
                self.overflow_h,
                self.overflow_v,
                false,
            );
        }
    }

    fn bounds(&self) -> Rect {
        // Bounds must include both the container and overflow regions
        let child_bounds = self.base.child_bounds();

        // Expand bounds to include overflow regions
        let right = child_bounds.min.x + self.container_size.width + self.overflow_h;
        let bottom = child_bounds.min.y + self.container_size.height + self.overflow_v;

        Rect::from_ltrb(child_bounds.min.x, child_bounds.min.y, right, bottom)
    }

    fn is_visible(&self) -> bool {
        self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.base.child_hit_test(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.child_handle_event(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_child();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn mark_needs_paint(&mut self) {
        if let Some(child) = self.base.child_mut() {
            child.mark_needs_paint();
        }
    }
}
