//! A display list's fingerprint: a hash of what its commands draw, read from
//! their values, never from a text form of them.
//!
//! Inspection asks it to tell two pictures of one length and bounds apart, so
//! it covers every value a command draws with: its transform, its op's fields,
//! the payload the op names (by content, not by position), and the paint,
//! path, image and paragraph behind them. Equal within one build of the
//! program, not across builds.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem;

use flui_foundation::geometry::{Matrix4, Offset, Point, RRect, RSuperellipse, Radius, Rect, Size};

use super::command::DrawOp;
use super::payload::{ImageRegionPlacement, NestedRRects, StoredPayload};
use super::{DisplayList, ShapedParagraph};
use crate::paint::image::ColorFilter;
use crate::paint::{DashPattern, Image, ImageShader, Paint, Shader};
use crate::styling::Color;

impl DisplayList {
    /// A hash of what the list's commands draw, so two lists of one length
    /// and bounds that draw differently read apart in inspection.
    ///
    /// Equal for lists whose commands hold the same values bit for bit (so
    /// `0.0` and `-0.0` read apart), wherever each list
    /// keeps its payloads; equal within one build of the program, not across
    /// builds. A path that has not built its curves builds them here.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut fp = Fingerprint(DefaultHasher::new());
        for command in self {
            fp.matrix(command.transform);
            fp.op(command.op);
            match command.op.payload_index() {
                Some(index) => {
                    fp.tag(&Some(()));
                    fp.stored(command.payloads.get(index));
                }
                None => fp.tag(&None::<()>),
            }
        }
        fp.0.finish()
    }
}

/// The hasher a fingerprint is taken with, and how each value feeds it.
pub(crate) struct Fingerprint(DefaultHasher);

