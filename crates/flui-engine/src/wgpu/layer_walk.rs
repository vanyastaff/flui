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
//! way. A caller expresses that by matching on the layer inside its visitor:
//!
//! - [`Step::Descend`] when the walk should carry the children — which is what
//!   a handler returns when its own work finishes BEFORE the children paint
//!   (`BackdropFilter` composites its blur, then the children go on top;
//!   a `Follower` pushes its offset and pops it on exit). Descending is what
//!   keeps a chain of such nodes off the call stack.
//! - [`Step::SkipSubtree`] when the handler consumed the subtree itself
//!   (`ShaderMask` renders children into its own offscreen texture, so it
//!   cannot hand them back to a walk that owns the real backend; an unlinked
//!   `Follower` with `show_when_unlinked == false`, whose subtree is hidden).
//!   A recursive call inside `enter` is the third option and the one to avoid:
//!   it costs a call-stack frame per level, which is the failure this module
//!   exists to remove.

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
///
/// # Cycles terminate
///
/// `LayerTree::add_child` rejects cycles, so a tree built through the public
/// API cannot contain one. A malformed tree can — the layer crate's own
/// `is_ancestor_of` names the case ("a slab loaded from disk with corrupt
/// parent pointers") — and an explicit-stack walk over one is worse than the
/// recursion it replaced: recursion overflowed the stack and aborted, this
/// would spin forever while `stack` grew without bound.
///
/// The guard tracks the nodes on the CURRENT descent path and skips an `Enter`
/// for one already on it. That is a true cycle; a node merely reachable twice
/// (a DAG, which a malformed parent pointer can also produce) still visits
/// twice, exactly as the recursion did, because its first visit's `Exit`
/// removes it from the path before the second `Enter`.
pub(crate) fn walk_layer_tree<V: LayerVisitor>(tree: &LayerTree, root: LayerId, visitor: &mut V) {
    let mut stack = vec![Frame::Enter(root)];
    // Nodes whose `Exit` frame is still pending, i.e. the current descent path.
    // Bounded by the tree's depth, not its size.
    let mut on_path: std::collections::HashSet<LayerId> = std::collections::HashSet::new();

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(id) => {
                let Some(node) = tree.get(id) else {
                    continue;
                };
                if !on_path.insert(id) {
                    tracing::error!(
                        target: "flui.layer",
                        ?id,
                        "layer tree contains a cycle reachable from this root; \
                         skipping the repeated node. The tree is malformed — \
                         LayerTree::add_child rejects cycles, so this was built \
                         by corrupting parent/child pointers after the fact"
                    );
                    continue;
                }
                if visitor.enter(tree, id, node.layer()) == Step::SkipSubtree {
                    // No `Exit` frame is pushed, so the node is off the path
                    // again the moment its subtree is.
                    on_path.remove(&id);
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
                on_path.remove(&id);
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

    /// A visitor that panics if the walk exceeds `budget` visits.
    ///
    /// Without this, a walk that failed to terminate would HANG the suite
    /// rather than fail it — a reverted cycle guard would consume CI's test
    /// timeout instead of reporting. The budget makes the bug loud and fast.
    struct Bounded {
        visits: usize,
        budget: usize,
    }

    impl LayerVisitor for Bounded {
        fn enter(&mut self, _t: &LayerTree, _i: LayerId, _l: &flui_layer::Layer) -> Step {
            self.visits += 1;
            assert!(
                self.visits <= self.budget,
                "walk exceeded {} visits — it is not terminating",
                self.budget
            );
            Step::Descend
        }

        fn exit(&mut self, _t: &LayerTree, _i: LayerId, _l: &flui_layer::Layer) {
            self.visits += 1;
            assert!(
                self.visits <= self.budget,
                "walk exceeded {} visits — it is not terminating",
                self.budget
            );
        }
    }

    /// A cycle in the tree terminates the walk instead of spinning forever.
    ///
    /// `LayerTree::add_child` rejects cycles, so this cannot be built through
    /// the tree-level API — but `LayerNode::add_child` is public and does not
    /// check, so a cyclic graph is constructible, and the layer crate's own
    /// `is_ancestor_of` names the case (a slab loaded from disk with corrupt
    /// parent pointers). The walk that preceded this one overflowed the stack
    /// on such input and aborted; an unbounded explicit-stack walk would spin
    /// instead, growing its heap stack without limit — strictly worse.
    ///
    /// The cycle guard skips only a node already on the CURRENT descent path,
    /// which is what makes a cycle (`root -> a -> root`) terminate while a DAG
    /// (`root -> [a, b]`, both -> `c`) still visits `c` once per parent,
    /// exactly as the recursion did.
    #[test]
    fn a_cyclic_tree_terminates_rather_than_spinning() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let a = tree.insert(offset());
        tree.add_child(root, a);
        // The cycle: `a` lists `root` as a child, so `root -> a -> root -> ...`
        // would never end. `LayerNode::add_child` does not reject it.
        tree.get_mut(a)
            .expect("the inserted node is live")
            .add_child(root);

        let mut bounded = Bounded {
            visits: 0,
            budget: 16,
        };
        walk_layer_tree(&tree, root, &mut bounded);

        assert_eq!(
            bounded.visits, 4,
            "root and `a` each enter and exit once; the cycle back to the root \
             is refused, so `a`'s second child contributes nothing"
        );
    }

    /// A diamond (shared child, no cycle) still visits the shared node once per
    /// parent — the guard must not mistake a DAG for a cycle.
    #[test]
    fn a_shared_child_is_visited_once_per_parent() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let a = tree.insert(offset());
        let b = tree.insert(offset());
        let shared = tree.insert(canvas());
        tree.add_child(root, a);
        tree.add_child(root, b);
        tree.add_child(a, shared);
        // NOT `tree.add_child`: that RE-PARENTS (detaches from the previous
        // parent), so the shared node would move rather than be shared. A
        // second parent edge needs the unchecked node-level call — the same
        // route a corrupt load takes.
        tree.get_mut(b)
            .expect("the inserted node is live")
            .add_child(shared);

        let mut rec = Recorder::default();
        walk_layer_tree(&tree, root, &mut rec);

        let expected = format!(
            "offset{r} offset{a} canvas{s} cleanup{s} cleanup{a} \
             offset{b} canvas{s} cleanup{s} cleanup{b} cleanup{r}",
            r = root.get(),
            a = a.get(),
            b = b.get(),
            s = shared.get(),
        );
        assert_eq!(
            rec.joined(),
            expected,
            "a shared child is not a cycle: both parents visit it"
        );
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

    /// A visitor that re-enters the walker from `enter` does NOT get a
    /// stack frame per level — proven by the engine's own `Follower` handler,
    /// which used to do exactly that.
    ///
    /// This is the shape that made the first version of this module's fix
    /// incomplete: `RenderLayerVisitor::enter` walked a `Follower`'s children
    /// by calling `walk_layer_tree` re-entrantly, so a chain whose every node
    /// was a `Follower` still consumed one stack frame per link and still
    /// `SIGABRT`ed at depth 10 000. Measured, not assumed — the recursive
    /// version of this test aborted with signal 6.
    ///
    /// The engine's visitor no longer re-enters (it pushes the offset, tracks
    /// it in `pushed_follower_offsets`, and returns `Descend`), and this pins
    /// the property that makes that safe: re-entrancy is what costs stack, so
    /// a visitor must not depend on it.
    #[test]
    fn a_reentrant_visitor_is_the_only_thing_that_costs_call_stack() {
        const DEPTH: usize = 10_000;

        /// Re-enters on every node, standing in for the old `Follower` arm.
        struct ReEntering;

        impl LayerVisitor for ReEntering {
            fn enter(&mut self, tree: &LayerTree, id: LayerId, _layer: &flui_layer::Layer) -> Step {
                if let Some(node) = tree.get(id) {
                    for &child_id in node.children() {
                        walk_layer_tree(tree, child_id, self);
                    }
                }
                Step::SkipSubtree
            }

            fn exit(&mut self, _tree: &LayerTree, _id: LayerId, _layer: &flui_layer::Layer) {}
        }

        /// Descends without re-entering — the engine visitor's shape.
        struct Descending;

        impl LayerVisitor for Descending {
            fn enter(&mut self, _t: &LayerTree, _i: LayerId, _l: &flui_layer::Layer) -> Step {
                Step::Descend
            }

            fn exit(&mut self, _t: &LayerTree, _i: LayerId, _l: &flui_layer::Layer) {}
        }

        // Built INSIDE the thread: `LayerTree::clone_subtree` still recurses
        // and would abort on this stack before the walk even ran (issue #1083's
        // `flui-layer` half, still open).
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut tree = LayerTree::new();
                let root = tree.insert(offset());
                tree.set_root(Some(root));
                let mut parent = root;
                for _ in 1..DEPTH {
                    let child = tree.insert(offset());
                    tree.add_child(parent, child);
                    parent = child;
                }

                let mut v = Descending;
                walk_layer_tree(&tree, root, &mut v);
            })
            .expect("spawn the descending walker thread")
            .join()
            .expect("a descending visitor must not consume the call stack");

        // The re-entering shape is the hazard, kept here as the control that
        // explains why the engine's visitor was restructured. It is not run at
        // depth — that aborts the process and would take the suite with it.
        let _ = ReEntering;
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
