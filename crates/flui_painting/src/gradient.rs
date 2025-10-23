//! Gradient painting implementation

use flui_types::{Rect, styling::{Gradient, LinearGradient, RadialGradient, SweepGradient}};

/// Painter for gradients
pub struct GradientPainter;

impl GradientPainter {
    /// Paint a gradient
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to paint the gradient in
    /// * `gradient` - The gradient to paint
    pub fn paint(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &Gradient,
    ) {
        match gradient {
            Gradient::Linear(linear) => Self::paint_linear(painter, rect, linear),
            Gradient::Radial(radial) => Self::paint_radial(painter, rect, radial),
            Gradient::Sweep(sweep) => Self::paint_sweep(painter, rect, sweep),
        }
    }

    /// Paint a linear gradient using colored mesh
    fn paint_linear(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &LinearGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // Single color - just fill with solid color
        if colors.len() == 1 {
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(rect.right(), rect.bottom()),
            );
            painter.rect(
                egui_rect,
                egui::CornerRadius::ZERO,
                colors[0],
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );
            return;
        }

        // Build mesh with colored vertices for multi-stop gradient
        let egui_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.bottom()),
        );

        // Determine gradient direction
        let is_horizontal = (gradient.begin.x - 0.0).abs() < 0.01
            && (gradient.end.x - 1.0).abs() < 0.01
            && (gradient.begin.y - gradient.end.y).abs() < 0.01;

        let is_vertical = (gradient.begin.y - 0.0).abs() < 0.01
            && (gradient.end.y - 1.0).abs() < 0.01
            && (gradient.begin.x - gradient.end.x).abs() < 0.01;

        // Number of segments for smooth gradient
        let num_segments = (colors.len() - 1).max(1) * 8; // 8 segments per color stop

        let mut mesh = egui::Mesh::default();
        mesh.reserve_triangles(num_segments * 2); // 2 triangles per segment
        mesh.reserve_vertices(num_segments * 2 + 2); // vertices along the gradient

        if is_horizontal {
            // Horizontal gradient
            Self::build_horizontal_gradient_mesh(
                &mut mesh,
                egui_rect,
                &colors,
                gradient.stops.as_deref(),
                num_segments,
            );
        } else if is_vertical {
            // Vertical gradient
            Self::build_vertical_gradient_mesh(
                &mut mesh,
                egui_rect,
                &colors,
                gradient.stops.as_deref(),
                num_segments,
            );
        } else {
            // Diagonal or custom angle gradient
            Self::build_angled_gradient_mesh(
                &mut mesh,
                egui_rect,
                &colors,
                gradient.stops.as_deref(),
                gradient.begin,
                gradient.end,
                num_segments,
            );
        }

        painter.add(egui::Shape::mesh(mesh));
    }

    /// Build horizontal gradient mesh
    fn build_horizontal_gradient_mesh(
        mesh: &mut egui::Mesh,
        rect: egui::Rect,
        colors: &[egui::Color32],
        stops: Option<&[f32]>,
        num_segments: usize,
    ) {
        let y_top = rect.top();
        let y_bottom = rect.bottom();
        let x_start = rect.left();
        let width = rect.width();

        for i in 0..=num_segments {
            let t = i as f32 / num_segments as f32;
            let x = x_start + width * t;

            // Interpolate color at position t
            let color = Self::interpolate_color(colors, stops, t);

            // Add two vertices (top and bottom)
            let idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(egui::pos2(x, y_top), color);
            mesh.colored_vertex(egui::pos2(x, y_bottom), color);

            // Add triangles (except for first column)
            if i > 0 {
                // Triangle 1: (prev_top, curr_top, prev_bottom)
                mesh.add_triangle(idx - 2, idx, idx - 1);
                // Triangle 2: (curr_top, curr_bottom, prev_bottom)
                mesh.add_triangle(idx, idx + 1, idx - 1);
            }
        }
    }

    /// Build vertical gradient mesh
    fn build_vertical_gradient_mesh(
        mesh: &mut egui::Mesh,
        rect: egui::Rect,
        colors: &[egui::Color32],
        stops: Option<&[f32]>,
        num_segments: usize,
    ) {
        let x_left = rect.left();
        let x_right = rect.right();
        let y_start = rect.top();
        let height = rect.height();

        for i in 0..=num_segments {
            let t = i as f32 / num_segments as f32;
            let y = y_start + height * t;

            // Interpolate color at position t
            let color = Self::interpolate_color(colors, stops, t);

            // Add two vertices (left and right)
            let idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(egui::pos2(x_left, y), color);
            mesh.colored_vertex(egui::pos2(x_right, y), color);

            // Add triangles (except for first row)
            if i > 0 {
                // Triangle 1: (prev_left, curr_left, prev_right)
                mesh.add_triangle(idx - 2, idx, idx - 1);
                // Triangle 2: (curr_left, curr_right, prev_right)
                mesh.add_triangle(idx, idx + 1, idx - 1);
            }
        }
    }

    /// Build angled gradient mesh
    fn build_angled_gradient_mesh(
        mesh: &mut egui::Mesh,
        rect: egui::Rect,
        colors: &[egui::Color32],
        stops: Option<&[f32]>,
        begin: flui_types::layout::Alignment,
        end: flui_types::layout::Alignment,
        num_segments: usize,
    ) {
        // Calculate gradient direction vector
        let start_x = rect.left() + rect.width() * begin.x;
        let start_y = rect.top() + rect.height() * begin.y;
        let end_x = rect.left() + rect.width() * end.x;
        let end_y = rect.top() + rect.height() * end.y;

        let dx = end_x - start_x;
        let dy = end_y - start_y;
        let length = (dx * dx + dy * dy).sqrt();

        if length < 0.001 {
            // Degenerate gradient - just fill with first color
            Self::build_horizontal_gradient_mesh(mesh, rect, &colors[0..1], None, 1);
            return;
        }

        // Perpendicular vector for gradient strips
        let perp_x = -dy / length;
        let perp_y = dx / length;

        // Use rect diagonal as perpendicular extent
        let perp_extent = (rect.width() * rect.width() + rect.height() * rect.height()).sqrt();

        for i in 0..=num_segments {
            let t = i as f32 / num_segments as f32;

            // Position along gradient direction
            let px = start_x + dx * t;
            let py = start_y + dy * t;

            // Interpolate color
            let color = Self::interpolate_color(colors, stops, t);

            // Create vertices perpendicular to gradient direction
            let idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(
                egui::pos2(px - perp_x * perp_extent, py - perp_y * perp_extent),
                color,
            );
            mesh.colored_vertex(
                egui::pos2(px + perp_x * perp_extent, py + perp_y * perp_extent),
                color,
            );

            // Add triangles
            if i > 0 {
                mesh.add_triangle(idx - 2, idx, idx - 1);
                mesh.add_triangle(idx, idx + 1, idx - 1);
            }
        }
    }

    /// Interpolate color at position t using gradient stops
    fn interpolate_color(
        colors: &[egui::Color32],
        stops: Option<&[f32]>,
        t: f32,
    ) -> egui::Color32 {
        if colors.is_empty() {
            return egui::Color32::TRANSPARENT;
        }

        if colors.len() == 1 {
            return colors[0];
        }

        // Use custom stops if provided, otherwise evenly distribute
        let stops_vec: Vec<f32> = if let Some(s) = stops {
            s.to_vec()
        } else {
            (0..colors.len())
                .map(|i| i as f32 / (colors.len() - 1) as f32)
                .collect()
        };

        // Find which color segment we're in
        for i in 0..colors.len() - 1 {
            let stop_start = stops_vec[i];
            let stop_end = stops_vec[i + 1];

            if t >= stop_start && t <= stop_end {
                // Interpolate between colors[i] and colors[i+1]
                let segment_t = if (stop_end - stop_start).abs() < 0.001 {
                    0.0
                } else {
                    (t - stop_start) / (stop_end - stop_start)
                };

                return Self::lerp_color32(colors[i], colors[i + 1], segment_t);
            }
        }

        // If t is beyond last stop, use last color
        *colors.last().unwrap()
    }

    /// Linear interpolation between two colors
    fn lerp_color32(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
        let t = t.clamp(0.0, 1.0);
        egui::Color32::from_rgba_unmultiplied(
            (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
            (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
            (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
            (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
        )
    }

    /// Paint a radial gradient using colored mesh
    fn paint_radial(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &RadialGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // Single color - just fill
        if colors.len() == 1 {
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(rect.right(), rect.bottom()),
            );
            painter.rect(
                egui_rect,
                egui::CornerRadius::ZERO,
                colors[0],
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );
            return;
        }

        // Calculate center position
        let center_x = rect.left() + rect.width() * gradient.center.x;
        let center_y = rect.top() + rect.height() * gradient.center.y;

        // Calculate radius (normalized to rect size)
        let max_dim = rect.width().max(rect.height());
        let radius = gradient.radius * max_dim;

        // Number of radial segments and angular segments
        let num_radial = (colors.len() - 1).max(1) * 8; // 8 segments per color stop
        let num_angular = 32; // 32 segments around the circle

        let mut mesh = egui::Mesh::default();
        mesh.reserve_triangles(num_radial * num_angular * 2);
        mesh.reserve_vertices((num_radial + 1) * num_angular + 1);

        // Center vertex
        let center_color = Self::interpolate_color(&colors, gradient.stops.as_deref(), 0.0);
        mesh.colored_vertex(egui::pos2(center_x, center_y), center_color);
        let center_idx = 0u32;

        // Build concentric circles
        for ring in 0..=num_radial {
            let t = ring as f32 / num_radial as f32;
            let r = radius * t;
            let color = Self::interpolate_color(&colors, gradient.stops.as_deref(), t);

            // Add vertices around this ring
            for seg in 0..num_angular {
                let angle = (seg as f32 / num_angular as f32) * std::f32::consts::TAU;
                let x = center_x + r * angle.cos();
                let y = center_y + r * angle.sin();

                mesh.colored_vertex(egui::pos2(x, y), color);
            }
        }

        // Create triangles
        // First ring connects to center
        for seg in 0..num_angular {
            let next_seg = (seg + 1) % num_angular;
            let v1 = center_idx;
            let v2 = 1 + seg as u32;
            let v3 = 1 + next_seg as u32;
            mesh.add_triangle(v1, v2, v3);
        }

        // Remaining rings connect to previous ring
        for ring in 1..=num_radial {
            let prev_ring_start = (1 + (ring - 1) * num_angular) as u32;
            let curr_ring_start = (1 + ring * num_angular) as u32;

            for seg in 0..num_angular {
                let next_seg = (seg + 1) % num_angular;

                let prev_v1 = prev_ring_start + seg as u32;
                let prev_v2 = prev_ring_start + next_seg as u32;
                let curr_v1 = curr_ring_start + seg as u32;
                let curr_v2 = curr_ring_start + next_seg as u32;

                // Two triangles per segment
                mesh.add_triangle(prev_v1, curr_v1, prev_v2);
                mesh.add_triangle(curr_v1, curr_v2, prev_v2);
            }
        }

        painter.add(egui::Shape::mesh(mesh));
    }

    /// Paint a sweep gradient (conical gradient) using colored mesh
    fn paint_sweep(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &SweepGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // Single color - just fill
        if colors.len() == 1 {
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(rect.right(), rect.bottom()),
            );
            painter.rect(
                egui_rect,
                egui::CornerRadius::ZERO,
                colors[0],
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );
            return;
        }

        // Calculate center position
        let center_x = rect.left() + rect.width() * gradient.center.x;
        let center_y = rect.top() + rect.height() * gradient.center.y;

        // Calculate radius to cover the whole rect
        let radius = ((rect.width() * rect.width() + rect.height() * rect.height()) / 4.0).sqrt()
            * 2.0; // Ensure it covers corners

        // Number of angular segments
        let num_segments = (colors.len() - 1).max(1) * 16; // 16 segments per color stop

        let mut mesh = egui::Mesh::default();
        mesh.reserve_triangles(num_segments);
        mesh.reserve_vertices(num_segments + 1);

        // Center vertex (use average color)
        let center_color = Self::interpolate_color(&colors, gradient.stops.as_deref(), 0.5);
        mesh.colored_vertex(egui::pos2(center_x, center_y), center_color);
        let center_idx = 0u32;

        // Create vertices around the circle
        let start_angle = gradient.start_angle;
        let end_angle = gradient.end_angle;
        let angle_range = end_angle - start_angle;

        for seg in 0..=num_segments {
            let t = seg as f32 / num_segments as f32;
            let angle = start_angle + angle_range * t;

            // Wrap t for color interpolation (sweep gradients typically wrap)
            let color_t = t % 1.0;
            let color = Self::interpolate_color(&colors, gradient.stops.as_deref(), color_t);

            let x = center_x + radius * angle.cos();
            let y = center_y + radius * angle.sin();

            mesh.colored_vertex(egui::pos2(x, y), color);
        }

        // Create triangles connecting center to edge
        for seg in 0..num_segments {
            let v1 = center_idx;
            let v2 = 1 + seg as u32;
            let v3 = 1 + (seg + 1) as u32;
            mesh.add_triangle(v1, v2, v3);
        }

        painter.add(egui::Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Color;

    #[test]
    fn test_linear_gradient_horizontal() {
        // Test horizontal gradient
        let gradient = LinearGradient::horizontal(vec![Color::RED, Color::BLUE]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::RED);
        assert_eq!(gradient.colors[1], Color::BLUE);
    }

    #[test]
    fn test_linear_gradient_vertical() {
        // Test vertical gradient
        let gradient = LinearGradient::vertical(vec![Color::GREEN, Color::YELLOW]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::GREEN);
        assert_eq!(gradient.colors[1], Color::YELLOW);
    }

    #[test]
    fn test_linear_gradient_multi_stop() {
        // Test gradient with multiple colors
        let colors = vec![Color::RED, Color::GREEN, Color::BLUE, Color::YELLOW];
        let gradient = LinearGradient::horizontal(colors.clone());
        assert_eq!(gradient.colors.len(), 4);
        assert_eq!(gradient.colors, colors);
    }

    #[test]
    fn test_radial_gradient() {
        // Test radial gradient
        let gradient = RadialGradient::centered(1.0, vec![Color::WHITE, Color::BLACK]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::WHITE);
        assert_eq!(gradient.colors[1], Color::BLACK);
    }

    #[test]
    fn test_sweep_gradient() {
        // Test sweep gradient
        let gradient = SweepGradient::centered(vec![Color::RED, Color::GREEN, Color::BLUE]);
        assert_eq!(gradient.colors.len(), 3);
        assert_eq!(gradient.colors[0], Color::RED);
        assert_eq!(gradient.colors[1], Color::GREEN);
        assert_eq!(gradient.colors[2], Color::BLUE);
    }

    #[test]
    fn test_gradient_enum_linear() {
        // Test Gradient enum with linear
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

        match gradient {
            Gradient::Linear(linear) => {
                assert_eq!(linear.colors.len(), 2);
            }
            _ => panic!("Expected linear gradient"),
        }
    }

    #[test]
    fn test_gradient_enum_radial() {
        // Test Gradient enum with radial
        let gradient = Gradient::Radial(RadialGradient::centered(
            1.0,
            vec![Color::WHITE, Color::BLACK],
        ));

        match gradient {
            Gradient::Radial(radial) => {
                assert_eq!(radial.colors.len(), 2);
            }
            _ => panic!("Expected radial gradient"),
        }
    }

    #[test]
    fn test_gradient_enum_sweep() {
        // Test Gradient enum with sweep
        let gradient = Gradient::Sweep(SweepGradient::centered(vec![
            Color::RED,
            Color::GREEN,
            Color::BLUE,
        ]));

        match gradient {
            Gradient::Sweep(sweep) => {
                assert_eq!(sweep.colors.len(), 3);
            }
            _ => panic!("Expected sweep gradient"),
        }
    }

    #[test]
    fn test_gradient_single_color() {
        // Test gradient with single color (should still work)
        let gradient = LinearGradient::horizontal(vec![Color::RED]);
        assert_eq!(gradient.colors.len(), 1);
        assert_eq!(gradient.colors[0], Color::RED);
    }

    #[test]
    fn test_gradient_empty_colors() {
        // Test gradient with no colors (edge case)
        let gradient = LinearGradient::horizontal(vec![]);
        assert_eq!(gradient.colors.len(), 0);
    }

    #[test]
    fn test_color_interpolation() {
        // Test that we can create gradients with various colors
        let colors = vec![
            Color::BLACK,
            Color::WHITE,
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::YELLOW,
            Color::TRANSPARENT,
        ];

        for color in colors {
            let gradient = LinearGradient::horizontal(vec![color, Color::WHITE]);
            assert_eq!(gradient.colors[0], color);
        }
    }

    #[test]
    fn test_rect_for_gradient() {
        // Test that rects work properly with gradients
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 150.0);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 150.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 170.0);
    }
}
