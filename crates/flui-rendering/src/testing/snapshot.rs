//! Scene-snapshot utilities for the render-object test harness.
//!
//! ## Primary API (Task 6)
//!
//! [`scene_diagnostics`] walks a [`LayerTree`] and assembles a full
//! [`DiagnosticsNode`] tree: each layer becomes a node (via
//! [`Layer::to_diagnostics_node`]) and each `Picture` layer's draw-command
//! children are already embedded by the layer impl.  Sub-layer children are
//! recursed here so the final tree mirrors the compositor hierarchy.
//!
//! [`SnapshotStrategy`] wraps a render function (`text` or `json`) so callers
//! can snapshot the same tree in two formats without duplicating the walk.
//!
//! ## Deprecated API (kept for one cycle)
//!
//! The `#213 string serializer` (`serialize_layer_tree`, `write_layer`,
//! `summarize_command`, `DrawCommandSummary`, `DrawKind`) is retired.
//! `serialize_layer_tree` is kept as a thin shim over `scene_diagnostics`
//! so external crates survive one release cycle.  The old free-function
//! snapshot helpers (`snapshot_tree`, `commands_of`, `assert_any`) are also
//! shimmed.
//!
//! Callers should migrate to:
//! - `scene_diagnostics(&tree)` for the full typed tree
//! - `SnapshotStrategy::text()` / `SnapshotStrategy::json()` for serialized snapshots
//! - `assert_paints_node` for predicate-based assertions over the node tree

use flui_foundation::{DiagnosticsNode, LayerId};
use flui_layer::LayerTree;
use flui_painting::PaintStyle;
use flui_painting::display_list::{ClipOp, DisplayList, DrawCommand, Paint};
use flui_types::{
    geometry::{Matrix4, Pixels, Point, RRect, Rect},
    painting::Clip,
    styling::Color,
};

// ── Primary: scene_diagnostics ────────────────────────────────────────────────

/// Walk a [`LayerTree`] from its root and produce a [`DiagnosticsNode`] tree.
///
/// For each [`LayerNode`], calls [`Layer::to_diagnostics_node`] which gives:
/// - the layer kind name (e.g. `"Offset"`, `"Picture"`, `"ClipRect"`)
/// - per-layer typed properties (bounds, alpha, clip behavior, …)
/// - for `Picture` layers: all draw-command children already embedded as
///   `DiagnosticsNode` children (one per `DrawCommand`)
///
/// Sub-layer children (the `LayerTree`'s structural parent→child links) are
/// recursed here and pushed onto each node's `children_mut()`, producing the
/// full compositor hierarchy as a `DiagnosticsNode` tree.
///
/// # Layout of the returned tree
///
/// ```text
/// Offset            ← LayerNode::layer().to_diagnostics_node()
///   Picture         ← child in the LayerTree
///     DrawCommand   ← draw-command child, already embedded by PictureLayer
///     DrawCommand
///   ClipRect        ← another LayerTree child
///     Picture
///       DrawCommand
/// ```
///
/// # Empty tree
///
/// Returns an anonymous `DiagnosticsNode` with no children when the tree has
/// no root.
#[must_use]
pub fn scene_diagnostics(tree: &LayerTree) -> DiagnosticsNode {
    if let Some(root) = tree.root() {
        walk_layer(tree, root)
    } else {
        DiagnosticsNode::anonymous()
    }
}

/// Convenience wrapper: returns `scene_diagnostics(tree)` or an anonymous
/// node when `tree` is `None`.
#[must_use]
pub fn scene_diagnostics_tree(tree: Option<&LayerTree>) -> DiagnosticsNode {
    tree.map_or_else(DiagnosticsNode::anonymous, scene_diagnostics)
}

