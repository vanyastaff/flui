//! Stack-safe traversal of a [`LayerTree`].
//!
//! Both the windowed renderer (`renderer.rs::render_layer_recursive`) and the
//! headless capture path (`headless.rs::walk_layer_tree`) walk a layer tree in
//! the same order — render a node, descend into its children in paint order,
//! then run the node's own post-children cleanup. Written as recursion, that
//! order costs one Rust stack frame per level, and a stack overflow in Rust is
//! a process abort rather than a recoverable panic: a deep composited chain,
//! which `flui-layer`'s own diagnostic walkers already treat as ordinary
//! (`testing/inspect.rs` walks 10 000 levels on a 64 KiB stack), would take
//! the render path down with it.
//!
//! This module owns the traversal once, as an explicit-stack iterator, so both
//! callers get the same order and the order itself is testable without a GPU.
//! It is generic over the visitor's state rather than over the callback shape,
//! because the renderers' visit steps are not uniform — the windowed renderer
//! diverts `BackdropFilter`, `ShaderMask`, and `Follower` subtrees to
//! specialized handlers, while the headless path renders every node the same
//! way. A caller expresses that by matching on the layer inside its visitor
//! and returning [`Step::SkipChildren`] for a subtree its handler already
//! consumed.

use flui_foundation::LayerId;
use flui_layer::LayerTree;

/// What a visitor wants the walk to do after a node is visited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Continue into this node's children, then run the node's exit step.
    Descend,
    /// Do not visit this node's children, and do not run its exit step. For a
    /// handler that consumed the whole subtree itself, so the walk must not
    /// also descend into it.
    SkipSubtree,
}

/// One frame of the explicit walk stack.
///
/// `Enter` visits a node and decides whether to descend; `Exit` runs the
/// node's post-children cleanup, which is what makes the order
/// render → children → cleanup rather than render → cleanup → children.
enum Frame {
    Enter(LayerId),
    Exit(LayerId),
}

/// Walk `tree` from `root`, visiting nodes in paint order.
///
/// `enter` runs before a node's children (its `Step` decides whether there are
/// any), `exit` runs after them. A node that has been removed from the tree
/// between frames is skipped at whichever step reaches it, matching the
/// recursion this replaces, which re-resolved every id it touched.
pub(crate) fn walk_layer_tree<V: LayerVisitor>(tree: &LayerTree, root: LayerId, visitor: &mut V) {
    let mut stack = vec![Frame::Enter(root)];

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(id) => {
                let Some(node) = tree.get(id) else {
                    continue;
                };
                if visitor.enter(tree, id, node.layer()) == Step::SkipSubtree {
                    continue;
                }
                // Children are pushed in reverse so they pop in paint order,
                // and the exit frame is pushed first so it runs after all of
                // them. `Vec` as the stack means a deep chain heap-allocates
                // rather than consuming the thread's stack.
                stack.push(Frame::Exit(id));
                for &child_id in node.children().iter().rev() {
                    stack.push(Frame::Enter(child_id));
                }
            }
            Frame::Exit(id) => {
                let Some(node) = tree.get(id) else {
                    continue;
                };
                visitor.exit(tree, id, node.layer());
            }
        }
    }
}

/// The two visit steps [`walk_layer_tree`] drives.
pub(crate) trait LayerVisitor {
    /// Visit a node before its children. Return [`Step::SkipSubtree`] when
    /// this visitor has already handled the whole subtree (a diverted
    /// `BackdropFilter`/`ShaderMask`/`Follower` handler), so the walk neither
    /// descends nor runs `exit` for it.
    fn enter(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) -> Step;

