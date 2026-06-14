//! Stable, normalized projection of one [`DrawCommand`] to a single text line,
//! plus [`serialize_layer_tree`] / [`serialize_layer_subtree`] /
//! [`collect_commands`] that walk a [`flui_layer::LayerTree`] to stable text.
//!
//! This is the leaf of the snapshot serializer and the unit a predicate sees.
//! Stability contract: once the line format is chosen (floats 2-dec, color
//! `#RRGGBBAA`, transform omitted unless non-identity) it must not drift.
//!
//! # Task context
//!
//! Task 4 will expose `serialize_layer_tree` on `FrameRun::snapshot()`.
//! Keep this file focused: summary helpers + the tree walk.

use flui_foundation::{LayerId, RenderId};
use flui_layer::LayerTree;
use flui_painting::display_list::summary::fmt::{
    f, fmt_clip, fmt_clip_op, fmt_point, fmt_rect, fmt_rrect, hex_color, maybe_transform,
    summarize_paint,
};
use flui_painting::display_list::{DisplayList, DrawCommand};

/// Coarse category of a drawing command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    /// Rectangle (filled or stroked).
    Rect,
    /// Rounded rectangle.
    RRect,
    /// Circle.
    Circle,
    /// Oval / ellipse.
    Oval,
    /// Arbitrary path.
    Path,
    /// Line segment.
    Line,
    /// Arc segment.
    Arc,
    /// Difference between two rounded rectangles.
    DRRect,
    /// Clip operation (any geometry).
    Clip,
    /// Text (plain or rich spans).
    Text,
    /// Image (any image variant, atlas, or texture).
    Image,
    /// Drop shadow.
    Shadow,
    /// Gradient fill.
    Gradient,
    /// Layer command (SaveLayer / RestoreLayer / ShaderMask / BackdropFilter).
    Layer,
    /// Any variant not covered by the above (fills, vertices, …).
    Other,
}

/// Stable, normalized projection of one [`DrawCommand`].
///
/// The `line` field is what tests assert on; the `kind` field lets predicates
/// filter by category without parsing strings.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCommandSummary {
    /// Coarse category of the command.
    pub kind: DrawKind,
    /// Stable single-line text representation of the command.
    pub line: String,
}

// ── public API ───────────────────────────────────────────────────────────────