/// Recursively build a [`DiagnosticsNode`] for the layer at `id` and all its
/// structural descendants in the [`LayerTree`].
fn walk_layer(tree: &LayerTree, id: LayerId) -> DiagnosticsNode {
    use flui_foundation::Diagnosticable as _;

    let Some(node) = tree.get(id) else {
        return DiagnosticsNode::anonymous();
    };

    // The Layer impl produces the node name + typed properties, and for
    // Picture it also embeds draw-command children.
    let mut diag = node.layer().to_diagnostics_node();

    // Recurse into the LayerTree's structural children (sub-layers like
    // ClipRect containing a Picture).  These are distinct from the
    // draw-command children that the Picture variant already embedded.
    for &child_id in node.children() {
        diag.children_mut().push(walk_layer(tree, child_id));
    }

    diag
}

// ── SnapshotStrategy ──────────────────────────────────────────────────────────

/// Render strategy for a [`DiagnosticsNode`] snapshot.
///
/// Call [`SnapshotStrategy::text`] or [`SnapshotStrategy::json`] to obtain a
/// strategy, then pass a node to `render`:
///
/// ```rust,ignore
/// let snap = SnapshotStrategy::text().render(&scene_diagnostics(tree));
/// insta::assert_snapshot!("my_snap", snap);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SnapshotStrategy {
    kind: StrategyKind,
}

#[derive(Debug, Clone, Copy)]
enum StrategyKind {
    Text,
    Json,
}

impl SnapshotStrategy {
    /// Renders the node tree as an indented human-readable text dump
    /// (via [`DiagnosticsNode::to_string_deep`]).
    ///
    /// This is the format consumed by the snapshot goldens.
    #[must_use]
    pub const fn text() -> Self {
        Self {
            kind: StrategyKind::Text,
        }
    }

    /// Renders the node tree as a faithful typed JSON string via
    /// [`flui_foundation::DiagnosticsEnvelope::to_json_pretty`].
    ///
    /// The output is wrapped in a [`flui_foundation::DiagnosticsEnvelope`] and
    /// always carries a `"format_version"` field, conforming to
    /// `schema/diagnostics.v1.json`. Typed values (Rect, Color, Float, …) are
    /// serialized as JSON objects / numbers, not as display strings.
    #[must_use]
    pub const fn json() -> Self {
        Self {
            kind: StrategyKind::Json,
        }
    }

    /// Render a [`DiagnosticsNode`] according to this strategy.
    ///
    /// The `Json` variant wraps the node in a [`DiagnosticsEnvelope`] before
    /// serializing, so the output always carries `"format_version"` as the
    /// first field. This is the versioned contract consumed by devtools and
    /// golden-diff scripts.
    ///
    /// # Panics
    ///
    /// The `Json` variant panics if the diagnostics tree contains a
    /// non-finite float (`NaN` / `±inf`). RFC 8259 §6 forbids those values
    /// in JSON, and a scene that produces them is itself broken — panicking
    /// surfaces the real failure immediately rather than swallowing it.
    ///
    /// This panic is acceptable in the testing harness because tests run in a
    /// controlled environment. A live devtools inspector (roadmap item) must
    /// instead call [`DiagnosticsEnvelope::to_json_pretty`] directly and
    /// propagate the `Err` to the caller.
    #[must_use]
    pub fn render(&self, node: &DiagnosticsNode) -> String {
        match self.kind {
            StrategyKind::Text => node.to_string_deep(),
            StrategyKind::Json => {
                // Wrap in DiagnosticsEnvelope so every inspector JSON carries
                // `format_version`. The `testing` feature activates
                // `flui-foundation/serde`, which provides Serialize on the
                // envelope and the full diagnostics tree.
                //
                // Non-finite floats produce Err from to_json_pretty (RFC 8259
                // §6). A test that constructs NaN/inf values in a painted scene
                // is itself broken; panicking here surfaces the real failure
                // immediately rather than swallowing it.
                flui_foundation::DiagnosticsEnvelope::new(node.clone())
                    .to_json_pretty()
                    .expect("non-finite float in DiagnosticsValue: the painted scene is invalid (NaN/±inf violates RFC 8259 §6 — fix the render object that produces it)")
            }
        }
    }
}

