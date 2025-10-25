//! Surface abstraction for rendering targets
//!
//! A Surface represents a rendering target - typically a window or offscreen buffer.
//! It provides a Frame for each rendered frame, which gives access to a Painter.

use crate::painter::Painter;
use flui_types::Size;

/// A rendering surface (window, buffer, etc.)
///
/// The Surface trait abstracts over different rendering targets. Each frame,
/// the compositor calls `begin_frame()` to get a Frame, paints to it, and
/// then calls `present()` to show the result.
///
/// # Lifecycle
///
/// ```text
/// 1. begin_frame() -> Frame
/// 2. Frame.painter() -> Painter
/// 3. Paint layers using Painter
/// 4. present() -> Show frame on screen
/// ```
///
/// # Example
///
/// ```rust,ignore
/// let mut frame = surface.begin_frame();
/// let painter = frame.painter();
///
/// // Paint content
/// scene.paint(painter);
///
/// drop(frame);
/// surface.present();
/// ```
pub trait Surface: Send {
    /// Get the size of this surface in logical pixels
    fn size(&self) -> Size;

    /// Begin a new frame
    ///
    /// This returns a Frame that provides access to a Painter for rendering.
    /// The frame must be dropped before calling `present()`.
    fn begin_frame(&mut self) -> Box<dyn Frame + '_>;

    /// Present the current frame to the screen
    ///
    /// This should be called after the Frame is dropped.
    fn present(&mut self);

    /// Resize the surface
    ///
    /// This is called when the window or buffer is resized.
    fn resize(&mut self, new_size: Size);

    /// Check if the surface is valid and ready for rendering
    fn is_valid(&self) -> bool {
        let size = self.size();
        size.width > 0.0 && size.height > 0.0
    }
}

/// A single frame of rendering
///
/// The Frame provides access to a Painter for drawing. It manages the
/// painter's lifecycle and ensures proper cleanup.
pub trait Frame: Send {
    /// Get the painter for this frame
    ///
    /// The painter is valid until the Frame is dropped.
    fn painter(&mut self) -> &mut dyn Painter;

    /// Get the size of this frame
    fn size(&self) -> Size;
}

/// A simple in-memory surface for testing
///
/// This surface doesn't actually render anything - it just tracks calls
/// and provides a mock painter.
#[cfg(test)]
pub struct TestSurface {
    size: Size,
    frame_count: usize,
}

#[cfg(test)]
impl TestSurface {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            frame_count: 0,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }
}

#[cfg(test)]
struct TestFrame<'a> {
    surface: &'a mut TestSurface,
    painter: TestPainter,
}

#[cfg(test)]
struct TestPainter;

#[cfg(test)]
impl Painter for TestPainter {
    fn rect(&mut self, _rect: flui_types::Rect, _paint: &crate::painter::Paint) {}
    fn rrect(&mut self, _rrect: crate::painter::RRect, _paint: &crate::painter::Paint) {}
    fn circle(&mut self, _center: flui_types::Point, _radius: f32, _paint: &crate::painter::Paint) {}
    fn line(&mut self, _p1: flui_types::Point, _p2: flui_types::Point, _paint: &crate::painter::Paint) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: flui_types::Offset) {}
    fn rotate(&mut self, _angle: f32) {}
    fn scale(&mut self, _sx: f32, _sy: f32) {}
    fn clip_rect(&mut self, _rect: flui_types::Rect) {}
    fn clip_rrect(&mut self, _rrect: crate::painter::RRect) {}
    fn set_opacity(&mut self, _opacity: f32) {}
}

#[cfg(test)]
impl Surface for TestSurface {
    fn size(&self) -> Size {
        self.size
    }

    fn begin_frame(&mut self) -> Box<dyn Frame + '_> {
        Box::new(TestFrame {
            surface: self,
            painter: TestPainter,
        })
    }

    fn present(&mut self) {
        self.frame_count += 1;
    }

    fn resize(&mut self, new_size: Size) {
        self.size = new_size;
    }
}

#[cfg(test)]
impl<'a> Frame for TestFrame<'a> {
    fn painter(&mut self) -> &mut dyn Painter {
        &mut self.painter
    }

    fn size(&self) -> Size {
        self.surface.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_creation() {
        let surface = TestSurface::new(Size::new(800.0, 600.0));
        assert_eq!(surface.size(), Size::new(800.0, 600.0));
        assert!(surface.is_valid());
    }

    #[test]
    fn test_surface_frame() {
        let mut surface = TestSurface::new(Size::new(800.0, 600.0));

        {
            let _frame = surface.begin_frame();
            // Frame is active
        }

        surface.present();
        assert_eq!(surface.frame_count(), 1);
    }

    #[test]
    fn test_surface_resize() {
        let mut surface = TestSurface::new(Size::new(800.0, 600.0));
        surface.resize(Size::new(1024.0, 768.0));
        assert_eq!(surface.size(), Size::new(1024.0, 768.0));
    }

    #[test]
    fn test_invalid_surface() {
        let surface = TestSurface::new(Size::ZERO);
        assert!(!surface.is_valid());
    }
}
