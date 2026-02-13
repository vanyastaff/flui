//! Path tessellation using Lyon
//!
//! This module provides GPU-ready triangle tessellation for vector paths.
//! It converts Path commands into vertex buffers suitable for GPU rendering.
//!
//! # Architecture
//!
//! ```text
//! Path (flui_types) → tessellate() → TessellatedPath → GPU vertex buffers
//! ```
//!
//! # Features
//!
//! - **Fill tessellation**: Converts filled paths to triangles
//! - **Stroke tessellation**: Converts stroked paths to triangle strips
//! - **Configurable quality**: Tolerance parameter controls triangle count
//! - **GPU-ready output**: Vertices compatible with wgpu/Metal/Vulkan
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_painting::tessellation::{tessellate_fill, tessellate_stroke, TessellationOptions};
//! use flui_types::painting::Path;
//!
//! // Create a path
//! let path = Path::circle(Point::ZERO, 50.0);
//!
//! // Tessellate for fill
//! let fill_result = tessellate_fill(&path, &TessellationOptions::default())?;
//! println!("Fill: {} vertices, {} indices",
//!          fill_result.vertices.len(),
//!          fill_result.indices.len());
//!
//! // Tessellate for stroke
//! let stroke_result = tessellate_stroke(&path, 2.0, &TessellationOptions::default())?;
//! println!("Stroke: {} vertices, {} indices",
//!          stroke_result.vertices.len(),
//!          stroke_result.indices.len());
//! ```

#[cfg(feature = "tessellation")]
use lyon::lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};
#[cfg(feature = "tessellation")]
use lyon::path::Path as LyonPath;

use flui_types::painting::{Path, PathCommand};

// ============================================================================
// PUBLIC API
// ============================================================================

/// A vertex in the tessellated geometry.
///
/// This is a simple 2D vertex with position only. For more complex rendering
/// (textures, colors), extend this type or create your own vertex builder.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TessellationVertex {
    /// Position in pixels
    pub position: [f32; 2],
}

/// Result of tessellation containing vertices and indices.
///
/// The indices form triangles (every 3 indices = 1 triangle).
#[derive(Clone, Debug, Default)]
pub struct TessellatedPath {
    /// Vertex positions
    pub vertices: Vec<TessellationVertex>,

    /// Triangle indices (every 3 indices = 1 triangle)
    pub indices: Vec<u32>,
}

impl TessellatedPath {
    /// Creates an empty tessellated path.
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Returns the number of triangles in this tessellation.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns true if the tessellation is empty.
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Clears all vertices and indices.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }
}

/// Options for path tessellation.
#[derive(Clone, Debug)]
pub struct TessellationOptions {
    /// Tolerance for curve approximation (smaller = more triangles, higher quality).
    ///
    /// Default: 0.1 pixels
    pub tolerance: f32,

    /// Whether to use anti-aliasing (generates additional edge vertices).
    ///
    /// Default: false
    pub anti_alias: bool,
}

impl Default for TessellationOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            anti_alias: false,
        }
    }
}

impl TessellationOptions {
    /// Creates options with custom tolerance.
    pub fn with_tolerance(tolerance: f32) -> Self {
        Self {
            tolerance,
            ..Default::default()
        }
    }

    /// Enables anti-aliasing.
    pub fn with_anti_alias(mut self, enabled: bool) -> Self {
        self.anti_alias = enabled;
        self
    }
}

// ============================================================================
// TESSELLATION FUNCTIONS
// ============================================================================

/// Tessellates a filled path into triangles.
///
/// Converts the path into a triangle mesh suitable for GPU rendering with a fill shader.
///
/// # Arguments
///
/// * `path` - The path to tessellate
/// * `options` - Tessellation quality options
///
/// # Returns
///
/// A `TessellatedPath` containing vertices and indices, or an error if tessellation fails.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::tessellation::{tessellate_fill, TessellationOptions};
/// use flui_types::painting::Path;
/// use flui_types::geometry::{Point, px};
///
/// let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
/// let result = tessellate_fill(&path, &TessellationOptions::default())?;
///
/// println!("Generated {} triangles", result.triangle_count());
/// ```
#[cfg(feature = "tessellation")]
pub fn tessellate_fill(
    path: &Path,
    options: &TessellationOptions,
) -> Result<TessellatedPath, TessellationError> {
    // Convert Path to lyon::Path
    let lyon_path = path_to_lyon(path)?;

    // Create tessellator
    let mut tessellator = FillTessellator::new();

    // Create output buffers
    let mut geometry: VertexBuffers<TessellationVertex, u32> = VertexBuffers::new();

    // Configure fill options
    let fill_options = FillOptions::default().with_tolerance(options.tolerance);

    // Tessellate
    tessellator
        .tessellate_path(
            &lyon_path,
            &fill_options,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex<'_>| TessellationVertex {
                position: vertex.position().to_array(),
            }),
        )
        .map_err(|e| TessellationError::FillError(format!("{:?}", e)))?;

    Ok(TessellatedPath {
        vertices: geometry.vertices,
        indices: geometry.indices,
    })
}