// ── Node-based predicate helpers ──────────────────────────────────────────────

/// Panics unless at least one node in the diagnostics tree (depth-first)
/// satisfies `pred`.
///
/// Visits every node recursively.  On failure the panic message includes the
/// full text snapshot so the developer can see what was actually painted.
pub fn assert_paints_node(tree: Option<&LayerTree>, pred: impl Fn(&DiagnosticsNode) -> bool) {
    let root = scene_diagnostics_tree(tree);
    assert!(
        any_node(&root, &pred),
        "no painted node matched the predicate:\n{}",
        SnapshotStrategy::text().render(&root),
    );
}

/// Returns `true` if any node in the subtree rooted at `node` satisfies `pred`.
fn any_node(node: &DiagnosticsNode, pred: &impl Fn(&DiagnosticsNode) -> bool) -> bool {
    if pred(node) {
        return true;
    }
    node.children().iter().any(|child| any_node(child, pred))
}

/// Returns `true` if `node` is a draw-command node whose `"rect"` property
/// carries a `DiagnosticsValue::Rect` — the pattern that identifies
/// `DrawRect`, `DrawOval`, `DrawArc`, and similar rect-bounded commands.
///
/// Node names are now per-variant (e.g. `"DrawRect"`, `"DrawOval"`, `"DrawArc"`)
/// rather than the generic `"DrawCommand"`, so this predicate matches any node
/// whose name starts with `"Draw"` **and** carries a typed `"rect"` property.
/// That combination is unique to rect-bearing draw primitives.
///
/// Use this with [`assert_paints_node`] as the migration target for the retired
/// `|c| c.kind == DrawKind::Rect` predicate.
#[must_use]
pub fn is_draw_command_with_rect(node: &DiagnosticsNode) -> bool {
    use flui_foundation::DiagnosticsValue;
    node.name().is_some_and(|n| n.starts_with("Draw"))
        && node
            .find_property("rect")
            .is_some_and(|p| matches!(p.value_typed(), DiagnosticsValue::Rect { .. }))
}

/// Returns `true` if `node` is a `DrawShadow` node — identified by the
/// per-variant node name `"DrawShadow"` plus the presence of a typed
/// `"path_bounds"` rect and an `"elevation"` property.
///
/// Node names are now per-variant (`"DrawShadow"`) rather than the generic
/// `"DrawCommand"`, so this predicate checks the exact name first for
/// precision, then validates the expected properties are present.
///
/// Use this with [`assert_paints_node`] as the migration target for the retired
/// `|c| c.kind == DrawKind::Shadow` predicate.
#[must_use]
pub fn is_draw_command_with_shadow(node: &DiagnosticsNode) -> bool {
    use flui_foundation::DiagnosticsValue;
    node.name() == Some("DrawShadow")
        && node
            .find_property("path_bounds")
            .is_some_and(|p| matches!(p.value_typed(), DiagnosticsValue::Rect { .. }))
        && node.find_property("elevation").is_some()
}

// ── Deprecated: #213 string serializer shims ──────────────────────────────────
//
// These are kept for one release cycle so external crates do not break on the
// day of the upgrade.  Remove them in the next major version bump.

use flui_foundation::RenderId;

/// Serialize the full [`LayerTree`] to a stable indented text form.
///
/// # Deprecation
///
/// This function is superseded by [`scene_diagnostics`] + [`SnapshotStrategy::text`].
/// The returned text format has changed: the new format uses
/// [`DiagnosticsNode::to_string_deep`] indentation and typed-value Display
/// notation.  Callers that assert on the literal output must regenerate their
/// goldens.
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Use `scene_diagnostics(tree).to_string_deep()` instead; \
            this shim delegates to that path and the text format has changed."
)]
pub fn serialize_layer_tree(tree: &LayerTree) -> String {
    scene_diagnostics(tree).to_string_deep()
}