impl Fingerprint {
    pub(crate) fn f64(&mut self, value: f64) {
        self.0.write_u64(value.to_bits());
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.0.write_u32(value.to_bits());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.0.write_u64(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.0.write_u8(u8::from(value));
    }

    /// Which variant `value` is: all an enum without fields holds, and the
    /// first word of one with fields, whose fields the caller feeds apart:
    /// an enum fed by its tag alone is one without fields.
    pub(crate) fn tag<T>(&mut self, value: &T) {
        mem::discriminant(value).hash(&mut self.0);
    }

    pub(crate) fn len(&mut self, len: usize) {
        self.0.write_usize(len);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.len(bytes.len());
        self.0.write(bytes);
    }

    pub(crate) fn point(&mut self, point: Point<f64>) {
        self.f64(point.x);
        self.f64(point.y);
    }

    fn points(&mut self, points: &[Point<f64>]) {
        self.len(points.len());
        for &point in points {
            self.point(point);
        }
    }

    fn offset(&mut self, offset: Offset<f64>) {
        self.f64(offset.dx);
        self.f64(offset.dy);
    }

    fn size(&mut self, size: Size<f64>) {
        self.f64(size.width);
        self.f64(size.height);
    }

    pub(crate) fn rect(&mut self, rect: &Rect<f64>) {
        self.point(rect.min);
        self.point(rect.max);
    }

    fn option_rect(&mut self, rect: Option<&Rect<f64>>) {
        self.tag(&rect);
        if let Some(rect) = rect {
            self.rect(rect);
        }
    }

    fn radius(&mut self, radius: Radius<f64>) {
        self.f64(radius.x);
        self.f64(radius.y);
    }

    pub(crate) fn rrect(&mut self, rrect: &RRect) {
        self.rect(&rrect.rect);
        self.radius(rrect.top_left);
        self.radius(rrect.top_right);
        self.radius(rrect.bottom_right);
        self.radius(rrect.bottom_left);
    }

    fn rsuperellipse(&mut self, rsuperellipse: &RSuperellipse) {
        self.rect(&rsuperellipse.outer_rect());
        self.radius(rsuperellipse.tl_radius());
        self.radius(rsuperellipse.tr_radius());
        self.radius(rsuperellipse.br_radius());
        self.radius(rsuperellipse.bl_radius());
    }

    fn matrix(&mut self, matrix: &Matrix4) {
        for &value in &matrix.m {
            self.f64(value);
        }
    }

    fn color(&mut self, color: Color) {
        for value in color.to_rgba_f32_array() {
            self.f32(value);
        }
    }

    fn colors(&mut self, colors: Option<&[Color]>) {
        self.tag(&colors);
        if let Some(colors) = colors {
            self.len(colors.len());
            for &color in colors {
                self.color(color);
            }
        }
    }

    fn f64s(&mut self, values: &[f64]) {
        self.len(values.len());
        for &value in values {
            self.f64(value);
        }
    }

    fn paint(&mut self, paint: &Paint) {
        let Paint {
            style,
            color,
            stroke_width,
            stroke_cap,
            stroke_join,
            blend_mode,
            anti_alias,
            shader,
            dash_pattern,
        } = paint;
        self.tag(style);
        self.color(*color);
        self.f64(*stroke_width);
        self.tag(stroke_cap);
        self.tag(stroke_join);
        self.tag(blend_mode);
        self.bool(*anti_alias);
        self.tag(shader);
        if let Some(shader) = shader {
            self.shader(shader);
        }
        self.tag(dash_pattern);
        if let Some(DashPattern { intervals, phase }) = dash_pattern {
            self.f64s(intervals);
            self.f64(*phase);
        }
    }

    fn option_paint(&mut self, paint: Option<&Paint>) {
        self.tag(&paint);
        if let Some(paint) = paint {
            self.paint(paint);
        }
    }

    fn gradient(&mut self, colors: &[Color], stops: Option<&[f64]>) {
        self.colors(Some(colors));
        self.tag(&stops);
        if let Some(stops) = stops {
            self.f64s(stops);
        }
    }

    fn shader(&mut self, shader: &Shader) {
        self.tag(shader);
        match shader {
            Shader::LinearGradient {
                from,
                to,
                colors,
                stops,
                tile_mode,
            } => {
                self.offset(*from);
                self.offset(*to);
                self.gradient(colors, stops.as_deref());
                self.tag(tile_mode);
            }
            Shader::RadialGradient {
                center,
                radius,
                colors,
                stops,
                tile_mode,
                focal,
                focal_radius,
            } => {
                self.offset(*center);
                self.f64(*radius);
                self.gradient(colors, stops.as_deref());
                self.tag(tile_mode);
                self.tag(focal);
                if let Some(focal) = focal {
                    self.offset(*focal);
                }
                self.tag(focal_radius);
                if let Some(focal_radius) = focal_radius {
                    self.f64(*focal_radius);
                }
            }
            Shader::SweepGradient {
                center,
                colors,
                stops,
                tile_mode,
                start_angle,
                end_angle,
            } => {
                self.offset(*center);
                self.gradient(colors, stops.as_deref());
                self.tag(tile_mode);
                self.f64(*start_angle);
                self.f64(*end_angle);
            }
            Shader::Solid { color } => self.color(*color),
            Shader::Image(ImageShader {
                tile_mode_x,
                tile_mode_y,
                transform,
                filter_quality,
            }) => {
                self.tag(tile_mode_x);
                self.tag(tile_mode_y);
                self.tag(transform);
                if let Some(transform) = transform {
                    for value in transform.iter().flatten() {
                        self.f64(*value);
                    }
                }
                self.tag(filter_quality);
                if let Some(filter_quality) = filter_quality {
                    self.tag(filter_quality);
                }
            }
        }
    }

    fn image(&mut self, image: &Image) {
        self.0.write_u32(image.width());
        self.0.write_u32(image.height());
        self.bytes(image.data());
    }

    fn color_filter(&mut self, filter: &ColorFilter) {
        self.tag(filter);
        match filter {
            ColorFilter::Mode { color, blend_mode } => {
                self.color(*color);
                self.tag(blend_mode);
            }
            ColorFilter::Matrix(matrix) => {
                for &value in &matrix.values {
                    self.f32(value);
                }
            }
            ColorFilter::LinearToSrgbGamma | ColorFilter::SrgbToLinearGamma => {}
        }
    }

    fn paragraph(&mut self, paragraph: &ShapedParagraph) {
        self.bytes(paragraph.text.as_bytes());
        self.size(paragraph.size);
        self.len(paragraph.baselines.len());
        for &baseline in &paragraph.baselines {
            self.f32(baseline);
        }
        self.len(paragraph.faces.len());
        for face in &paragraph.faces {
            face.key().hash(&mut self.0);
        }
        self.len(paragraph.coords.len());
        for &coord in &paragraph.coords {
            self.0.write_i16(coord);
        }
        self.len(paragraph.runs.len());
        for run in &paragraph.runs {
            self.0.write_u32(run.face);
            self.f32(run.font_size);
            self.0.write_u32(run.coords.start);
            self.0.write_u32(run.coords.end);
            run.synthesis.hash(&mut self.0);
            self.colors(run.color.as_ref().map(std::slice::from_ref));
            self.f32(run.baseline);
            self.0.write_u32(run.glyphs.start);
            self.0.write_u32(run.glyphs.end);
        }
        self.len(paragraph.glyphs.len());
        for glyph in &paragraph.glyphs {
            self.0.write_u16(glyph.id);
            self.f32(glyph.x);
            self.f32(glyph.dy);
        }
    }

    fn stored(&mut self, payload: Option<&StoredPayload>) {
        self.tag(&payload);
        let Some(payload) = payload else { return };
        self.tag(payload);
        match payload {
            StoredPayload::Path(path) => path.fingerprint(self),
            StoredPayload::NestedRRects(NestedRRects { outer, inner }) => {
                self.rrect(outer);
                self.rrect(inner);
            }
            StoredPayload::ImageRegion(ImageRegionPlacement {
                src,
                dst,
                tile,
                repeat,
                filter,
            }) => {
                self.rect(src);
                self.rect(dst);
                self.option_rect(tile.as_ref());
                self.tag(repeat);
                self.tag(filter);
                if let Some(filter) = filter {
                    self.color_filter(filter);
                }
            }
            StoredPayload::ColorFilter(filter) => self.color_filter(filter),
        }
    }

    /// An op's own values; the payload it names is fed apart, by content.
    fn op(&mut self, op: &DrawOp) {
        self.tag(op);
        match op {
            DrawOp::ClipRect {
                rect,
                clip_op,
                clip_behavior,
            } => {
                self.rect(rect);
                self.tag(clip_op);
                self.tag(clip_behavior);
            }
            DrawOp::ClipRRect {
                rrect,
                clip_op,
                clip_behavior,
            } => {
                self.rrect(rrect);
                self.tag(clip_op);
                self.tag(clip_behavior);
            }
            DrawOp::ClipRSuperellipse {
                rsuperellipse,
                clip_op,
                clip_behavior,
            } => {
                self.rsuperellipse(rsuperellipse);
                self.tag(clip_op);
                self.tag(clip_behavior);
            }
            DrawOp::ClipPath {
                path: _,
                clip_op,
                clip_behavior,
            } => {
                self.tag(clip_op);
                self.tag(clip_behavior);
            }
            DrawOp::Line { p1, p2, paint } => {
                self.point(*p1);
                self.point(*p2);
                self.paint(paint);
            }
            DrawOp::Rect { rect, paint } | DrawOp::Oval { rect, paint } => {
                self.rect(rect);
                self.paint(paint);
            }
            DrawOp::RRect { rrect, paint } => {
                self.rrect(rrect);
                self.paint(paint);
            }
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => {
                self.point(*center);
                self.f64(*radius);
                self.paint(paint);
            }
            DrawOp::Path { path: _, paint } | DrawOp::DRRect { rrects: _, paint } => {
                self.paint(paint);
            }
            DrawOp::Paragraph {
                paragraph,
                offset,
                color,
            } => {
                self.paragraph(paragraph);
                self.offset(*offset);
                self.color(*color);
            }
            DrawOp::ParagraphShadow {
                paragraph,
                offset,
                color,
                sigma,
            } => {
                self.paragraph(paragraph);
                self.offset(*offset);
                self.color(*color);
                self.f32(sigma.get());
            }
            DrawOp::Image { image, dst, paint } => {
                self.image(image);
                self.rect(dst);
                self.option_paint(paint.as_deref());
            }
            DrawOp::ImageRegion {
                image,
                region: _,
                paint,
            } => {
                self.image(image);
                self.option_paint(paint.as_deref());
            }
            DrawOp::ImageRepeat {
                image,
                dst,
                repeat,
                paint,
            } => {
                self.image(image);
                self.rect(dst);
                self.tag(repeat);
                self.option_paint(paint.as_deref());
            }
            DrawOp::ImageNineSlice {
                image,
                center_slice,
                dst,
                paint,
            } => {
                self.image(image);
                self.rect(center_slice);
                self.rect(dst);
                self.option_paint(paint.as_deref());
            }
            DrawOp::ImageFiltered {
                image,
                dst,
                filter: _,
                paint,
            } => {
                self.image(image);
                self.rect(dst);
                self.option_paint(paint.as_deref());
            }
            DrawOp::Texture {
                texture_id,
                dst,
                src,
                filter_quality,
                opacity,
            } => {
                self.u64(texture_id.get());
                self.rect(dst);
                self.option_rect(src.as_ref());
                self.tag(filter_quality);
                self.f64(*opacity);
            }
            DrawOp::Shadow {
                path: _,
                color,
                elevation,
            } => {
                self.color(*color);
                self.f64(*elevation);
            }
            DrawOp::Arc {
                rect,
                start_angle,
                sweep_angle,
                use_center,
                paint,
            } => {
                self.rect(rect);
                self.f64(*start_angle);
                self.f64(*sweep_angle);
                self.bool(*use_center);
                self.paint(paint);
            }
            DrawOp::Points {
                mode,
                points,
                paint,
            } => {
                self.tag(mode);
                self.points(points);
                self.paint(paint);
            }
            DrawOp::Vertices {
                vertices,
                colors,
                tex_coords,
                indices,
                paint,
            } => {
                self.points(vertices);
                self.colors(colors.as_deref());
                self.tag(tex_coords);
                if let Some(tex_coords) = tex_coords {
                    self.points(tex_coords);
                }
                self.len(indices.len());
                for &index in indices {
                    self.0.write_u16(index);
                }
                self.paint(paint);
            }
            DrawOp::Color { color, blend_mode } => {
                self.color(*color);
                self.tag(blend_mode);
            }
            DrawOp::Paint { paint } => self.paint(paint),
            DrawOp::Atlas {
                image,
                sprites,
                transforms,
                colors,
                blend_mode,
                paint,
            } => {
                self.image(image);
                self.len(sprites.len());
                for sprite in sprites {
                    self.rect(sprite);
                }
                self.len(transforms.len());
                for transform in transforms {
                    self.matrix(transform);
                }
                self.colors(colors.as_deref());
                self.tag(blend_mode);
                self.option_paint(paint.as_deref());
            }
            DrawOp::SaveLayer { bounds, paint } => {
                self.option_rect(bounds.as_ref());
                self.paint(paint);
            }
            DrawOp::RestoreLayer | DrawOp::Save | DrawOp::Restore => {}
        }
    }
}
