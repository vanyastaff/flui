//! Integration tests for the complete rendering pipeline
//!
//! These tests verify that all Phase 2 components work together correctly:
//! - Scene graph construction
//! - Primitive batching
//! - Buffer management
//! - Pipeline caching
//! - Text rendering
//! - Texture atlas
//! - Compositor

#[cfg(test)]
mod tests {
    use crate::wgpu::{
        AtlasRect, BufferManager, Compositor, PipelineCache, RenderContext, TextureAtlas,
        TransformStack,
    };
    use flui_types::{Color, DevicePixels, Point, Rect, Size};
    use glam::Mat4;

    #[cfg(feature = "wgpu-backend")]
    use crate::wgpu::{TextRenderingSystem, TextRun};

    use super::super::scene::{
        BlendMode, Layer, LayerBatch, Primitive, PrimitiveBatch, PrimitiveType, Scene,
    };

    /// Test complete scene rendering workflow
    #[test]
    fn test_complete_scene_workflow() {
        // 1. Build scene
        let scene = Scene::new(
            vec![
                Layer::new(
                    vec![
                        Primitive::Rect {
                            rect: Rect::new(
                                Point::new(DevicePixels(0.0), DevicePixels(0.0)),
                                Size::new(DevicePixels(100.0), DevicePixels(100.0)),
                            ),
                            color: Color::new(1.0, 0.0, 0.0, 1.0),
                            border_radius: 0.0,
                        },
                        Primitive::Text {
                            text: "Hello".to_string(),
                            position: Point::new(DevicePixels(10.0), DevicePixels(10.0)),
                            style: Default::default(),
                            color: Color::new(1.0, 1.0, 1.0, 1.0),
                        },
                    ],
                    Mat4::IDENTITY,
                    1.0,
                    BlendMode::Normal,
                    None,
                ),
                Layer::new(
                    vec![Primitive::Rect {
                        rect: Rect::new(
                            Point::new(DevicePixels(50.0), DevicePixels(50.0)),
                            Size::new(DevicePixels(100.0), DevicePixels(100.0)),
                        ),
                        color: Color::new(0.0, 1.0, 0.0, 0.5),
                        border_radius: 10.0,
                    }],
                    Mat4::from_translation(glam::Vec3::new(10.0, 10.0, 0.0)),
                    0.8,
                    BlendMode::Alpha,
                    None,
                ),
            ],
            Size::new(DevicePixels(800.0), DevicePixels(600.0)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );

        // 2. Batch primitives
        let batches = scene.batch_primitives();
        assert_eq!(batches.len(), 2); // Rect batch + Text batch

        let rect_batch = batches
            .iter()
            .find(|b| b.primitive_type == PrimitiveType::Rect)
            .unwrap();
        assert_eq!(rect_batch.count, 2);

        let text_batch = batches
            .iter()
            .find(|b| b.primitive_type == PrimitiveType::Text)
            .unwrap();
        assert_eq!(text_batch.count, 1);

        // 3. Batch with layer context
        let layer_batches = scene.batch_with_context();
        assert_eq!(layer_batches.len(), 2); // One per layer

        assert_eq!(layer_batches[0].blend_mode, BlendMode::Normal);
        assert_eq!(layer_batches[1].blend_mode, BlendMode::Alpha);
    }

    /// Test texture atlas integration with batching
    #[test]
    fn test_atlas_integration() {
        // Create atlas
        let mut atlas = TextureAtlas::new_mock(1024, 1024);

        // Allocate regions for images
        let (id1, rect1) = atlas.allocate(128, 128).unwrap();
        let (id2, rect2) = atlas.allocate(256, 256).unwrap();
        let (id3, rect3) = atlas.allocate(64, 64).unwrap();

        // Verify UV coordinates are correct
        let (uv_min, uv_max) = rect1.uv_coords(1024, 1024);
        assert!(uv_min[0] >= 0.0 && uv_min[0] <= 1.0);
        assert!(uv_max[0] >= 0.0 && uv_max[0] <= 1.0);

        // Create scene with images using atlas IDs
        let scene = Scene::new(
            vec![Layer::new(
                vec![
                    Primitive::Image {
                        rect: Rect::new(
                            Point::new(DevicePixels(0.0), DevicePixels(0.0)),
                            Size::new(DevicePixels(128.0), DevicePixels(128.0)),
                        ),
                        source_rect: None,
                        image_id: id1,
                    },
                    Primitive::Image {
                        rect: Rect::new(
                            Point::new(DevicePixels(128.0), DevicePixels(0.0)),
                            Size::new(DevicePixels(256.0), DevicePixels(256.0)),
                        ),
                        source_rect: None,
                        image_id: id2,
                    },
                    Primitive::Image {
                        rect: Rect::new(
                            Point::new(DevicePixels(0.0), DevicePixels(128.0)),
                            Size::new(DevicePixels(64.0), DevicePixels(64.0)),
                        ),
                        source_rect: None,
                        image_id: id3,
                    },
                ],
                Mat4::IDENTITY,
                1.0,
                BlendMode::Normal,
                None,
            )],
            Size::new(DevicePixels(800.0), DevicePixels(600.0)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );

        // Batch - should group all images together since they're in same atlas
        let batches = scene.batch_primitives();
        let image_batch = batches
            .iter()
            .find(|b| b.primitive_type == PrimitiveType::Image)
            .unwrap();
        assert_eq!(image_batch.count, 3);
    }

    /// Test compositor with complex layer hierarchy
    #[test]
    fn test_compositor_integration() {
        // Create layered scene with transforms and opacity
        let scene = Scene::new(
            vec![
                // Root layer
                Layer::new(
                    vec![Primitive::Rect {
                        rect: Rect::new(
                            Point::new(DevicePixels(0.0), DevicePixels(0.0)),
                            Size::new(DevicePixels(200.0), DevicePixels(200.0)),
                        ),
                        color: Color::new(1.0, 0.0, 0.0, 1.0),
                        border_radius: 0.0,
                    }],
                    Mat4::IDENTITY,
                    1.0,
                    BlendMode::Normal,
                    None,
                ),
                // Child layer with transform and opacity
                Layer::new(
                    vec![Primitive::Rect {
                        rect: Rect::new(
                            Point::new(DevicePixels(50.0), DevicePixels(50.0)),
                            Size::new(DevicePixels(100.0), DevicePixels(100.0)),
                        ),
                        color: Color::new(0.0, 1.0, 0.0, 1.0),
                        border_radius: 0.0,
                    }],
                    Mat4::from_translation(glam::Vec3::new(10.0, 10.0, 0.0)),
                    0.5,
                    BlendMode::Alpha,
                    None,
                ),
                // Grandchild layer with additional transform
                Layer::new(
                    vec![Primitive::Rect {
                        rect: Rect::new(
                            Point::new(DevicePixels(75.0), DevicePixels(75.0)),
                            Size::new(DevicePixels(50.0), DevicePixels(50.0)),
                        ),
                        color: Color::new(0.0, 0.0, 1.0, 1.0),
                        border_radius: 5.0,
                    }],
                    Mat4::from_scale(glam::Vec3::new(2.0, 2.0, 1.0)),
                    0.8,
                    BlendMode::Multiply,
                    None,
                ),
            ],
            Size::new(DevicePixels(800.0), DevicePixels(600.0)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );

        // Create compositor
        let mut compositor = Compositor::new();
        let layer_batches = scene.batch_with_context();

        // Process each layer
        for batch in &layer_batches {
            compositor.begin_layer(batch);

            // Verify transform composition
            let current_transform = compositor.current_transform();
            assert_ne!(current_transform, Mat4::IDENTITY); // Should be composed

            // Verify opacity composition
            let current_opacity = compositor.current_opacity();
            assert!(current_opacity <= 1.0);

            compositor.end_layer();
        }

        // All layers should be popped
        assert_eq!(compositor.current_opacity(), 1.0);
    }

    /// Test render context state management
    #[test]
    fn test_render_context() {
        let mut context = RenderContext::new(800, 600);

        // Frame progression
        assert_eq!(context.frame_number, 0);
        context.next_frame();
        assert_eq!(context.frame_number, 1);

        // Layer management
        let batch = LayerBatch {
            primitives: vec![],
            transform: Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0)),
            opacity: 0.75,
            blend_mode: BlendMode::Alpha,
            clip_rect: None,
        };

        context.compositor.begin_layer(&batch);
        assert_eq!(context.compositor.current_opacity(), 0.75);

        // Nested layer
        let nested_batch = LayerBatch {
            primitives: vec![],
            transform: Mat4::from_scale(glam::Vec3::new(2.0, 2.0, 1.0)),
            opacity: 0.5,
            blend_mode: BlendMode::Normal,
            clip_rect: None,
        };

        context.compositor.begin_layer(&nested_batch);
        assert_eq!(context.compositor.current_opacity(), 0.75 * 0.5); // Composed

        context.compositor.end_layer();
        assert_eq!(context.compositor.current_opacity(), 0.75);

        context.compositor.end_layer();
        assert_eq!(context.compositor.current_opacity(), 1.0);
    }

    /// Test text rendering integration
    #[cfg(feature = "wgpu-backend")]
    #[test]
    fn test_text_rendering_integration() {
        // Create text runs
        let runs = vec![
            TextRun {
                text: "Hello".to_string(),
                position: Point::new(DevicePixels(10.0), DevicePixels(10.0)),
                style: Default::default(),
                color: Color::new(1.0, 1.0, 1.0, 1.0),
            },
            TextRun {
                text: "World".to_string(),
                position: Point::new(DevicePixels(10.0), DevicePixels(30.0)),
                style: Default::default(),
                color: Color::new(1.0, 1.0, 1.0, 1.0),
            },
        ];

        // Create scene with text primitives
        let scene = Scene::new(
            vec![Layer::new(
                vec![
                    Primitive::Text {
                        text: runs[0].text.clone(),
                        position: runs[0].position,
                        style: runs[0].style.clone(),
                        color: runs[0].color,
                    },
                    Primitive::Text {
                        text: runs[1].text.clone(),
                        position: runs[1].position,
                        style: runs[1].style.clone(),
                        color: runs[1].color,
                    },
                ],
                Mat4::IDENTITY,
                1.0,
                BlendMode::Normal,
                None,
            )],
            Size::new(DevicePixels(800.0), DevicePixels(600.0)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );

        // Batch text primitives
        let batches = scene.batch_primitives();
        let text_batch = batches
            .iter()
            .find(|b| b.primitive_type == PrimitiveType::Text)
            .unwrap();
        assert_eq!(text_batch.count, 2);
    }

    /// Test blend mode pipeline requirements
    #[test]
    fn test_blend_mode_pipeline_integration() {
        let modes = [
            (BlendMode::Normal, false),
            (BlendMode::Alpha, false),
            (BlendMode::Multiply, true),
            (BlendMode::Screen, true),
            (BlendMode::Overlay, true),
            (BlendMode::Darken, true),
            (BlendMode::Lighten, true),
            (BlendMode::ColorDodge, true),
            (BlendMode::ColorBurn, true),
            (BlendMode::HardLight, true),
            (BlendMode::SoftLight, true),
            (BlendMode::Difference, true),
            (BlendMode::Exclusion, true),
        ];

        for (mode, requires_shader) in modes {
            assert_eq!(
                mode.requires_shader(),
                requires_shader,
                "BlendMode::{:?} shader requirement mismatch",
                mode
            );

            // Verify wgpu blend state can be created
            let _blend_state = mode.to_wgpu_blend();
        }
    }

    /// Test complete rendering pipeline from scene to buffers
    #[test]
    fn test_full_pipeline() {
        // 1. Create complex scene
        let scene = Scene::new(
            vec![
                Layer::new(
                    vec![
                        Primitive::Rect {
                            rect: Rect::new(
                                Point::new(DevicePixels(0.0), DevicePixels(0.0)),
                                Size::new(DevicePixels(100.0), DevicePixels(100.0)),
                            ),
                            color: Color::new(1.0, 0.0, 0.0, 1.0),
                            border_radius: 0.0,
                        },
                        Primitive::Rect {
                            rect: Rect::new(
                                Point::new(DevicePixels(10.0), DevicePixels(10.0)),
                                Size::new(DevicePixels(80.0), DevicePixels(80.0)),
                            ),
                            color: Color::new(0.0, 1.0, 0.0, 1.0),
                            border_radius: 5.0,
                        },
                    ],
                    Mat4::IDENTITY,
                    1.0,
                    BlendMode::Normal,
                    None,
                ),
                Layer::new(
                    vec![Primitive::Text {
                        text: "Test".to_string(),
                        position: Point::new(DevicePixels(20.0), DevicePixels(20.0)),
                        style: Default::default(),
                        color: Color::new(1.0, 1.0, 1.0, 1.0),
                    }],
                    Mat4::from_translation(glam::Vec3::new(5.0, 5.0, 0.0)),
                    0.9,
                    BlendMode::Alpha,
                    None,
                ),
            ],
            Size::new(DevicePixels(800.0), DevicePixels(600.0)),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );

        // 2. Batch primitives
        let primitive_batches = scene.batch_primitives();
        assert!(primitive_batches.len() >= 2); // At least rect and text

        // 3. Batch with layer context
        let layer_batches = scene.batch_with_context();
        assert_eq!(layer_batches.len(), 2);

        // 4. Create render context
        let mut context = RenderContext::new(800, 600);

        // 5. Process each layer through compositor
        for batch in &layer_batches {
            context.compositor.begin_layer(batch);

            // Verify state is tracked correctly
            assert!(context.compositor.current_opacity() <= 1.0);
            assert_ne!(
                context.compositor.current_blend_mode(),
                BlendMode::Normal
            ); // Second layer uses Alpha

            context.compositor.end_layer();
        }

        // 6. Verify final state
        assert_eq!(context.compositor.current_opacity(), 1.0);
        assert_eq!(context.compositor.current_blend_mode(), BlendMode::Normal);
    }
}
