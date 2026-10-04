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

use std::fmt::Write as _;

use flui_foundation::geometry::{Matrix4, Point, RRect, Rect};
use flui_foundation::{LayerId, RenderId};
use flui_layer::LayerTree;
use flui_painting::PaintStyle;
use flui_painting::paint::{ClipOp, Paint};
use flui_painting::{DisplayList, DrawCommand, DrawOp};
use flui_painting::{paint::Clip, styling::Color};

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
    /// Offscreen layer command (SaveLayer / RestoreLayer).
    Layer,
    /// Save or restore the current transform and clip state.
    State,
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

// ── private helpers ──────────────────────────────────────────────────────────

/// Format one `f64` to 2 decimal places, normalizing `-0.0` → `0.0`.
fn f(v: f64) -> String {
    // Stability contract: callers pass finite floats. A non-finite value would
    // format as "NaN"/"inf" and break the fixed-decimal snapshot invariant — it
    // signals a bug in the render object that produced the command, not here.
    debug_assert!(
        v.is_finite(),
        "snapshot: non-finite float in a draw command"
    );
    // Normalize negative zero before formatting.
    let v = if v == 0.0 { 0.0_f64 } else { v };
    format!("{v:.2}")
}

/// Format a `Color` as `#RRGGBBAA`.
fn hex_color(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
}

/// Summarize a `Paint` as `"<style> <#RRGGBBAA>[ stroke=<w>]"`.
fn summarize_paint(paint: &Paint) -> String {
    let style = match paint.style {
        PaintStyle::Fill => "fill",
        PaintStyle::Stroke => "stroke",
    };
    let color = hex_color(paint.color);
    // Only the NON-default is printed. Anti-aliasing is on for almost every
    // command, so printing it always would add a constant to every line of
    // every snapshot and change all of them at once; printing only the
    // deviation keeps existing snapshots untouched while making a paint that
    // opted out visible to any test reading these lines.
    let aliased = if paint.anti_alias { "" } else { " aliased" };
    if matches!(paint.style, PaintStyle::Stroke) {
        format!("{style} {color} stroke={}{aliased}", f(paint.stroke_width))
    } else {
        format!("{style} {color}{aliased}")
    }
}

/// Format a `Rect` as `"(l,t WxH)"`.
fn fmt_rect(r: Rect<f64>) -> String {
    format!(
        "({},{} {}x{})",
        f(r.left()),
        f(r.top()),
        f(r.width()),
        f(r.height()),
    )
}

/// Format a `Point` as `"(x,y)"`.
fn fmt_point(p: Point<f64>) -> String {
    format!("({},{})", f(p.x), f(p.y))
}

/// Format an `RRect` as `"(l,t WxH r=tl/tr/br/bl)"`.
///
/// Uses the `rect` field of `RRect` for geometry and the four corner radii
/// (circular approximation: `x` component of each radius).
fn fmt_rrect(rr: &RRect) -> String {
    let r = rr.rect;
    format!(
        "({},{} {}x{} r={}/{}/{}/{})",
        f(r.left()),
        f(r.top()),
        f(r.width()),
        f(r.height()),
        f(rr.top_left.x),
        f(rr.top_right.x),
        f(rr.bottom_right.x),
        f(rr.bottom_left.x),
    )
}

/// Format a `ClipOp` as a short lowercase string.
fn fmt_clip_op(op: ClipOp) -> &'static str {
    match op {
        ClipOp::Intersect => "intersect",
        ClipOp::Difference => "difference",
    }
}

/// Format a `Clip` behavior as a short lowercase string.
///
/// Distinct rendering qualities must serialize distinctly so a regression that
/// swaps, say, `AntiAlias` for `HardEdge` shows up as a snapshot diff instead
/// of passing silently.
fn fmt_clip(behavior: Clip) -> &'static str {
    match behavior {
        Clip::None => "none",
        Clip::HardEdge => "hard",
        Clip::AntiAlias => "antialias",
        Clip::AntiAliasWithSaveLayer => "antialias-savelayer",
    }
}