/// Serialize the subtree rooted at the layer boundary for `node`.
///
/// # Deprecation
///
/// Superseded by [`scene_diagnostics`].  Falls back to the full tree (same
/// approximation as before; a `RenderId → LayerId` map does not yet exist).
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Use `scene_diagnostics(tree).to_string_deep()` instead."
)]
pub fn serialize_layer_subtree(tree: &LayerTree, _node: RenderId) -> String {
    scene_diagnostics(tree).to_string_deep()
}

/// Serialize a `LayerTree` to text, or return an empty string when `None`.
///
/// # Deprecation
///
/// Superseded by [`scene_diagnostics_tree`] + [`SnapshotStrategy::text`].
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Use `SnapshotStrategy::text().render(&scene_diagnostics_tree(tree))` instead."
)]
pub fn snapshot_tree(tree: Option<&LayerTree>) -> String {
    SnapshotStrategy::text().render(&scene_diagnostics_tree(tree))
}

/// Serialize the subtree at `node`, or return an empty string when `None`.
///
/// # Deprecation
///
/// Superseded by [`scene_diagnostics_tree`].
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Use `SnapshotStrategy::text().render(&scene_diagnostics_tree(tree))` instead."
)]
pub fn snapshot_subtree(tree: Option<&LayerTree>, node: RenderId) -> String {
    let _ = node; // no RenderId→LayerId map yet
    SnapshotStrategy::text().render(&scene_diagnostics_tree(tree))
}

// ── Deprecated: DrawCommandSummary predicate API ──────────────────────────────

/// Coarse category of a drawing command.
///
/// Companion to the [`DiagnosticsNode`]-based predicates
/// ([`is_draw_command_with_rect`], [`is_draw_command_with_shadow`]): the
/// harness's `display_commands()` line/kind assertions use this summary API.
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

// ── private helpers ──────────────────────────────────────────────────────────

/// Format one `f32` to 2 decimal places, normalizing `-0.0` → `0.0`.
fn f(v: f32) -> String {
    // Stability contract: callers pass finite floats. A non-finite value would
    // format as "NaN"/"inf" and break the fixed-decimal snapshot invariant — it
    // signals a bug in the render object that produced the command, not here.
    debug_assert!(
        v.is_finite(),
        "snapshot: non-finite float in a draw command"
    );
    // Normalize negative zero before formatting.
    let v = if v == 0.0 { 0.0_f32 } else { v };
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
    if matches!(paint.style, PaintStyle::Stroke) {
        format!("{style} {color} stroke={}", f(paint.stroke_width))
    } else {
        format!("{style} {color}")
    }
}

/// Format a `Rect<Pixels>` as `"(l,t WxH)"`.
fn fmt_rect(r: Rect<Pixels>) -> String {
    format!(
        "({},{} {}x{})",
        f(r.left().get()),
        f(r.top().get()),
        f(r.width().get()),
        f(r.height().get()),
    )
}

/// Format a `Point<Pixels>` as `"(x,y)"`.
fn fmt_point(p: Point<Pixels>) -> String {
    format!("({},{})", f(p.x.get()), f(p.y.get()))
}

