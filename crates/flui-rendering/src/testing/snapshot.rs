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

    /// Renders the node tree as a faithful typed JSON string
    /// (via [`DiagnosticsNode::to_json`]).
    ///
    /// Typed values (Rect, Color, Float, …) are serialized as JSON objects
    /// / numbers, not as display strings.
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
    if !any_node(&root, &pred) {
        panic!(
            "no painted node matched the predicate:\n{}",
            SnapshotStrategy::text().render(&root),
        );
    }
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
/// # Deprecation
///
/// Superseded by [`is_draw_command_with_rect`], [`is_draw_command_with_shadow`],
/// and custom [`DiagnosticsNode`]-based predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[deprecated(
    since = "0.1.0",
    note = "Use `DiagnosticsNode`-based predicates with `assert_paints_node` instead."
)]
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
/// # Deprecation
///
/// Superseded by [`DiagnosticsNode`]-based predicates.
#[allow(deprecated)]
#[derive(Debug, Clone, PartialEq)]
#[deprecated(
    since = "0.1.0",
    note = "Use `DiagnosticsNode`-based predicates with `assert_paints_node` instead."
)]
pub struct DrawCommandSummary {
    /// Coarse category of the command.
    pub kind: DrawKind,
    /// Stable single-line text representation of the command.
    pub line: String,
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

    /// Mounting a `RenderColoredBox::red(40, 40)` and running a frame must
    /// produce a serialized layer tree containing `"Picture"` and a
    /// `DrawCommand` node with a `rect` property.
    #[test]
    fn serialize_simple_box_is_stable() {
        use flui_types::Size;

        use crate::objects::RenderColoredBox;
        use crate::testing::{RenderTester, box_node};

        let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
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

        use crate::objects::RenderColoredBox;
        use crate::testing::{RenderTester, box_node};

        let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
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