/// Append a transform suffix when the matrix is non-identity.
fn maybe_transform(transform: &Matrix4) -> String {
    if transform.is_identity() {
        return String::new();
    }
    // Build the bracket inline without an intermediate `Vec` allocation.
    let mut s = " xf=[".to_owned();
    let mut first = true;
    for v in &transform.m {
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&f(*v));
    }
    s.push(']');
    s
}

// ── public API ───────────────────────────────────────────────────────────────

/// Produce a stable, normalized single-line summary of one [`DrawCommand`].
///
/// Every named variant gets its own match arm so that adding a new variant
/// to `DrawCommand` (a coordinated breaking change) immediately produces a
/// compile error here rather than silently falling through.
#[must_use]
pub fn summarize_command(cmd: &DrawCommand) -> DrawCommandSummary {
    let DrawCommandSummary { kind, mut line } = summarize_op(&cmd.op);
    line.push_str(&maybe_transform(&cmd.transform));
    DrawCommandSummary { kind, line }
}

/// The op half of [`summarize_command`]: the variant and its geometry,
/// without the transform suffix.
fn summarize_op(op: &DrawOp) -> DrawCommandSummary {
    match op {
        DrawOp::Save => DrawCommandSummary {
            kind: DrawKind::State,
            line: "Save".to_owned(),
        },
        DrawOp::Restore => DrawCommandSummary {
            kind: DrawKind::State,
            line: "Restore".to_owned(),
        },
        // ── Clips ────────────────────────────────────────────────────────────
        DrawOp::ClipRect {
            rect,
            clip_op,
            clip_behavior,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRect rect={} op={} clip={}",
                fmt_rect(*rect),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
            ),
        },

        DrawOp::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRRect rrect={} op={} clip={}",
                fmt_rrect(rrect),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
            ),
        },

        DrawOp::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRSuperellipse rect={} op={} clip={}",
                fmt_rect(rsuperellipse.outer_rect()),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
            ),
        },

        DrawOp::ClipPath {
            path,
            clip_op,
            clip_behavior,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipPath bounds={} pts={} op={} clip={}",
                fmt_rect(path.compute_bounds()),
                path.commands().len(),
                fmt_clip_op(*clip_op),
                fmt_clip(*clip_behavior),
            ),
        },

        // ── Primitive shapes ─────────────────────────────────────────────────
        DrawOp::Line { p1, p2, paint } => DrawCommandSummary {
            kind: DrawKind::Line,
            line: format!(
                "DrawLine {}->{} {}",
                fmt_point(*p1),
                fmt_point(*p2),
                summarize_paint(paint),
            ),
        },

        DrawOp::Rect { rect, paint } => DrawCommandSummary {
            kind: DrawKind::Rect,
            line: format!(
                "DrawRect rect={} {}",
                fmt_rect(*rect),
                summarize_paint(paint),
            ),
        },

        DrawOp::RRect { rrect, paint } => DrawCommandSummary {
            kind: DrawKind::RRect,
            line: format!(
                "DrawRRect rrect={} {}",
                fmt_rrect(rrect),
                summarize_paint(paint),
            ),
        },

        DrawOp::Circle {
            center,
            radius,
            paint,
        } => DrawCommandSummary {
            kind: DrawKind::Circle,
            line: format!(
                "DrawCircle center={} r={} {}",
                fmt_point(*center),
                f(*radius),
                summarize_paint(paint),
            ),
        },

        DrawOp::Oval { rect, paint } => DrawCommandSummary {
            kind: DrawKind::Oval,
            line: format!(
                "DrawOval rect={} {}",
                fmt_rect(*rect),
                summarize_paint(paint),
            ),
        },

        DrawOp::Path { path, paint } => {
            // Do NOT dump raw path verbs — too verbose and unstable.
            // Use bounds + command count as the stable fingerprint.
            DrawCommandSummary {
                kind: DrawKind::Path,
                line: format!(
                    "DrawPath bounds={} pts={} {}",
                    fmt_rect(path.compute_bounds()),
                    path.commands().len(),
                    summarize_paint(paint),
                ),
            }
        }

        DrawOp::Arc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
        } => DrawCommandSummary {
            kind: DrawKind::Arc,
            line: format!(
                "DrawArc rect={} start={} sweep={} center={} {}",
                fmt_rect(*rect),
                f(*start_angle),
                f(*sweep_angle),
                use_center,
                summarize_paint(paint),
            ),
        },

        DrawOp::DRRect {
            outer,
            inner,
            paint,
        } => DrawCommandSummary {
            kind: DrawKind::DRRect,
            line: format!(
                "DrawDRRect outer={} inner={} {}",
                fmt_rrect(outer),
                fmt_rrect(inner),
                summarize_paint(paint),
            ),
        },

        DrawOp::Points {
            mode,
            points,
            paint,
        } => DrawCommandSummary {
            kind: DrawKind::Path,
            line: format!(
                "DrawPoints mode={mode:?} pts={} {}",
                points.len(),
                summarize_paint(paint),
            ),
        },

        DrawOp::Vertices {
            vertices, paint, ..
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawVertices verts={} {}",
                vertices.len(),
                summarize_paint(paint),
            ),
        },

        // ── Text ─────────────────────────────────────────────────────────────
        DrawOp::Paragraph {
            paragraph,
            offset,
            color,
        } => {
            // The text, the root colour, and what reached the shaper per span.
            // Glyph geometry is deliberately absent, but the shaped styles are
            // NOT optional detail: a regression that recolours, re-weights, or
            // resizes a span moves no other field in this summary, so
            // omitting them makes such a change invisible to every snapshot.
            let runs = paragraph.describe_spans();
            DrawCommandSummary {
                kind: DrawKind::Text,
                line: format!(
                    "Paragraph offset=({},{}) {:?} {} lines={} runs=[{}]",
                    f(offset.dx),
                    f(offset.dy),
                    paragraph.text(),
                    hex_color(*color),
                    paragraph.line_count(),
                    runs.join(", "),
                ),
            }
        }

        // ── Images ───────────────────────────────────────────────────────────
        DrawOp::ImageRegion { src, dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageRegion src={} dst={}",
                fmt_rect(*src),
                fmt_rect(*dst)
            ),
        },
        DrawOp::Image { dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawImage dst={}", fmt_rect(*dst)),
        },

        DrawOp::ImageRepeat { dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawImageRepeat dst={}", fmt_rect(*dst)),
        },

        DrawOp::ImageNineSlice { dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawImageNineSlice dst={}", fmt_rect(*dst)),
        },

        DrawOp::ImageFiltered { dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawImageFiltered dst={}", fmt_rect(*dst)),
        },

        DrawOp::Texture { dst, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawTexture dst={}", fmt_rect(*dst)),
        },

        DrawOp::Atlas {
            image: _, sprites, ..
        } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!("DrawAtlas sprites={}", sprites.len()),
        },

        // ── Effects ──────────────────────────────────────────────────────────
        DrawOp::Shadow {
            path,
            color,
            elevation,
        } => DrawCommandSummary {
            kind: DrawKind::Shadow,
            line: format!(
                "DrawShadow path_bounds={} color={} elev={}",
                fmt_rect(path.compute_bounds()),
                hex_color(*color),
                f(*elevation),
            ),
        },

        // ── Fills ────────────────────────────────────────────────────────────
        DrawOp::Color { color, blend_mode } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!("DrawColor {} mode={blend_mode:?}", hex_color(*color)),
        },

        DrawOp::Paint { paint, .. } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!("DrawPaint {}", summarize_paint(paint)),
        },

        // ── Layer commands ───────────────────────────────────────────────────
        DrawOp::SaveLayer { bounds, paint } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "SaveLayer bounds={} {}",
                match bounds {
                    Some(r) => fmt_rect(*r),
                    None => "none".to_owned(),
                },
                summarize_paint(paint),
            ),
        },

        DrawOp::RestoreLayer => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: "RestoreLayer".to_owned(),
        },
    }
}