/// Format an `RRect` as `"(l,t WxH r=tl/tr/br/bl)"`.
///
/// Uses the `rect` field of `RRect` for geometry and the four corner radii
/// (circular approximation: `x` component of each radius).
fn fmt_rrect(rr: &RRect) -> String {
    let r = rr.rect;
    format!(
        "({},{} {}x{} r={}/{}/{}/{})",
        f(r.left().get()),
        f(r.top().get()),
        f(r.width().get()),
        f(r.height().get()),
        f(rr.top_left.x.get()),
        f(rr.top_right.x.get()),
        f(rr.bottom_right.x.get()),
        f(rr.bottom_left.x.get()),
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

/// Collect all `DrawCommandSummary` values from a `DisplayList`, recursing
/// into `ShaderMask` and `BackdropFilter` child lists.
fn collect_from_display_list(dl: &DisplayList, out: &mut Vec<DrawCommandSummary>) {
    for cmd in dl {
        match cmd {
            DrawCommand::ShaderMask { child, .. }
            | DrawCommand::BackdropFilter {
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

/// Collect every [`DrawCommandSummary`] reachable from all `Picture` layers in
/// the tree, in pre-order.
///
/// `ShaderMask` and `BackdropFilter` commands that embed a child `DisplayList`
/// are recursed so masked content is included.
///
/// This is the line/kind summary companion to [`collect_commands`] (which
/// returns typed [`DiagnosticsNode`]s); the render-object harness asserts on
/// the stable `line` strings and `DrawKind` categories.
#[must_use]
pub fn collect_command_summaries(tree: &LayerTree) -> Vec<DrawCommandSummary> {
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

/// Collect every [`DrawCommandSummary`] reachable from `tree`, or return an
/// empty `Vec` when `tree` is `None`.
#[must_use]
pub fn command_summaries_of(tree: Option<&LayerTree>) -> Vec<DrawCommandSummary> {
    tree.map(collect_command_summaries).unwrap_or_default()
}

/// Collect all draw-command diagnostics nodes from a layer tree.
///
/// # Deprecation
///
/// Superseded by [`scene_diagnostics`] + walking the returned
/// [`DiagnosticsNode`] tree with custom predicates or [`assert_paints_node`].
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Walk `scene_diagnostics(tree)` children instead; \
            this shim returns all command nodes depth-first."
)]
pub fn collect_commands(tree: &LayerTree) -> Vec<DiagnosticsNode> {
    let root = scene_diagnostics(tree);
    let mut out = Vec::new();
    collect_command_nodes(&root, &mut out);
    out
}

/// Collect all draw-command nodes from the subtree at `node` into `out`.
fn collect_command_nodes(node: &DiagnosticsNode, out: &mut Vec<DiagnosticsNode>) {
    // A "command node" is any node whose name starts with a known draw-command
    // prefix.  Layer nodes (Offset, Picture, ClipRect, Opacity, …) are skipped.
    if node.name().is_some_and(|n| {
        n.starts_with("Draw")
            || n.starts_with("Clip")
            || n.starts_with("Save")
            || n.starts_with("Restore")
            || n.starts_with("Shader")
            || n.starts_with("Backdrop")
    }) {
        out.push(node.clone());
    }
    for child in node.children() {
        collect_command_nodes(child, out);
    }
}

/// Collect draw-command nodes from an optional layer tree.
///
/// # Deprecation
///
/// Superseded by [`scene_diagnostics_tree`] + a manual depth-first walk.
#[must_use]
#[deprecated(
    since = "0.1.0",
    note = "Walk `scene_diagnostics_tree(tree)` children instead."
)]
pub fn commands_of(tree: Option<&LayerTree>) -> Vec<DiagnosticsNode> {
    tree.map_or_else(Vec::new, |t| {
        #[allow(deprecated)] // calling our own shim
        collect_commands(t)
    })
}

/// Assert that at least one draw-command node in the tree satisfies `pred`.
///
/// # Deprecation
///
/// Superseded by [`assert_paints_node`], which accepts the same predicate
/// signature but operates directly on [`DiagnosticsNode`] values.
///
/// # Panics
///
/// Panics when no node satisfies `pred`, with a text snapshot in the message.
#[deprecated(
    since = "0.1.0",
    note = "Use `assert_paints_node(tree, pred)` instead."
)]
pub fn assert_any(tree: Option<&LayerTree>, pred: impl Fn(&DiagnosticsNode) -> bool) {
    assert_paints_node(tree, pred);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use flui_foundation::DiagnosticsNode;
    use flui_layer::{Layer, LayerTree, OffsetLayer, PictureLayer};
    use flui_painting::{Canvas, display_list::Paint};
    use flui_types::{
        geometry::{Pixels, Rect, px},
        painting::Path,
        styling::Color,
    };

    use super::{is_draw_command_with_rect, is_draw_command_with_shadow, scene_diagnostics};

    fn rect_px(x: f32, y: f32, w: f32, h: f32) -> Rect<Pixels> {
        Rect::from_xywh(px(x), px(y), px(w), px(h))
    }

    /// A small Offset > Picture > DrawRect tree must produce:
    /// - root node named "Offset"
    /// - one child node named "Picture"
    /// - one grandchild node named "DrawCommand" (the DrawRect)
    #[test]
    fn scene_diagnostics_assembles_hierarchy() {
        let mut tree = LayerTree::new();

        // Build a DrawRect display list via the public Canvas API.
        let mut canvas = Canvas::new();
        canvas.draw_rect(rect_px(0.0, 0.0, 40.0, 40.0), &Paint::fill(Color::RED));
        let dl = canvas.finish();

        // Build Picture layer carrying the display list.
        let picture = PictureLayer::new(dl);
        let picture_id = tree.insert(Layer::Picture(Box::new(picture)));

        // Build Offset layer containing the Picture layer.
        let offset_id = tree.insert(Layer::Offset(OffsetLayer::from_xy(0.0, 0.0)));
        tree.set_root(Some(offset_id));
        tree.add_child(offset_id, picture_id);

        let root = scene_diagnostics(&tree);

        // Root must be "Offset".
        assert_eq!(
            root.name(),
            Some("Offset"),
            "root node must be named Offset; got: {:?}",
            root.name()
        );

        // Must have exactly one structural child: the Picture layer.
        let layer_children: Vec<&DiagnosticsNode> = root
            .children()
            .iter()
            .filter(|n| n.name() == Some("Picture"))
            .collect();
        assert_eq!(
            layer_children.len(),
            1,
            "Offset must have exactly one Picture child; got: {:?}",
            root.children().iter().map(|n| n.name()).collect::<Vec<_>>()
        );

        let picture_node = layer_children[0];

        // Picture must have exactly one command child: the DrawRect (named
        // "DrawRect" by the per-variant Diagnosticable override).
        let cmd_children: Vec<&DiagnosticsNode> = picture_node
            .children()
            .iter()
            .filter(|n| n.name() == Some("DrawRect"))
            .collect();
        assert_eq!(
            cmd_children.len(),
            1,
            "Picture must have exactly one DrawRect child; got: {:?}",
            picture_node
                .children()
                .iter()
                .map(|n| n.name())
                .collect::<Vec<_>>()
        );

        // The DrawCommand child must have a "rect" property.
        let cmd_node = cmd_children[0];
        assert!(
            cmd_node.find_property("rect").is_some(),
            "DrawCommand child must have a 'rect' property for DrawRect"
        );
    }

    /// `is_draw_command_with_rect` must return true for a DrawRect node and
    /// false for a DrawShadow node.
    #[test]
    fn predicate_is_draw_command_with_rect() {
        let mut tree = LayerTree::new();
        let mut canvas = Canvas::new();
        canvas.draw_rect(rect_px(0.0, 0.0, 40.0, 40.0), &Paint::fill(Color::RED));
        let id = tree.insert(Layer::Picture(Box::new(PictureLayer::new(canvas.finish()))));
        tree.set_root(Some(id));

        let root = scene_diagnostics(&tree);
        // root is "Picture"; its first child is the DrawCommand node.
        assert!(
            root.children().iter().any(is_draw_command_with_rect),
            "scene_diagnostics must expose a DrawRect-like node"
        );
    }

    /// `is_draw_command_with_shadow` must return true for a DrawShadow node.
    #[test]
    fn predicate_is_draw_command_with_shadow() {
        let mut path = Path::new();
        path.add_rect(rect_px(0.0, 0.0, 40.0, 40.0));

        let mut tree = LayerTree::new();
        let mut canvas = Canvas::new();
        canvas.draw_shadow(&path, Color::BLACK, 4.0);
        let id = tree.insert(Layer::Picture(Box::new(PictureLayer::new(canvas.finish()))));
        tree.set_root(Some(id));

        let root = scene_diagnostics(&tree);
        assert!(
            root.children().iter().any(is_draw_command_with_shadow),
            "scene_diagnostics must expose a DrawShadow-like node"
        );
        assert!(
            !root.children().iter().any(is_draw_command_with_rect),
            "DrawShadow must not match the rect predicate"
        );
    }

    // ── LayerTree serialization tests (kept for compile-compatibility) ─────────

    mod layer_tree_helpers {
        use flui_tree::Leaf;
        use flui_types::{Color, Point, Rect, Size, geometry::px};

        use crate::{
            context::BoxLayoutContext, parent_data::BoxParentData, pipeline::Paint,
            traits::RenderBox,
        };

        /// Minimal leaf that fills its area with a solid color.
        /// Replaces `RenderColoredBox` in serialization harness tests.
        #[derive(Debug)]
        pub(super) struct RedBox {
            size: Size,
        }

        impl RedBox {
            pub(super) fn fixed(width: f32, height: f32) -> Self {
                Self {
                    size: Size::new(px(width), px(height)),
                }
            }
        }

        impl flui_foundation::Diagnosticable for RedBox {}

        impl RenderBox for RedBox {
            type Arity = Leaf;
            type ParentData = BoxParentData;

            fn perform_layout(
                &mut self,
                ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
            ) -> Size {
                ctx.constraints().constrain(self.size)
            }

            fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Leaf>) {
                let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
                ctx.canvas().draw_rect(
                    rect,
                    &Paint::fill(Color::from_rgba_f32_array([1.0, 0.0, 0.0, 1.0])),
                );
            }
        }
    }

    use layer_tree_helpers::RedBox;

    /// Mounting a `RenderColoredBox::red(40, 40)` and running a frame must
    /// produce a serialized layer tree containing `"Picture"` and a
    /// `DrawCommand` node with a `rect` property.
    #[test]
    fn serialize_simple_box_is_stable() {
        use flui_types::Size;

        use crate::testing::{RenderTester, box_node};

        let run = RenderTester::mount(box_node(RedBox::fixed(40.0, 40.0)))
            .with_size(Size::new(px(40.0), px(40.0)))
            .run_frame();

        let tree = run
            .layer_tree()
            .expect("RenderColoredBox must produce a layer tree");
        let root = scene_diagnostics(tree);
        let text = root.to_string_deep();

        assert!(
            text.contains("Picture"),
            "scene_diagnostics text must contain a Picture layer; got:\n{text}"
        );
        // Per-variant node names appear in the text output (e.g. "DrawRect").
        assert!(
            text.contains("DrawRect"),
            "scene_diagnostics text must contain per-variant DrawRect node; got:\n{text}"
        );
    }

    /// `is_draw_command_with_rect` on a `RenderColoredBox` frame must find a
    /// match in the diagnostics tree (replacing the old `DrawKind::Rect` check).
    #[test]
    fn collect_commands_red_box_first_is_rect() {
        use flui_types::Size;

        use crate::testing::{RenderTester, box_node};

        let run = RenderTester::mount(box_node(RedBox::fixed(40.0, 40.0)))
            .with_size(Size::new(px(40.0), px(40.0)))
            .run_frame();

        let tree = run
            .layer_tree()
            .expect("RenderColoredBox must produce a layer tree");
        let root = scene_diagnostics(tree);

        // Walk the full node tree looking for a DrawRect-like command node.
        fn find_rect(node: &DiagnosticsNode) -> bool {
            if is_draw_command_with_rect(node) {
                return true;
            }
            node.children().iter().any(find_rect)
        }

        assert!(
            find_rect(&root),
            "scene_diagnostics must expose a DrawRect node for a painted box"
        );
    }
}
