//! Stack-safe traversal of a [`LayerTree`].
//!
//! Both the windowed renderer and the headless capture path walk a layer tree in
//! the same order — render a node, descend into its children in paint order,
//! then run the node's own post-children cleanup. Written as recursion, that
//! order costs one Rust stack frame per level, and a stack overflow in Rust is
//! a process abort rather than a recoverable panic: a deep composited chain,
//! which `flui-layer`'s own diagnostic walkers already treat as ordinary
//! (`testing/inspect.rs` walks 10 000 levels on a 64 KiB stack), would take
//! the render path down with it.
//!
//! The explicit stack owns traversal order; `record_layer_tree` is the shared
//! window/capture recorder. Shader masks and backdrops lower into ordered IR;
//! followers resolve against the original tree and scope their offset until exit.
//! GPU replay chooses each effect's active attachment after recording.

use flui_foundation::LayerId;
use flui_layer::LayerTree;

/// What a visitor wants the walk to do after a node is visited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Continue into this node's children, then run the node's exit step.
    Descend,
    /// Do not visit this node's children, and do not run its exit step. For a
    /// hidden, empty or failed subtree whose enter step acquired no state
    /// requiring cleanup.
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
/// any), `exit` runs after them. An id the tree does not know is skipped at
/// whichever step reaches it.
///
/// Termination needs no guard: a `LayerTree` is append-only and links a node
/// only in the call that mints its id (`LayerTree::push_child`), so neither a
/// cycle nor a doubly-parented node is representable, and every node is
/// entered exactly once.
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

pub(crate) trait LayerVisitor {
    /// Visit a node before its children. Return [`Step::SkipSubtree`] when
    /// this subtree needs no traversal or cleanup, so the walk neither
    /// descends nor runs `exit` for it.
    fn enter(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) -> Step;

    /// Run a node's post-children cleanup. Never called for a node whose
    /// `enter` returned [`Step::SkipSubtree`].
    fn exit(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer);
}

/// Record every consumer through the same layer semantics; GPU effects remain ordered IR.
pub(crate) fn record_layer_tree(
    tree: &LayerTree,
    root: LayerId,
    backend: &mut crate::layer_dispatcher::LayerDispatcher<'_>,
) -> crate::EngineResult<()> {
    struct Recorder<'a, 'b> {
        backend: &'a mut crate::layer_dispatcher::LayerDispatcher<'b>,
        error: Option<crate::EngineError>,
        followers: Vec<LayerId>,
    }
    impl LayerVisitor for Recorder<'_, '_> {
        fn enter(&mut self, tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) -> Step {
            use crate::{layer_render::LayerRender, layer_state_stack::LayerStateStack};
            if self.error.is_some() {
                return Step::SkipSubtree;
            }
            if let Err(error) = self.backend.painter().recording_result() {
                self.error = Some(error);
                return Step::SkipSubtree;
            }
            self.backend.flush_active_transform();
            match layer {
                flui_layer::Layer::Follower(_) => {
                    let Some(offset) = flui_layer::resolve_follower_offset(tree, id) else {
                        return Step::SkipSubtree;
                    };
                    self.backend.push_offset(offset);
                    self.followers.push(id);
                }
                flui_layer::Layer::BackdropFilter(layer) => {
                    if let Err(error) = self.backend.painter_mut().record_backdrop_filter(
                        layer.bounds(),
                        layer.filter(),
                        layer.blend_mode(),
                    ) {
                        self.error = Some(error);
                        return Step::SkipSubtree;
                    }
                }
                flui_layer::Layer::ShaderMask(layer) => {
                    match self
                        .backend
                        .painter_mut()
                        .begin_shader_mask(layer.bounds(), layer.shader())
                    {
                        Ok(true) => {}
                        Ok(false) => return Step::SkipSubtree,
                        Err(error) => {
                            self.error = Some(error);
                            return Step::SkipSubtree;
                        }
                    }
                }
                _ => layer.render(self.backend),
            }
            if let Err(error) = self.backend.painter().recording_result() {
                self.error = Some(error);
            }
            Step::Descend
        }
        fn exit(&mut self, _tree: &LayerTree, id: LayerId, layer: &flui_layer::Layer) {
            use crate::{layer_render::LayerRender, layer_state_stack::LayerStateStack};
            self.backend.flush_active_transform();
            match layer {
                flui_layer::Layer::Follower(_) => {
                    if self.followers.last() == Some(&id) {
                        self.followers.pop();
                        self.backend.pop_transform();
                    }
                }
                flui_layer::Layer::ShaderMask(layer) => {
                    if let Err(error) = self
                        .backend
                        .painter_mut()
                        .end_shader_mask(layer.shader(), layer.blend_mode())
                        && self.error.is_none()
                    {
                        self.error = Some(error);
                    }
                }
                flui_layer::Layer::BackdropFilter(_) => {}
                _ => layer.cleanup(self.backend),
            }
            if self.error.is_none()
                && let Err(error) = self.backend.painter().recording_result()
            {
                self.error = Some(error);
            }
        }
    }
    let mut recorder = Recorder {
        backend,
        error: None,
        followers: Vec::new(),
    };
    walk_layer_tree(tree, root, &mut recorder);
    recorder.error.map_or(Ok(()), Err)
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

    fn offset() -> flui_layer::Layer {
        flui_layer::Layer::Offset(flui_layer::OffsetLayer::new(
            flui_foundation::geometry::Offset::ZERO,
        ))
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

        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let mut parent = root;
        for _ in 1..DEPTH {
            parent = tree.push_child(parent, offset());
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
