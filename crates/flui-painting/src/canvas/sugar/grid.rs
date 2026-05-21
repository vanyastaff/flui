//! Grid and repeat-pattern helpers.
//!
//! Each method here delegates to [`Canvas::with_translate`]
//! (see [`crate::canvas::scoped`]) for the per-cell save/restore
//! discipline — closures may invoke any drawing operation on the
//! borrowed canvas and the transform is unwound at scope exit.

impl crate::canvas::Canvas {
    /// Draws a grid of items using a closure.
    pub fn draw_grid<F>(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        f: F,
    ) where
        F: Fn(&mut Self, usize, usize),
    {
        for row in 0..rows {
            for col in 0..cols {
                self.with_translate(col as f32 * cell_width, row as f32 * cell_height, |c| {
                    f(c, col, row);
                });
            }
        }
    }

    /// Repeats a drawing operation in a horizontal line.
    pub fn repeat_x<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(i as f32 * spacing, 0.0, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation in a vertical line.
    pub fn repeat_y<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(0.0, i as f32 * spacing, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation around a circle.
    pub fn repeat_radial<F>(&mut self, count: usize, radius: f32, f: F)
    where
        F: Fn(&mut Self, usize, f32),
    {
        use std::f32::consts::PI;
        let angle_step = 2.0 * PI / count as f32;

        for i in 0..count {
            let angle = i as f32 * angle_step;
            let x = angle.cos() * radius;
            let y = angle.sin() * radius;

            self.with_translate(x, y, |c| {
                f(c, i, angle);
            });
        }
    }
}