/// Produce a stable, normalized single-line summary of one [`DrawCommand`].
///
/// Every named variant gets its own match arm so that adding a new variant
/// to `DrawCommand` (a coordinated breaking change) immediately produces a
/// compile error here rather than silently falling through.
#[must_use]
pub fn summarize_command(cmd: &DrawCommand) -> DrawCommandSummary {
    match cmd {
        // ── Clips ────────────────────────────────────────────────────────────
        DrawCommand::ClipRect {
            rect,
            clip_op,
            clip_behavior,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRect rect={} op={} clip={}{}",
                fmt_rect(*rect),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRRect rrect={} op={} clip={}{}",
                fmt_rrect(rrect),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRSuperellipse rect={} op={} clip={}{}",
                fmt_rect(rsuperellipse.outer_rect()),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipPath {
            path,
            clip_op,
            clip_behavior,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipPath bounds={} pts={} op={} clip={}{}",
                fmt_rect(path.compute_bounds()),
                path.commands().len(),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
                maybe_transform(transform),
            ),
        },

        // ── Primitive shapes ─────────────────────────────────────────────────
        DrawCommand::DrawLine {
            p1,
            p2,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Line,
            line: format!(
                "DrawLine {}->{} {}{}",
                fmt_point(*p1),
                fmt_point(*p2),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawRect {
            rect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Rect,
            line: format!(
                "DrawRect rect={} {}{}",
                fmt_rect(*rect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawRRect {
            rrect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::RRect,
            line: format!(
                "DrawRRect rrect={} {}{}",
                fmt_rrect(rrect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Circle,
            line: format!(
                "DrawCircle center={} r={} {}{}",
                fmt_point(*center),
                f(radius.get()),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawOval {
            rect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Oval,
            line: format!(
                "DrawOval rect={} {}{}",
                fmt_rect(*rect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPath {
            path,
            paint,
            transform,
        } => {
            // Do NOT dump raw path verbs — too verbose and unstable.
            // Use bounds + command count as the stable fingerprint.
            DrawCommandSummary {
                kind: DrawKind::Path,
                line: format!(
                    "DrawPath bounds={} pts={} {}{}",
                    fmt_rect(path.compute_bounds()),
                    path.commands().len(),
                    summarize_paint(paint),
                    maybe_transform(transform),
                ),
            }
        }

        DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Arc,
            line: format!(
                "DrawArc rect={} start={} sweep={} center={} {}{}",
                fmt_rect(*rect),
                f(*start_angle),
                f(*sweep_angle),
                use_center,
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawDRRect {
            outer,
            inner,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::DRRect,
            line: format!(
                "DrawDRRect outer={} inner={} {}{}",
                fmt_rrect(outer),
                fmt_rrect(inner),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPoints {
            mode,
            points,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Path,
            line: format!(
                "DrawPoints mode={mode:?} pts={} {}{}",
                points.len(),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawVertices {
            vertices,
            paint,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawVertices verts={} {}{}",
                vertices.len(),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        // ── Text ─────────────────────────────────────────────────────────────
        DrawCommand::DrawText {
            text,
            offset,
            paint,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Text,
            line: format!(
                "DrawText offset=({},{}) {:?} {}{}",
                f(offset.dx.get()),
                f(offset.dy.get()),
                text,
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawTextSpan {
            span,
            offset,
            transform,
            ..
        } => {
            // Summarize via plain text (via to_plain_text). Glyph/run details
            // are NOT needed — shaped content is Task 3's concern (layer walk).
            let plain = span.to_plain_text();
            DrawCommandSummary {
                kind: DrawKind::Text,
                line: format!(
                    "DrawTextSpan offset=({},{}) {:?}{}",
                    f(offset.dx.get()),
                    f(offset.dy.get()),
                    plain,
                    maybe_transform(transform),
                ),
            }
        }

        // ── Images ───────────────────────────────────────────────────────────
        DrawCommand::DrawImage { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImage dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageRepeat { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageRepeat dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageNineSlice { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageNineSlice dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageFiltered { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageFiltered dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawTexture { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawTexture dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawAtlas {
            image: _,
            sprites,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawAtlas sprites={}{}",
                sprites.len(),
                maybe_transform(transform),
            ),
        },

        // ── Effects ──────────────────────────────────────────────────────────
        DrawCommand::DrawShadow {
            path,
            color,
            elevation,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Shadow,
            line: format!(
                "DrawShadow path_bounds={} color={} elev={}{}",
                fmt_rect(path.compute_bounds()),
                hex_color(*color),
                f(*elevation),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawGradient {
            rect, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Gradient,
            line: format!(
                "DrawGradient rect={}{}",
                fmt_rect(*rect),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawGradientRRect {
            rrect, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Gradient,
            line: format!(
                "DrawGradientRRect rrect={}{}",
                fmt_rrect(rrect),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ShaderMask {
            bounds,
            transform,
            // child: Box<DisplayList> — recursing into child display lists is
            // Task 3 (layer walk). Here we only summarize this command line.
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "ShaderMask bounds={}{}",
                fmt_rect(*bounds),
                maybe_transform(transform),
            ),
        },

        DrawCommand::BackdropFilter {
            bounds,
            transform,
            // child: Option<Box<DisplayList>> — recursing into child display
            // lists is Task 3 (layer walk). Here we only summarize this line.
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "BackdropFilter bounds={}{}",
                fmt_rect(*bounds),
                maybe_transform(transform),
            ),
        },

        // ── Fills ────────────────────────────────────────────────────────────
        DrawCommand::DrawColor {
            color,
            blend_mode,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawColor {} mode={blend_mode:?}{}",
                hex_color(*color),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPaint {
            paint, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawPaint {}{}",
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        // ── Layer commands ───────────────────────────────────────────────────
        DrawCommand::SaveLayer {
            bounds,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "SaveLayer bounds={} {}{}",
                match bounds {
                    Some(r) => fmt_rect(*r),
                    None => "none".to_owned(),
                },
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::RestoreLayer { transform } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!("RestoreLayer{}", maybe_transform(transform)),
        },

        // Catch-all for any future `#[non_exhaustive]` variants added to
        // `DrawCommand` that are not yet named above. Every *named* variant
        // above already has a dedicated arm, so nothing silently falls through
        // within the current variant set.
        _ => DrawCommandSummary {
            kind: DrawKind::Other,
            line: "Unknown".to_owned(),
        },
    }
}

// ── LayerTree serialization ──────────────────────────────────────────────────

/// Serialize a `DisplayList`'s commands into `out` at the given indent depth.
///
/// Recurses into the child `DisplayList`s embedded in `ShaderMask` and
/// `BackdropFilter` commands so that masked content appears in the snapshot.
fn write_display_list(out: &mut String, dl: &DisplayList, depth: usize) {
    let indent = "  ".repeat(depth);
    for cmd in dl.iter() {
        // Recurse into effect-command children before printing the line so that
        // the child content appears nested under the effect header.
        match cmd {
            DrawCommand::ShaderMask { child, .. } => {
                let summary = summarize_command(cmd);
                out.push_str(&indent);
                out.push_str(&summary.line);
                out.push('\n');
                write_display_list(out, child, depth + 1);
            }
            DrawCommand::BackdropFilter {
                child: Some(child), ..
            } => {
                let summary = summarize_command(cmd);
                out.push_str(&indent);
                out.push_str(&summary.line);
                out.push('\n');
                write_display_list(out, child, depth + 1);
            }
            _ => {
                let summary = summarize_command(cmd);
                out.push_str(&indent);
                out.push_str(&summary.line);
                out.push('\n');
            }
        }
    }
}

/// Collect all `DrawCommandSummary` values from a `DisplayList`, recursing
/// into `ShaderMask` and `BackdropFilter` child lists.
fn collect_from_display_list(dl: &DisplayList, out: &mut Vec<DrawCommandSummary>) {
    for cmd in dl.iter() {
        match cmd {
            DrawCommand::ShaderMask { child, .. } => {
                out.push(summarize_command(cmd));
                collect_from_display_list(child, out);
            }
            DrawCommand::BackdropFilter {
                child: Some(child), ..
            } => {
                out.push(summarize_command(cmd));
                collect_from_display_list(child, out);
            }
            _ => {
                out.push(summarize_command(cmd));
            }
        }
    }
}

/// Write one layer node (and all its descendants) into `out`.
///
/// Each layer gets one header line at `depth*2` spaces of indentation.
/// `Picture` layers additionally emit their commands at `depth+1`.
/// Container layers recurse into children in their stored order (deterministic,
/// no hash iteration).
fn write_layer(out: &mut String, tree: &LayerTree, id: LayerId, depth: usize) {
    use flui_layer::Layer;

    let Some(node) = tree.get(id) else {
        return;
    };
    let indent = "  ".repeat(depth);

    // One header line describing this layer with its defining parameter.
    match node.layer() {
        Layer::Canvas(_) => {
            out.push_str(&indent);
            out.push_str("Canvas\n");
        }
        Layer::Picture(p) => {
            let b = p.bounds();
            out.push_str(&indent);
            out.push_str(&format!(
                "Picture bounds=({},{} {}x{})\n",
                f(b.left().get()),
                f(b.top().get()),
                f(b.width().get()),
                f(b.height().get()),
            ));
            // Emit commands one level deeper.
            write_display_list(out, p.picture(), depth + 1);
        }
        Layer::Texture(_) => {
            out.push_str(&indent);
            out.push_str("Texture\n");
        }
        Layer::PlatformView(_) => {
            out.push_str(&indent);
            out.push_str("PlatformView\n");
        }
        Layer::PerformanceOverlay(_) => {
            out.push_str(&indent);
            out.push_str("PerformanceOverlay\n");
        }
        Layer::ClipRect(c) => {
            let r = c.clip_rect();
            out.push_str(&indent);
            out.push_str(&format!(
                "ClipRect rect=({},{} {}x{}) clip={}\n",
                f(r.left().get()),
                f(r.top().get()),
                f(r.width().get()),
                f(r.height().get()),
                fmt_clip(c.clip_behavior()),
            ));
        }
        Layer::ClipRRect(c) => {
            // Serialize the full rounded rect (outer bounds + corner radii) so a
            // regression that drops or changes the radii diffs the snapshot
            // instead of passing under an identical outer-rect line.
            out.push_str(&indent);
            out.push_str(&format!(
                "ClipRRect rrect={} clip={}\n",
                fmt_rrect(c.clip_rrect()),
                fmt_clip(c.clip_behavior()),
            ));
        }
        Layer::ClipPath(c) => {
            // Mirror the command-path summary: clipping geometry shape (bounds +
            // point count) and behavior, so the clip is not reduced to a bare
            // "ClipPath" marker that hides every shape/quality change.
            let path = c.clip_path();
            out.push_str(&indent);
            out.push_str(&format!(
                "ClipPath bounds={} pts={} clip={}\n",
                fmt_rect(path.compute_bounds()),
                path.commands().len(),
                fmt_clip(c.clip_behavior()),
            ));
        }
        Layer::ClipSuperellipse(c) => {
            let r = c.clip_superellipse().outer_rect();
            out.push_str(&indent);
            out.push_str(&format!(
                "ClipSuperellipse rect=({},{} {}x{}) clip={}\n",
                f(r.left().get()),
                f(r.top().get()),
                f(r.width().get()),
                f(r.height().get()),
                fmt_clip(c.clip_behavior()),
            ));
        }
        Layer::Offset(o) => {
            out.push_str(&indent);
            out.push_str(&format!("Offset dx={} dy={}\n", f(o.dx()), f(o.dy())));
        }
        Layer::Transform(_) => {
            // Known blind spot: `TransformLayer` exposes no public matrix getter
            // (only `is_identity`/`transform_point`), so the snapshot records the
            // layer's presence but not its matrix. Two distinct matrices produce
            // the same line; a `TransformLayer::matrix` accessor would let this
            // print the normalized matrix (same `f()` format) and close the gap.
            out.push_str(&indent);
            out.push_str("Transform\n");
        }
        Layer::Opacity(o) => {
            out.push_str(&indent);
            out.push_str(&format!("Opacity alpha={}\n", f(o.alpha())));
        }
        Layer::ColorFilter(_) => {
            out.push_str(&indent);
            out.push_str("ColorFilter\n");
        }
        Layer::ImageFilter(_) => {
            out.push_str(&indent);
            out.push_str("ImageFilter\n");
        }
        Layer::ShaderMask(s) => {
            let r = s.bounds();
            out.push_str(&indent);
            out.push_str(&format!(
                "ShaderMask bounds=({},{} {}x{})\n",
                f(r.left().get()),
                f(r.top().get()),
                f(r.width().get()),
                f(r.height().get()),
            ));
        }
        Layer::BackdropFilter(b) => {
            let r = b.bounds();
            out.push_str(&indent);
            out.push_str(&format!(
                "BackdropFilter bounds=({},{} {}x{})\n",
                f(r.left().get()),
                f(r.top().get()),
                f(r.width().get()),
                f(r.height().get()),
            ));
        }
        Layer::Leader(_) => {
            out.push_str(&indent);
            out.push_str("Leader\n");
        }
        Layer::Follower(_) => {
            out.push_str(&indent);
            out.push_str("Follower\n");
        }
        Layer::AnnotatedRegion(_) => {
            out.push_str(&indent);
            out.push_str("AnnotatedRegion\n");
        }
    }
    // NOTE: `Layer` is not `#[non_exhaustive]`, so this match is exhaustive by
    // construction — a new variant in `flui-layer` produces a compile error
    // here, which is the desired behaviour (the snapshot must account for it).

    // Recurse into children in their stored order (deterministic).
    for &child_id in node.children() {
        write_layer(out, tree, child_id, depth + 1);
    }
}

/// Serialize the full [`LayerTree`] to a stable indented text form.
///
/// # Format
///
/// Each layer produces one header line indented by `depth * 2` spaces,
/// containing the layer kind plus its defining parameter (clip rect, alpha,
/// offset, …). `Picture` layers additionally list their draw commands one
/// level deeper (via [`summarize_command`]). `ShaderMask` and
/// `BackdropFilter` draw commands inside a picture recurse into their
/// embedded child `DisplayList`s.
///
/// The format is stable across runs: floats are 2-decimal, children appear in
/// their stored (insertion) order, and no hash-map iteration is involved.
#[must_use]
pub fn serialize_layer_tree(tree: &LayerTree) -> String {
    let mut out = String::new();
    if let Some(root) = tree.root() {
        write_layer(&mut out, tree, root, 0);
    }
    out
}

/// Serialize the subtree rooted at the layer boundary for `node`.
///
/// # Current approximation
///
/// `LayerNode` carries an `element_id: Option<ElementId>` cross-tree
/// reference but **not** a `RenderId`, and `OffsetLayer` (the repaint-boundary
/// carrier) stores no per-node identity. Because there is no O(1) lookup of
/// "the layer whose boundary corresponds to render node `node`", this function
/// currently falls back to [`serialize_layer_tree`] and serializes the whole
/// tree.
///
/// A per-node scoping map (`RenderId → LayerId`) would enable precise subtree
/// snapshots; tracked as a concern for Task 4 / the `FrameRun` integration.
#[must_use]
pub fn serialize_layer_subtree(tree: &LayerTree, _node: RenderId) -> String {
    // No RenderId→LayerId mapping exists yet; fall back to the whole tree.
    serialize_layer_tree(tree)
}

/// Collect every [`DrawCommandSummary`] reachable from all `Picture` layers in
/// the tree, in pre-order.
///
/// `ShaderMask` and `BackdropFilter` commands that embed a child `DisplayList`
/// are recursed so masked content is included.
#[must_use]
pub fn collect_commands(tree: &LayerTree) -> Vec<DrawCommandSummary> {
    fn walk(tree: &LayerTree, id: LayerId, out: &mut Vec<DrawCommandSummary>) {
        let Some(node) = tree.get(id) else {
            return;
        };
        if let flui_layer::Layer::Picture(p) = node.layer() {
            collect_from_display_list(p.picture(), out);
        }
        for &child_id in node.children() {
            walk(tree, child_id, out);
        }
    }

    let mut out = Vec::new();
    if let Some(root) = tree.root() {
        walk(tree, root, &mut out);
    }
    out
}

// ── Option<&LayerTree> helpers (shared by FrameRun and PaintRun) ─────────────

/// Serialize a `LayerTree` to stable indented text, or return `"<no layer
/// tree>"` when `tree` is `None`.
///
/// Delegates to [`serialize_layer_tree`]; see its docs for the format contract.
#[must_use]
pub fn snapshot_tree(tree: Option<&LayerTree>) -> String {
    tree.map_or_else(|| "<no layer tree>".to_owned(), serialize_layer_tree)
}

/// Serialize the subtree rooted at the layer boundary for `node`, or return
/// `"<no layer tree>"` when `tree` is `None`.
///
/// Delegates to [`serialize_layer_subtree`]; see its docs for the current
/// approximation (whole-tree fallback until a `RenderId → LayerId` map exists).
#[must_use]
pub fn snapshot_subtree(tree: Option<&LayerTree>, node: RenderId) -> String {
    tree.map_or_else(
        || "<no layer tree>".to_owned(),
        |t| serialize_layer_subtree(t, node),
    )
}

/// Collect every [`DrawCommandSummary`] reachable from `tree`, or return an
/// empty `Vec` when `tree` is `None`.
///
/// Delegates to [`collect_commands`].
#[must_use]
pub fn commands_of(tree: Option<&LayerTree>) -> Vec<DrawCommandSummary> {
    tree.map(collect_commands).unwrap_or_default()
}

/// Panics unless at least one command in `tree` satisfies `pred`.
///
/// On failure the panic message includes the full snapshot so the developer
/// can see what was actually painted.
///
/// Unlike Flutter's `paints..something()` matcher this assertion is **strict**:
/// if `pred` never matches it is always a test failure, never a silent pass.
pub fn assert_any(tree: Option<&LayerTree>, pred: impl Fn(&DrawCommandSummary) -> bool) {
    if !commands_of(tree).iter().any(pred) {
        panic!(
            "no painted command matched the predicate:\n{}",
            snapshot_tree(tree),
        );
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_painting::display_list::{DrawCommand, Paint};
    use flui_types::{
        geometry::{Matrix4, Pixels, Point, Rect, px},
        painting::Path,
        styling::Color,
    };

    use super::{DrawKind, summarize_command};

    /// Helper: build an identity `Rect<Pixels>` from raw f32 coordinates.
    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<Pixels> {
        Rect::from_xywh(px(x), px(y), px(w), px(h))
    }

    /// `DrawRect` with `fill Color::RED` + identity transform must produce
    /// `kind == Rect` and a stable line.
    #[test]
    fn summarize_draw_rect_is_stable() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            paint: Arc::new(Paint::fill(Color::RED)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Rect);
        // Color::RED = rgba(255,0,0,255) → #FF0000FF
        assert_eq!(
            s.line,
            "DrawRect rect=(0.00,0.00 40.00x40.00) fill #FF0000FF"
        );
    }

    /// `DrawShadow` must summarize with `kind == Shadow`.
    #[test]
    fn summarize_draw_shadow_has_shadow_kind() {
        let mut path = Path::new();
        path.add_rect(rect(10.0, 10.0, 50.0, 30.0));

        let cmd = DrawCommand::DrawShadow {
            path,
            color: Color::BLACK,
            elevation: 4.0,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Shadow);
        assert!(
            s.line.starts_with("DrawShadow"),
            "unexpected line: {}",
            s.line
        );
        assert!(
            s.line.contains("elev=4.00"),
            "expected elev=4.00 in: {}",
            s.line
        );
        // Color::BLACK = rgba(0,0,0,255) → #000000FF
        assert!(
            s.line.contains("#000000FF"),
            "expected #000000FF in: {}",
            s.line
        );
    }

    /// Non-identity transform must append `xf=[...]`.
    #[test]
    fn non_identity_transform_is_appended() {
        let translate = Matrix4::translation(10.0, 20.0, 0.0);
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::fill(Color::BLUE)),
            transform: translate,
        };
        let s = summarize_command(&cmd);
        assert!(
            s.line.contains("xf=["),
            "expected xf= suffix in: {}",
            s.line
        );
    }

    /// Identity transform must NOT append `xf=[...]`.
    #[test]
    fn identity_transform_is_omitted() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::fill(Color::BLUE)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert!(
            !s.line.contains("xf=["),
            "identity transform should be omitted, got: {}",
            s.line
        );
    }

    /// Stroke paint includes `stroke=<w>`.
    #[test]
    fn stroke_paint_includes_width() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::stroke(Color::GREEN, 2.5)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        // Color::GREEN = rgba(0,255,0,255)
        assert_eq!(
            s.line,
            "DrawRect rect=(0.00,0.00 10.00x10.00) stroke #00FF00FF stroke=2.50"
        );
    }

    /// `DrawText` must summarize with `kind == Text` and include the text.
    #[test]
    fn summarize_draw_text_has_text_kind() {
        use flui_types::geometry::Offset;
        let cmd = DrawCommand::DrawText {
            text: "hello".to_owned(),
            offset: Offset::new(px(1.0), px(2.0)),
            size: flui_types::geometry::Size::new(px(50.0), px(12.0)),
            style: flui_types::typography::TextStyle::default(),
            paint: Arc::new(Paint::fill(Color::BLACK)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Text);
        assert!(s.line.contains("\"hello\""), "expected text in: {}", s.line);
    }

    /// `ClipRect` must summarize with `kind == Clip`.
    #[test]
    fn summarize_clip_rect_has_clip_kind() {
        use flui_painting::display_list::ClipOp;
        use flui_types::painting::Clip;
        let cmd = DrawCommand::ClipRect {
            rect: rect(5.0, 5.0, 100.0, 80.0),
            clip_op: ClipOp::Intersect,
            clip_behavior: Clip::HardEdge,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Clip);
        assert!(
            s.line.starts_with("ClipRect"),
            "unexpected line: {}",
            s.line
        );
        assert!(
            s.line.contains("op=intersect"),
            "expected op=intersect in: {}",
            s.line
        );
        assert!(
            s.line.contains("clip=hard"),
            "expected clip=hard in: {}",
            s.line
        );
    }

    /// Clip behavior is part of the summary: the same geometry under `HardEdge`
    /// vs `AntiAlias` must produce different lines, so a rendering-quality
    /// regression diffs the snapshot instead of passing silently.
    #[test]
    fn clip_behavior_distinguishes_clip_summaries() {
        use flui_painting::display_list::ClipOp;
        use flui_types::painting::Clip;
        let mk = |behavior| {
            summarize_command(&DrawCommand::ClipRect {
                rect: rect(0.0, 0.0, 10.0, 10.0),
                clip_op: ClipOp::Intersect,
                clip_behavior: behavior,
                transform: Matrix4::IDENTITY,
            })
            .line
        };
        let hard = mk(Clip::HardEdge);
        let aa = mk(Clip::AntiAlias);
        assert!(hard.contains("clip=hard"), "got: {hard}");
        assert!(aa.contains("clip=antialias"), "got: {aa}");
        assert_ne!(hard, aa, "clip behavior must change the summary");
    }

    /// Rounded-clip radii are part of the summary: the same outer rect with
    /// different corner radii must produce different lines, so a dropped or
    /// altered radius diffs the snapshot instead of passing silently.
    #[test]
    fn clip_rrect_radii_distinguish_summaries() {
        use flui_painting::display_list::ClipOp;
        use flui_types::geometry::RRect;
        use flui_types::painting::Clip;
        let mk = |radius: f32| {
            summarize_command(&DrawCommand::ClipRRect {
                rrect: RRect::from_rect_circular(rect(0.0, 0.0, 40.0, 40.0), px(radius)),
                clip_op: ClipOp::Intersect,
                clip_behavior: Clip::HardEdge,
                transform: Matrix4::IDENTITY,
            })
            .line
        };
        assert_ne!(
            mk(4.0),
            mk(12.0),
            "different corner radii must change the summary"
        );
    }

    /// `RestoreLayer` must summarize with `kind == Layer`.
    #[test]
    fn summarize_restore_layer_has_layer_kind() {
        let cmd = DrawCommand::RestoreLayer {
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Layer);
        assert_eq!(s.line, "RestoreLayer");
    }

    /// Negative-zero normalization: `f(-0.0)` must produce `"0.00"` not `"-0.00"`.
    #[test]
    fn negative_zero_normalizes_to_zero() {
        use super::f;
        assert_eq!(f(-0.0_f32), "0.00");
        assert_eq!(f(0.0_f32), "0.00");
        assert_eq!(f(-1.5_f32), "-1.50");
    }

    /// `hex_color` produces the canonical `#RRGGBBAA` format.
    #[test]
    fn hex_color_format() {
        use super::hex_color;
        assert_eq!(hex_color(Color::RED), "#FF0000FF");
        assert_eq!(hex_color(Color::TRANSPARENT), "#00000000");
        assert_eq!(hex_color(Color::rgba(1, 2, 3, 4)), "#01020304");
    }

    /// `DrawImage` must summarize with `kind == Image`.
    #[test]
    fn summarize_draw_image_has_image_kind() {
        use flui_types::painting::image::Image;
        let cmd = DrawCommand::DrawImage {
            image: Image::default(),
            dst: rect(0.0, 0.0, 100.0, 80.0),
            paint: None,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Image);
        assert!(
            s.line.starts_with("DrawImage"),
            "unexpected line: {}",
            s.line
        );
    }

    /// Point helper produces correct format.
    #[test]
    fn fmt_point_helper() {
        use super::fmt_point;
        let p = Point::new(px(3.5), px(-1.0));
        assert_eq!(fmt_point(p), "(3.50,-1.00)");
    }

    // ── LayerTree serialization tests ─────────────────────────────────────────

    /// Mounting a `RenderColoredBox::red(40, 40)` and running a frame must
    /// produce a serialized layer tree containing `"Picture"` and the stable
    /// `DrawRect` line for the painted rectangle.
    #[test]
    fn serialize_simple_box_is_stable() {
        use flui_types::Size;

        use crate::objects::RenderColoredBox;
        use crate::testing::{RenderTester, box_node, serialize_layer_tree};

        let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
            .with_size(Size::new(px(40.0), px(40.0)))
            .run_frame();

        let tree = run
            .layer_tree()
            .expect("RenderColoredBox must produce a layer tree");
        let s = serialize_layer_tree(tree);

        assert!(
            s.contains("Picture"),
            "serialized tree must contain a Picture layer; got:\n{s}"
        );
        assert!(
            s.contains("DrawRect rect=(0.00,0.00 40.00x40.00)"),
            "serialized tree must contain the DrawRect for the red box; got:\n{s}"
        );
    }

    /// `collect_commands` on a `RenderColoredBox` frame must return a non-empty
    /// `Vec` whose first element has `kind == DrawKind::Rect`.
    #[test]
    fn collect_commands_red_box_first_is_rect() {
        use flui_types::Size;

        use crate::objects::RenderColoredBox;
        use crate::testing::{RenderTester, box_node, collect_commands};

        let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
            .with_size(Size::new(px(40.0), px(40.0)))
            .run_frame();

        let tree = run
            .layer_tree()
            .expect("RenderColoredBox must produce a layer tree");
        let cmds = collect_commands(tree);

        assert!(
            !cmds.is_empty(),
            "collect_commands must return at least one command for a painted box"
        );
        assert_eq!(
            cmds[0].kind,
            DrawKind::Rect,
            "first command for a colored box must be a Rect, got: {:?}",
            cmds[0].kind
        );
    }
}