    /// Run a node's post-children cleanup. Never called for a node whose
    /// `enter` returned [`Step::SkipSubtree`].
    fn exit(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The order both production walkers must produce, recorded as a string so
    /// a mismatch reads as the sequence rather than an index.
    #[derive(Default)]
    struct Recorder {
        log: Vec<String>,
    }

    impl Recorder {
        fn joined(&self) -> String {
            self.log.join(" ")
        }
    }

    impl LayerVisitor for Recorder {
        fn enter(&mut self, _tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) -> Step {
            let kind = match layer {
                flui_layer::Layer::Offset(_) => "offset",
                flui_layer::Layer::Canvas(_) => "canvas",
                flui_layer::Layer::ClipRect(_) => "clip",
                _ => "other",
            };
            self.log.push(format!("{kind}{}", id.get()));
            Step::Descend
        }

        fn exit(&mut self, _tree: &LayerTree, id: LayerId, _layer: &flui_layer::Layer) {
            self.log.push(format!("cleanup{}", id.get()));
        }
    }

    /// A visitor that consumes the subtree of the first `Canvas` node it sees,
    /// standing in for the windowed renderer's diverted handlers.
    struct DivertingVisitor {
        inner: Recorder,
    }

    impl LayerVisitor for DivertingVisitor {
        fn enter(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) -> Step {
            if matches!(layer, flui_layer::Layer::Canvas(_)) {
                self.inner.log.push(format!("handled{}", id.get()));
                return Step::SkipSubtree;
            }
            self.inner.enter(tree, id, layer)
        }

        fn exit(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) {
            self.inner.exit(tree, id, layer);
        }
    }

    fn offset() -> flui_layer::Layer {
        flui_layer::Layer::Offset(flui_layer::OffsetLayer::new(
            flui_types::geometry::Offset::ZERO,
        ))
    }

    fn canvas() -> flui_layer::Layer {
        flui_layer::Layer::Canvas(Box::new(flui_layer::CanvasLayer::new()))
    }

    /// Render → children → cleanup, children in paint order.
    ///
    /// The tree is root → [a, b], with `a` → [c]. The expected log pins the
    /// one property that makes this walker correct rather than merely
    /// stack-safe: `cleanup` for a node comes after the WHOLE subtree, not
    /// directly after its own `enter`. A recursion that cleaned up before
    /// descending (or a stack that popped the exit frame too early) would
    /// close a clip before painting inside it, which is a silently wrong
    /// picture rather than a crash.
    #[test]
    fn order_is_enter_children_exit_in_paint_order() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let a = tree.insert(offset());
        let b = tree.insert(offset());
        let c = tree.insert(canvas());
        tree.add_child(root, a);
        tree.add_child(root, b);
        tree.add_child(a, c);

        let mut rec = Recorder::default();
        walk_layer_tree(&tree, root, &mut rec);

        assert_eq!(
            rec.joined(),
            format!(
                "offset{r} offset{a} canvas{c} cleanup{c} cleanup{a} \
                 offset{b} cleanup{b} cleanup{r}",
                r = root.get(),
                a = a.get(),
                b = b.get(),
                c = c.get(),
            ),
            "paint order, with each cleanup after its own subtree"
        );
    }

    /// `SkipSubtree` neither descends nor cleans up: the diverting handler owns
    /// the subtree, so the walk must not also emit a cleanup the handler's own
    /// path is responsible for balancing.
    #[test]
    fn skip_subtree_descends_not_and_cleans_not() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let a = tree.insert(offset());
        let hidden = tree.insert(canvas());
        let inner = tree.insert(offset());
        tree.add_child(root, a);
        tree.add_child(a, hidden);
        tree.add_child(hidden, inner);

        let mut rec = DivertingVisitor {
            inner: Recorder::default(),
        };
        walk_layer_tree(&tree, root, &mut rec);

        assert_eq!(
            rec.inner.joined(),
            format!(
                "offset{r} offset{a} handled{h} cleanup{a} cleanup{r}",
                r = root.get(),
                a = a.get(),
                h = hidden.get(),
            ),
            "the diverted subtree contributes no enter for its child and no cleanup"
        );
    }

    /// The property the module exists for: a deep chain walks on a small
    /// stack, because the walk allocates its work on the heap.
    ///
    /// Same depth and stack budget as `flui-layer`'s own diagnostic-walker
    /// test (`testing/inspect.rs::walkers_survive_a_deep_chain_on_a_small_stack`),
    /// so the two walkers are held to one standard.
    #[test]
    fn a_deep_chain_survives_a_small_stack() {
        const DEPTH: usize = 10_000;

        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let mut parent = root;
        for _ in 1..DEPTH {
            let child = tree.insert(offset());
            tree.add_child(parent, child);
            parent = child;
        }

        let visited = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut rec = Recorder::default();
                walk_layer_tree(&tree, root, &mut rec);
                rec.log.len()
            })
            .expect("spawn the small-stack walker thread")
            .join()
            .expect("the walk must not overflow a small stack on a deep chain");

        assert_eq!(
            visited,
            DEPTH * 2,
            "every node entered and exited, on a 64 KiB stack"
        );
    }
}
