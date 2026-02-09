//! Integration tests for tessellation module

#[cfg(feature = "tessellation")]
mod tessellation_tests {
    use flui_painting::tessellation::{tessellate_fill, tessellate_stroke, TessellationOptions};
    use flui_types::geometry::{px, Point, Rect};
    use flui_types::painting::Path;

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
    fn test_tessellate_polygon() {
        let points = vec![
            Point::new(px(0.0), px(0.0)),
            Point::new(px(100.0), px(0.0)),
            Point::new(px(100.0), px(100.0)),
            Point::new(px(0.0), px(100.0)),
        ];
        let path = Path::polygon(&points);

        // Debug: print path commands
        println!("Path commands:");
        for (i, cmd) in path.commands().iter().enumerate() {
            println!("  {}: {:?}", i, cmd);
        }

        let result = tessellate_fill(&path, &TessellationOptions::default()).unwrap();

        assert!(!result.is_empty());
        assert!(result.triangle_count() >= 2);
        println!(
            "Polygon fill: {} vertices, {} triangles",
            result.vertices.len(),
            result.triangle_count()
        );
    }
}