/// Tessellates a stroked path into triangles.
///
/// Converts the path outline into a triangle mesh suitable for GPU rendering with a stroke shader.
///
/// # Arguments
///
/// * `path` - The path to tessellate
/// * `stroke_width` - Width of the stroke in pixels
/// * `options` - Tessellation quality options
///
/// # Returns
///
/// A `TessellatedPath` containing vertices and indices, or an error if tessellation fails.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::tessellation::{tessellate_stroke, TessellationOptions};
/// use flui_types::painting::Path;
/// use flui_types::geometry::{Point, px};
///
/// let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
/// let result = tessellate_stroke(&path, 2.0, &TessellationOptions::default())?;
///
/// println!("Generated {} triangles for stroke", result.triangle_count());
/// ```
#[cfg(feature = "tessellation")]
pub fn tessellate_stroke(
    path: &Path,
    stroke_width: f32,
    options: &TessellationOptions,
) -> Result<TessellatedPath, TessellationError> {
    // Convert Path to lyon::Path
    let lyon_path = path_to_lyon(path)?;

    // Create tessellator
    let mut tessellator = StrokeTessellator::new();

    // Create output buffers
    let mut geometry: VertexBuffers<TessellationVertex, u32> = VertexBuffers::new();

    // Configure stroke options
    let stroke_options = StrokeOptions::default()
        .with_line_width(stroke_width)
        .with_tolerance(options.tolerance);

    // Tessellate
    tessellator
        .tessellate_path(
            &lyon_path,
            &stroke_options,
            &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex<'_, '_>| {
                TessellationVertex {
                    position: vertex.position().to_array(),
                }
            }),
        )
        .map_err(|e| TessellationError::StrokeError(format!("{:?}", e)))?;

    Ok(TessellatedPath {
        vertices: geometry.vertices,
        indices: geometry.indices,
    })
}

// ============================================================================
// PATH CONVERSION
// ============================================================================

/// Converts a FLUI Path to a Lyon Path.
#[cfg(feature = "tessellation")]
fn path_to_lyon(path: &Path) -> Result<LyonPath, TessellationError> {
    use lyon::geom::euclid::Point2D;
    use lyon::path::Winding;

    let mut builder = LyonPath::builder();
    let mut path_started = false;

    for cmd in path.commands() {
        match cmd {
            PathCommand::MoveTo(p) => {
                if path_started {
                    builder.end(false);
                }
                builder.begin(Point2D::new(p.x.0, p.y.0));
                path_started = true;
            }
            PathCommand::LineTo(p) => {
                if !path_started {
                    builder.begin(Point2D::new(p.x.0, p.y.0));
                    path_started = true;
                } else {
                    builder.line_to(Point2D::new(p.x.0, p.y.0));
                }
            }
            PathCommand::QuadraticTo(ctrl, to) => {
                if !path_started {
                    builder.begin(Point2D::new(ctrl.x.0, ctrl.y.0));
                    path_started = true;
                }
                builder.quadratic_bezier_to(
                    Point2D::new(ctrl.x.0, ctrl.y.0),
                    Point2D::new(to.x.0, to.y.0),
                );
            }
            PathCommand::CubicTo(ctrl1, ctrl2, to) => {
                if !path_started {
                    builder.begin(Point2D::new(ctrl1.x.0, ctrl1.y.0));
                    path_started = true;
                }
                builder.cubic_bezier_to(
                    Point2D::new(ctrl1.x.0, ctrl1.y.0),
                    Point2D::new(ctrl2.x.0, ctrl2.y.0),
                    Point2D::new(to.x.0, to.y.0),
                );
            }
            PathCommand::Close => {
                if path_started {
                    builder.end(true); // true = close the path
                    path_started = false;
                }
            }
            PathCommand::AddRect(rect) => {
                if path_started {
                    builder.end(false);
                    path_started = false;
                }
                builder.add_rectangle(
                    &lyon::geom::Box2D::new(
                        Point2D::new(rect.min_x().0, rect.min_y().0),
                        Point2D::new(rect.max_x().0, rect.max_y().0),
                    ),
                    Winding::Positive,
                );
            }
            PathCommand::AddCircle(center, radius) => {
                if path_started {
                    builder.end(false);
                    path_started = false;
                }
                builder.add_circle(
                    Point2D::new(center.x.0, center.y.0),
                    *radius,
                    Winding::Positive,
                );
            }
            PathCommand::AddOval(rect) => {
                if path_started {
                    builder.end(false);
                    path_started = false;
                }
                builder.add_ellipse(
                    Point2D::new(rect.center().x.0, rect.center().y.0),
                    lyon::math::Vector::new(rect.width().0 / 2.0, rect.height().0 / 2.0),
                    lyon::math::Angle::zero(),
                    Winding::Positive,
                );
            }
            PathCommand::AddArc(rect, start_angle, _sweep_angle) => {
                if path_started {
                    builder.end(false);
                    path_started = false;
                }
                // For arcs, we approximate with an ellipse segment
                // Lyon doesn't have add_ellipse_arc, so we use the full ellipse for now
                // TODO: Implement proper arc tessellation using arc_to or manual bezier curves
                let center = rect.center();
                let radii = lyon::math::Vector::new(rect.width().0 / 2.0, rect.height().0 / 2.0);

                builder.add_ellipse(
                    Point2D::new(center.x.0, center.y.0),
                    radii,
                    lyon::math::Angle::radians(*start_angle),
                    Winding::Positive,
                );
            }
        }
    }

    if path_started {
        builder.end(false);
    }

    Ok(builder.build())
}