// ── LayerTree serialization ──────────────────────────────────────────────────

/// Serialize a `DisplayList`'s commands into `out` at the given indent depth.
fn write_display_list(out: &mut String, dl: &DisplayList, depth: usize) {
    let indent = "  ".repeat(depth);
    for cmd in dl {
        let summary = summarize_command(cmd);
        out.push_str(&indent);
        out.push_str(&summary.line);
        out.push('\n');
    }
}

/// Collect all `DrawCommandSummary` values from a `DisplayList`.
fn collect_from_display_list(dl: &DisplayList, out: &mut Vec<DrawCommandSummary>) {
    out.extend(dl.iter().map(summarize_command));
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
            out.push_str(&indent);
            match p.bounds() {
                Some(b) => {
                    let _ = writeln!(
                        out,
                        "Picture bounds=({},{} {}x{})",
                        f(b.left()),
                        f(b.top()),
                        f(b.width()),
                        f(b.height()),
                    );
                }
                None => out.push_str("Picture bounds=none\n"),
            }
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
            let _ = writeln!(
                out,
                "ClipRect rect=({},{} {}x{}) clip={}",
                f(r.left()),
                f(r.top()),
                f(r.width()),
                f(r.height()),
                fmt_clip(c.clip_behavior()),
            );
        }
        Layer::ClipRRect(c) => {
            // Serialize the full rounded rect (outer bounds + corner radii) so a
            // regression that drops or changes the radii diffs the snapshot
            // instead of passing under an identical outer-rect line.
            out.push_str(&indent);
            let _ = writeln!(
                out,
                "ClipRRect rrect={} clip={}",
                fmt_rrect(c.clip_rrect()),
                fmt_clip(c.clip_behavior()),
            );
        }
        Layer::ClipPath(c) => {
            // Mirror the command-path summary: clipping geometry shape (bounds +
            // point count) and behavior, so the clip is not reduced to a bare
            // "ClipPath" marker that hides every shape/quality change.
            let path = c.clip_path();
            out.push_str(&indent);
            let _ = writeln!(
                out,
                "ClipPath bounds={} pts={} clip={}",
                fmt_rect(path.compute_bounds()),
                path.commands().len(),
                fmt_clip(c.clip_behavior()),
            );
        }
        Layer::ClipSuperellipse(c) => {
            let r = c.clip_superellipse().outer_rect();
            out.push_str(&indent);
            let _ = writeln!(
                out,
                "ClipSuperellipse rect=({},{} {}x{}) clip={}",
                f(r.left()),
                f(r.top()),
                f(r.width()),
                f(r.height()),
                fmt_clip(c.clip_behavior()),
            );
        }
        Layer::Offset(o) => {
            out.push_str(&indent);
            let offset = o.offset();
            let _ = writeln!(out, "Offset dx={} dy={}", f(offset.dx), f(offset.dy));
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
            let _ = writeln!(out, "Opacity alpha={}", f(o.alpha()));
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
            let _ = writeln!(
                out,
                "ShaderMask bounds=({},{} {}x{})",
                f(r.left()),
                f(r.top()),
                f(r.width()),
                f(r.height()),
            );
        }
        Layer::BackdropFilter(b) => {
            let r = b.bounds();
            out.push_str(&indent);
            let _ = writeln!(
                out,
                "BackdropFilter bounds=({},{} {}x{})",
                f(r.left()),
                f(r.top()),
                f(r.width()),
                f(r.height()),
            );
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
    write_layer(&mut out, tree, tree.root(), 0);
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
    walk(tree, tree.root(), &mut out);
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
/// This assertion is **strict**:
/// if `pred` never matches it is always a test failure, never a silent pass.
pub fn assert_any(tree: Option<&LayerTree>, pred: impl Fn(&DrawCommandSummary) -> bool) {
    assert!(
        commands_of(tree).iter().any(pred),
        "no painted command matched the predicate:\n{}",
        snapshot_tree(tree),
    );
}