// ============================================================================
// ERRORS
// ============================================================================

/// Errors that can occur during tessellation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TessellationError {
    /// Error during fill tessellation.
    #[error("Fill tessellation failed: {0}")]
    FillError(String),

    /// Error during stroke tessellation.
    #[error("Stroke tessellation failed: {0}")]
    StrokeError(String),

    /// Invalid path (e.g., empty, malformed).
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

// ============================================================================
// STUB IMPLEMENTATIONS (when tessellation feature is disabled)
// ============================================================================

#[cfg(not(feature = "tessellation"))]
pub fn tessellate_fill(
    _path: &Path,
    _options: &TessellationOptions,
) -> Result<TessellatedPath, TessellationError> {
    Err(TessellationError::InvalidPath(
        "Tessellation feature not enabled. Enable the 'tessellation' feature.".to_string(),
    ))
}

#[cfg(not(feature = "tessellation"))]
pub fn tessellate_stroke(
    _path: &Path,
    _stroke_width: f32,
    _options: &TessellationOptions,
) -> Result<TessellatedPath, TessellationError> {
    Err(TessellationError::InvalidPath(
        "Tessellation feature not enabled. Enable the 'tessellation' feature.".to_string(),
    ))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(all(test, feature = "tessellation"))]
mod tests {
    use super::*;
    use flui_types::geometry::{px, Rect};
    use flui_types::Point;

    #[test]
    fn test_tessellate_fill_circle() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
        let result = tessellate_fill(&path, &TessellationOptions::default()).unwrap();

        assert!(!result.is_empty());
        assert!(result.triangle_count() > 0);
        println!(
            "Circle fill: {} vertices, {} triangles",
            result.vertices.len(),
            result.triangle_count()
        );
    }

    #[test]
    fn test_tessellate_stroke_circle() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
        let result = tessellate_stroke(&path, 2.0, &TessellationOptions::default()).unwrap();

        assert!(!result.is_empty());
        assert!(result.triangle_count() > 0);
        println!(
            "Circle stroke: {} vertices, {} triangles",
            result.vertices.len(),
            result.triangle_count()
        );
    }

    #[test]
    fn test_tessellate_fill_rect() {
        let path = Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let result = tessellate_fill(&path, &TessellationOptions::default()).unwrap();

        assert!(!result.is_empty());
        // Rectangle should produce 2 triangles (4 vertices, 6 indices)
        assert_eq!(result.triangle_count(), 2);
        println!(
            "Rectangle fill: {} vertices, {} triangles",
            result.vertices.len(),
            result.triangle_count()
        );
    }

    #[test]
    fn test_tessellate_tolerance() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);

        // High tolerance = fewer triangles
        let low_quality =
            tessellate_fill(&path, &TessellationOptions::with_tolerance(1.0)).unwrap();

        // Low tolerance = more triangles
        let high_quality =
            tessellate_fill(&path, &TessellationOptions::with_tolerance(0.01)).unwrap();

        assert!(low_quality.triangle_count() < high_quality.triangle_count());
        println!(
            "Low quality: {} triangles, High quality: {} triangles",
            low_quality.triangle_count(),
            high_quality.triangle_count()
        );
    }

    #[test]
    fn test_tessellated_path_empty() {
        let mut tessellated = TessellatedPath::new();
        assert!(tessellated.is_empty());
        assert_eq!(tessellated.triangle_count(), 0);

        tessellated.vertices.push(TessellationVertex {
            position: [0.0, 0.0],
        });
        tessellated.indices.extend_from_slice(&[0, 1, 2]);

        assert!(!tessellated.is_empty());
        assert_eq!(tessellated.triangle_count(), 1);

        tessellated.clear();
        assert!(tessellated.is_empty());
    }
}
