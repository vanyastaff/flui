//! Stack-safe traversal of what a [`Scene`](flui_layer::Scene) holds: a
//! [`LayerTree`], or a retained [`SceneVersion`] walked in place (ADR-0304 §9).
//!
//! Both the windowed renderer and the headless capture path walk a scene in
//! the same order — render a node, descend into its children in paint order,
//! then run the node's own post-children cleanup. Written as recursion, that
//! order costs one Rust stack frame per level, and a stack overflow in Rust is
//! a process abort rather than a recoverable panic: a deep composited chain,
//! which `flui-layer`'s own diagnostic walkers already treat as ordinary
//! (`testing/inspect.rs` walks 10 000 levels on a 64 KiB stack), would take
//! the render path down with it.
//!
//! A version is walked as the tree `SceneVersion::flatten` would build from
//! it: each node an offset at its placement, each effect entry its layer over
//! the entries under it, each chunk its recording, each slot the child node
//! where it stands. A partial frame passes its damage, and a slot whose
//! subtree cannot reach it is skipped ([`SceneVersion::subtree_cull`]): what
//! the walk costs follows what the damage reaches, not the scene's size.
//!
//! The explicit stack owns traversal order; `record_scene` is the shared
//! window/capture recorder. Shader masks and backdrops lower into ordered IR;
//! followers resolve against the scene they stand in and scope their offset
//! until exit. GPU replay chooses each effect's active attachment after
//! recording.

use flui_foundation::LayerId;
use flui_foundation::geometry::{Matrix4, Rect};
use flui_layer::{
    Entry, EntryAddress, EntryIndex, Layer, LayerTree, OffsetLayer, SceneContent, SceneNodeId,
    SceneVersion,
};
use flui_painting::display_list::DisplayList;
use smallvec::SmallVec;

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

/// What the walk visits.
pub(crate) enum Item<'a> {
    /// A layer of a tree, or the layer of a version's effect entry.
    Layer(&'a Layer),
    /// A version's node, at its placement.
    Placed(OffsetLayer),
    /// A version's chunk: its recording.
    Chunk(&'a DisplayList),
}

/// Where a visited item stands, for what is resolved against the scene
/// around it.
#[derive(Clone, Copy)]
pub(crate) enum Site<'a> {
    /// A layer of a tree.
    Tree(&'a LayerTree, LayerId),
    /// An entry of a version's node.
    Entry(&'a SceneVersion, EntryAddress),
    /// A version's node itself.
    Node,
}

impl Site<'_> {
    /// The transform that lands a follower standing here on its leader;
    /// `None` when it draws nothing, or nothing here is a follower.
    pub(crate) fn follower_transform(self) -> Option<Matrix4> {
        match self {
            Self::Tree(tree, id) => flui_layer::resolve_follower_transform(tree, id),
            Self::Entry(version, at) => version.follower_transform(at),
            Self::Node => None,
        }
    }

    /// An identity for matching a follower's enter with its exit.
    fn key(self) -> Option<SiteKey> {
        match self {
            Self::Tree(_, id) => Some(SiteKey::Layer(id)),
            Self::Entry(_, at) => Some(SiteKey::Entry(at)),
            Self::Node => None,
        }
    }
}

/// [`Site`] without the scene it borrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SiteKey {
    Layer(LayerId),
    Entry(EntryAddress),
}

pub(crate) trait SceneVisitor {
    /// Visit an item before its children. Return [`Step::SkipSubtree`] when
    /// this subtree needs no traversal or cleanup, so the walk neither
    /// descends nor runs `exit` for it.
    fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step;

    /// Run an item's post-children cleanup. Never called for an item whose
    /// `enter` returned [`Step::SkipSubtree`].
    fn exit(&mut self, site: Site<'_>, item: &Item<'_>);
}

/// Walks `scene` from its root in paint order. A version's slot whose
/// subtree cannot reach `cull`, in surface pixels, is skipped; a tree is
/// walked whole.
pub(crate) fn walk_scene<V: SceneVisitor>(
    scene: SceneContent<'_>,
    cull: Option<Rect<f64>>,
    visitor: &mut V,
) {
    match scene {
        SceneContent::Tree(tree) => walk_layer_tree(tree, tree.root(), visitor),
        SceneContent::Version(version) => walk_version(version, cull, visitor),
    }
}

/// One frame of the explicit tree walk stack.
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
pub(crate) fn walk_layer_tree<V: SceneVisitor>(tree: &LayerTree, root: LayerId, visitor: &mut V) {
    let mut stack = vec![Frame::Enter(root)];
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(id) => {
                let Some(node) = tree.get(id) else {
                    continue;
                };
                if visitor.enter(Site::Tree(tree, id), &Item::Layer(node.layer()))
                    == Step::SkipSubtree
                {
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
                visitor.exit(Site::Tree(tree, id), &Item::Layer(node.layer()));
            }
        }
    }
}

/// An effect entry a node's walk has opened: whether its `enter` descended,
/// so its exit runs and the entries under it are visited.
#[derive(Clone, Copy)]
struct Open {
    entry: EntryIndex,
    entered: bool,
}

/// One frame of the explicit version walk stack.
enum Visit {
    /// A node, entered at its placement.
    Node(SceneNodeId),
    /// The rest of a node's entries from `next`, under the effects `open`.
    Entries {
        node: SceneNodeId,
        next: usize,
        open: SmallVec<[Open; 8]>,
    },
    /// A node's exit, after its entries.
    Leave(SceneNodeId),
}

/// Walk `version` from its root, visiting what `flatten` would build in the
/// same order, without building it. A slot's subtree that cannot reach
/// `cull` is skipped.
///
/// Termination: a version's slots form a tree (`SceneOwner::apply` refuses a
/// cycle and a second parent), so every node is entered at most once.
fn walk_version<V: SceneVisitor>(version: &SceneVersion, cull: Option<Rect<f64>>, visitor: &mut V) {
    let mut cull = cull.map(|rect| version.subtree_cull(rect));
    let mut stack = vec![Visit::Node(version.root())];
    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Node(id) => {
                let Some(node) = version.node(id) else {
                    continue;
                };
                let placed = Item::Placed(OffsetLayer::new(node.placement().offset()));
                if visitor.enter(Site::Node, &placed) == Step::SkipSubtree {
                    continue;
                }
                stack.push(Visit::Leave(id));
                stack.push(Visit::Entries {
                    node: id,
                    next: 0,
                    open: SmallVec::new(),
                });
            }
            Visit::Leave(id) => {
                if let Some(node) = version.node(id) {
                    let placed = Item::Placed(OffsetLayer::new(node.placement().offset()));
                    visitor.exit(Site::Node, &placed);
                }
            }
            Visit::Entries {
                node: id,
                next,
                mut open,
            } => {
                let Some(node) = version.node(id) else {
                    continue;
                };
                let content = node.content();
                let entries = content.entries();
                let mut position = next;
                let mut descended = false;
                while let Some(entry) = entries.get(position) {
                    let index = EntryIndex::try_from(position)
                        .expect("BUG: a boundary's entries are indexed by u32");
                    let at = EntryAddress {
                        node: id,
                        entry: index,
                    };
                    // The entries are in paint order and an effect's scope is
                    // contiguous: an entry whose parent is not the innermost
                    // open effect closes the effects above its parent.
                    close_until(version, id, &mut open, entry.parent(), visitor);
                    position += 1;
                    if open.last().is_some_and(|top| !top.entered) {
                        if matches!(entry, Entry::Effect { .. }) {
                            open.push(Open {
                                entry: index,
                                entered: false,
                            });
                        }
                        continue;
                    }
                    match entry {
                        Entry::Effect { .. } => {
                            let layer = content
                                .effect_layer(index)
                                .expect("BUG: a boundary's effect entry holds its layer");
                            let entered = visitor
                                .enter(Site::Entry(version, at), &Item::Layer(layer))
                                == Step::Descend;
                            open.push(Open {
                                entry: index,
                                entered,
                            });
                        }
                        Entry::Chunk { chunk, .. } => {
                            let item = Item::Chunk(chunk.commands());
                            if visitor.enter(Site::Entry(version, at), &item) == Step::Descend {
                                visitor.exit(Site::Entry(version, at), &item);
                            }
                        }
                        Entry::Slot { node: child, .. } => {
                            if cull.as_mut().is_some_and(|cull| !cull.meets(*child)) {
                                continue;
                            }
                            stack.push(Visit::Entries {
                                node: id,
                                next: position,
                                open: std::mem::take(&mut open),
                            });
                            stack.push(Visit::Node(*child));
                            descended = true;
                            break;
                        }
                    }
                }
                if !descended {
                    close_until(version, id, &mut open, None, visitor);
                }
            }
        }
    }
}

/// Exits the open effects of `node` above `parent`, innermost first.
fn close_until<V: SceneVisitor>(
    version: &SceneVersion,
    node: SceneNodeId,
    open: &mut SmallVec<[Open; 8]>,
    parent: Option<EntryIndex>,
    visitor: &mut V,
) {
    while let Some(&top) = open.last()
        && Some(top.entry) != parent
    {
        open.pop();
        if !top.entered {
            continue;
        }
        let layer = version
            .node(node)
            .and_then(|held| held.content().effect_layer(top.entry))
            .expect("BUG: an entered effect's node and layer outlive its scope");
        let at = EntryAddress {
            node,
            entry: top.entry,
        };
        visitor.exit(Site::Entry(version, at), &Item::Layer(layer));
    }
}

/// Record every consumer through the same layer semantics; GPU effects remain
/// ordered IR. A partial frame passes its damage as `cull`.
pub(crate) fn record_scene(
    scene: SceneContent<'_>,
    cull: Option<Rect<f64>>,
    backend: &mut crate::layer_dispatcher::LayerDispatcher<'_>,
) -> crate::EngineResult<()> {
    struct Recorder<'a, 'b> {
        backend: &'a mut crate::layer_dispatcher::LayerDispatcher<'b>,
        error: Option<crate::EngineError>,
        followers: Vec<SiteKey>,
    }
    impl Recorder<'_, '_> {
        fn enter_layer(&mut self, site: Site<'_>, layer: &Layer) -> Step {
            use crate::layer_render::LayerRender;
            use crate::layer_state_stack::LayerStateStack;
            match layer {
                Layer::Follower(_) => {
                    let (Some(transform), Some(key)) = (site.follower_transform(), site.key())
                    else {
                        return Step::SkipSubtree;
                    };
                    self.backend.push_transform(&transform);
                    self.followers.push(key);
                }
                Layer::BackdropFilter(layer) => {
                    if let Err(error) = self.backend.painter_mut().record_backdrop_filter(
                        layer.bounds(),
                        layer.filter(),
                        layer.blend_mode(),
                        layer.group(),
                    ) {
                        self.error = Some(error);
                        return Step::SkipSubtree;
                    }
                }
                Layer::ShaderMask(layer) => {
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
            Step::Descend
        }

        fn exit_layer(&mut self, site: Site<'_>, layer: &Layer) {
            use crate::layer_render::LayerRender;
            use crate::layer_state_stack::LayerStateStack;
            match layer {
                Layer::Follower(_) => {
                    if site.key().is_some() && self.followers.last() == site.key().as_ref() {
                        self.followers.pop();
                        self.backend.pop_transform();
                    }
                }
                Layer::ShaderMask(layer) => {
                    if let Err(error) = self
                        .backend
                        .painter_mut()
                        .end_shader_mask(layer.shader(), layer.blend_mode())
                        && self.error.is_none()
                    {
                        self.error = Some(error);
                    }
                }
                Layer::BackdropFilter(_) => {}
                _ => layer.cleanup(self.backend),
            }
        }
    }
    impl SceneVisitor for Recorder<'_, '_> {
        fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step {
            use crate::layer_render::LayerRender;
            if self.error.is_some() {
                return Step::SkipSubtree;
            }
            if let Err(error) = self.backend.painter().recording_result() {
                self.error = Some(error);
                return Step::SkipSubtree;
            }
            self.backend.flush_active_transform();
            let step = match item {
                Item::Layer(layer) => self.enter_layer(site, layer),
                Item::Placed(offset) => {
                    offset.render(self.backend);
                    Step::Descend
                }
                Item::Chunk(commands) => {
                    crate::dispatch::dispatch_commands(*commands, self.backend);
                    Step::Descend
                }
            };
            if step == Step::Descend
                && let Err(error) = self.backend.painter().recording_result()
            {
                self.error = Some(error);
            }
            step
        }

        fn exit(&mut self, site: Site<'_>, item: &Item<'_>) {
            use crate::layer_render::LayerRender;
            self.backend.flush_active_transform();
            match item {
                Item::Layer(layer) => self.exit_layer(site, layer),
                Item::Placed(offset) => offset.cleanup(self.backend),
                Item::Chunk(_) => {}
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
    walk_scene(scene, cull, &mut recorder);
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

    impl SceneVisitor for Recorder {
        fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step {
            let kind = match item {
                Item::Layer(flui_layer::Layer::Offset(_)) => "offset",
                Item::Layer(flui_layer::Layer::Canvas(_)) => "canvas",
                Item::Layer(flui_layer::Layer::ClipRect(_)) => "clip",
                Item::Layer(_) => "other",
                Item::Placed(_) => "node",
                Item::Chunk(_) => "chunk",
            };
            self.log.push(format!("{kind}{}", label(site)));
            Step::Descend
        }

        fn exit(&mut self, site: Site<'_>, _item: &Item<'_>) {
            self.log.push(format!("cleanup{}", label(site)));
        }
    }

    fn label(site: Site<'_>) -> String {
        match site {
            Site::Tree(_, id) => id.get().to_string(),
            Site::Entry(_, at) => format!("@{}", at.entry.get()),
            Site::Node => String::new(),
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

    /// What a walk visits, as text two walks can be compared by: a node's
    /// placement and an offset layer read alike, a chunk and the picture
    /// that holds its recording read alike, and a follower names the
    /// transform it resolves.
    #[derive(Default)]
    struct Trace {
        log: Vec<String>,
        entered: usize,
    }

    impl SceneVisitor for Trace {
        fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step {
            self.entered += 1;
            let line = match item {
                Item::Placed(offset) | Item::Layer(flui_layer::Layer::Offset(offset)) => {
                    format!("offset {:?}", offset.offset())
                }
                Item::Chunk(commands) => format!("chunk {:p}", *commands),
                Item::Layer(flui_layer::Layer::Picture(picture)) => {
                    format!("chunk {:p}", picture.picture())
                }
                Item::Layer(layer @ flui_layer::Layer::Follower(_)) => {
                    // The version and the tree resolve a chain in their own
                    // order of products: equal to well under a pixel.
                    let transform = site
                        .follower_transform()
                        .map(|matrix| matrix.m.map(|value| format!("{value:.6}")).join(" "));
                    format!("{layer:?} at {transform:?}")
                }
                Item::Layer(layer) => format!("{layer:?}"),
            };
            self.log.push(line);
            Step::Descend
        }

        fn exit(&mut self, _site: Site<'_>, _item: &Item<'_>) {
            self.log.push("exit".to_owned());
        }
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % n as u64).expect("below a usize")
        }

        fn signed(&mut self, scale: f64) -> f64 {
            let unit = f64::from(u32::try_from(self.next() % 1_000).expect("below 1 000"));
            (unit / 500.0 - 1.0) * scale
        }
    }

    const SURFACE: flui_layer::SurfaceKey = flui_layer::SurfaceKey {
        size: (400, 400),
        generation: flui_foundation::SurfaceGeneration::ZERO,
    };

    fn square(
        owner: usize,
        version: flui_layer::ContentVersion,
        at: (f64, f64),
    ) -> std::sync::Arc<flui_layer::Chunk> {
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(at.0, at.1, 10.0, 10.0),
            &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
        );
        std::sync::Arc::new(flui_layer::Chunk::new(
            flui_foundation::RenderId::new(owner),
            0,
            version,
            canvas.finish(),
        ))
    }

    fn random_effect(
        rng: &mut Rng,
        link: flui_layer::LayerLink,
        led: &mut bool,
    ) -> flui_layer::Layer {
        use flui_foundation::geometry::{Offset, Size};
        use flui_layer::{ClipRectLayer, FollowerLayer, Layer, LeaderLayer, TransformLayer};
        match rng.below(5) {
            0 => Layer::from(OffsetLayer::new(Offset::new(
                rng.signed(40.0),
                rng.signed(40.0),
            ))),
            1 => Layer::from(TransformLayer::new(
                Matrix4::translation(rng.signed(30.0), rng.signed(30.0), 0.0)
                    * Matrix4::rotation_z(rng.signed(1.0)),
            )),
            2 => Layer::from(ClipRectLayer::new(
                Rect::from_xywh(rng.signed(50.0), rng.signed(50.0), 60.0, 60.0),
                flui_painting::paint::Clip::HardEdge,
            )),
            3 if !std::mem::replace(led, true) => Layer::from(LeaderLayer::with_offset(
                link,
                Size::new(20.0, 20.0),
                Offset::new(rng.signed(30.0), rng.signed(30.0)),
            )),
            3 => Layer::from(OffsetLayer::zero()),
            _ => Layer::from(
                FollowerLayer::new(link)
                    .with_target_offset(Offset::new(rng.signed(20.0), rng.signed(20.0)))
                    .with_size(Size::new(10.0, 10.0)),
            ),
        }
    }

    /// A random version of `count` boundaries, nested effects, chunks,
    /// leaders and followers.
    fn random_version(rng: &mut Rng, count: usize) -> SceneVersion {
        use flui_layer::{BoundaryContent, Placement, SceneOp, SceneOwner};
        let link = flui_layer::LayerLink::new();
        let mut led = false;
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let mut nodes: Vec<SceneNodeId> = vec![writer.current().root()];
        let mut parents = vec![0];
        for index in 1..count {
            nodes.push(writer.mint_node());
            parents.push(rng.below(index));
        }
        let mut publishes = Vec::new();
        for (index, &node) in nodes.iter().enumerate() {
            let mut content = BoundaryContent::builder();
            let mut ordinal = 0;
            let children: Vec<usize> = (1..count).filter(|&c| parents[c] == index).collect();
            let mut pending = children.iter().copied().peekable();
            for _ in 0..rng.below(10) {
                match rng.below(4) {
                    0 => {
                        let layer = random_effect(rng, link, &mut led);
                        content.push_effect(
                            layer,
                            flui_foundation::RenderId::new(index + 1),
                            ordinal,
                        );
                        ordinal += 1;
                    }
                    1 => {
                        content.pop_effect();
                    }
                    _ => {
                        if let Some(child) = pending.next() {
                            content.slot(nodes[child]);
                        } else {
                            let version = writer.next_content_version().expect("a version");
                            let at = (rng.signed(100.0) + 150.0, rng.signed(100.0) + 150.0);
                            content.chunk(square(index + 1, version, at));
                        }
                    }
                }
            }
            for child in pending {
                content.slot(nodes[child]);
            }
            publishes.push(SceneOp::Publish {
                node,
                content: std::sync::Arc::new(content.finish()),
            });
        }
        for op in publishes.into_iter().rev() {
            writer.push(op);
        }
        for &node in nodes.iter().skip(1).rev() {
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(flui_foundation::geometry::Offset::new(
                    rng.signed(60.0),
                    rng.signed(60.0),
                ))
                .expect("a finite offset"),
            });
        }
        owner.apply(SURFACE).expect("a random tree applies");
        owner.current().clone()
    }

    /// A version is walked as the tree it flattens to: the same layers,
    /// chunks and placements in the same order, each follower at the same
    /// transform, over random trees of boundaries and effects.
    #[test]
    fn a_version_walks_as_its_flattened_tree() {
        for case in 0..2_000_u64 {
            let mut rng =
                Rng(0x2545_F491_4F6C_DD1D ^ (case + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let count = 1 + rng.below(6);
            let version = random_version(&mut rng, count);
            let tree = version.flatten();
            let (mut walked, mut flattened) = (Trace::default(), Trace::default());
            walk_scene(SceneContent::Version(&version), None, &mut walked);
            walk_scene(SceneContent::Tree(&tree), None, &mut flattened);
            assert_eq!(walked.log, flattened.log, "case {case}");
        }
    }

    /// Where each chunk of a tree lands on the surface, read from the tree
    /// walk alone: the transforms it accumulates, each follower at the
    /// transform it resolves, and the clips above, as bounding boxes. `None`
    /// for a chunk nothing of which is shown.
    #[derive(Default)]
    struct Landing {
        state: Vec<(Matrix4, Option<Rect<f64>>, bool)>,
        chunks: Vec<(*const flui_painting::DisplayList, Option<Rect<f64>>)>,
    }

    impl SceneVisitor for Landing {
        fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step {
            use flui_layer::Layer;
            let (ctm, clip, hidden) =
                self.state
                    .last()
                    .copied()
                    .unwrap_or((Matrix4::IDENTITY, None, false));
            let translated = |offset: flui_foundation::geometry::Offset| {
                ctm * Matrix4::translation(offset.dx, offset.dy, 0.0)
            };
            let mut next = (ctm, clip, hidden);
            let commands = match item {
                Item::Chunk(commands) => Some(*commands),
                Item::Layer(Layer::Picture(picture)) => Some(picture.picture()),
                _ => None,
            };
            if let Some(commands) = commands {
                let landed = commands
                    .bounds()
                    .map(|bounds| ctm.transform_rect(&bounds))
                    .and_then(|bounds| match clip {
                        Some(clip) => bounds.intersect(&clip),
                        None => Some(bounds),
                    })
                    .filter(|_| !hidden);
                self.chunks.push((std::ptr::from_ref(commands), landed));
            } else {
                match item {
                    Item::Placed(offset) => next.0 = translated(offset.offset()),
                    Item::Layer(Layer::Transform(transform)) => {
                        next.0 = ctm * *transform.transform();
                    }
                    Item::Layer(Layer::Follower(_)) => match site.follower_transform() {
                        Some(transform) => next.0 = ctm * transform,
                        None => next.2 = true,
                    },
                    Item::Layer(Layer::ClipRect(rect)) if rect.clips() => {
                        let bounds = ctm.transform_rect(&rect.bounds());
                        next.1 = match clip {
                            Some(clip) => clip.intersect(&bounds),
                            None => Some(bounds),
                        };
                        next.2 |= next.1.is_none();
                    }
                    Item::Layer(layer) => next.0 = translated(layer.local_translation()),
                    Item::Chunk(_) => {}
                }
            }
            self.state.push(next);
            Step::Descend
        }

        fn exit(&mut self, _site: Site<'_>, _item: &Item<'_>) {
            self.state.pop();
        }
    }

    /// The chunks a walk enters.
    #[derive(Default)]
    struct Entered(Vec<*const flui_painting::DisplayList>);

    impl SceneVisitor for Entered {
        fn enter(&mut self, _site: Site<'_>, item: &Item<'_>) -> Step {
            if let Item::Chunk(commands) = item {
                self.0.push(std::ptr::from_ref(*commands));
            }
            Step::Descend
        }

        fn exit(&mut self, _site: Site<'_>, _item: &Item<'_>) {}
    }

    fn meets(a: Rect<f64>, b: Rect<f64>) -> bool {
        a.left() <= b.right()
            && b.left() <= a.right()
            && a.top() <= b.bottom()
            && b.top() <= a.bottom()
    }

    /// A walk culled by a damage rect enters every chunk the flattened tree
    /// lands in that rect, over random trees of boundaries, transforms,
    /// rotations, clips, leaders and followers and random damage; the
    /// landings come from the tree walk, not from the version's geometry.
    #[test]
    fn a_culled_walk_enters_every_chunk_its_damage_meets() {
        let mut skipped = 0_usize;
        for case in 0..2_000_u64 {
            let mut rng =
                Rng(0x9E37_79B9_7F4A_7C15 ^ (case + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9));
            let count = 1 + rng.below(6);
            let version = random_version(&mut rng, count);
            let tree = version.flatten();
            let mut landing = Landing::default();
            walk_scene(SceneContent::Tree(&tree), None, &mut landing);
            let damage = Rect::from_xywh(
                rng.signed(150.0) + 150.0,
                rng.signed(150.0) + 150.0,
                1.0 + rng.signed(40.0).abs(),
                1.0 + rng.signed(40.0).abs(),
            );
            let mut culled = Entered::default();
            walk_scene(SceneContent::Version(&version), Some(damage), &mut culled);
            for &(chunk, landed) in &landing.chunks {
                let entered = culled.0.contains(&chunk);
                if let Some(landed) = landed
                    && meets(landed, damage)
                {
                    assert!(
                        entered,
                        "case {case}: a chunk landing at {landed:?} skipped under {damage:?}"
                    );
                }
                skipped += usize::from(!entered);
            }
        }
        assert!(
            skipped > 0,
            "no walk skipped a chunk, so the cull was never tested"
        );
    }

    /// A blur over a slot draws past the chunk under it: a damage beside the
    /// chunk, inside the blur's reach, enters it; one past that reach does not.
    #[test]
    fn a_culled_walk_enters_a_slot_whose_blur_reaches_the_damage() {
        use flui_layer::{
            BoundaryContent, ImageFilterLayer, Layer, Placement, SceneOp, SceneOwner,
        };
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let child = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut own = BoundaryContent::builder();
        own.chunk(square(3, version, (0.0, 0.0)));
        writer.push(SceneOp::Publish {
            node: child,
            content: std::sync::Arc::new(own.finish()),
        });
        writer.push(SceneOp::Place {
            node: child,
            placement: Placement::try_new(flui_foundation::geometry::Offset::new(100.0, 100.0))
                .expect("a finite offset"),
        });
        let mut content = BoundaryContent::builder();
        content.push_effect(
            Layer::from(ImageFilterLayer::new(
                flui_painting::paint::ImageFilter::blur(4.0),
            )),
            flui_foundation::RenderId::new(2),
            0,
        );
        content.slot(child);
        content.pop_effect();
        writer.push(SceneOp::Publish {
            node: root,
            content: std::sync::Arc::new(content.finish()),
        });
        owner.apply(SURFACE).expect("a blurred slot applies");
        let version = owner.current();
        let walk = |damage: Rect<f64>| {
            let mut entered = Entered::default();
            walk_scene(SceneContent::Version(version), Some(damage), &mut entered);
            entered.0.len()
        };
        // The square spans 100..110; the blur spreads it a few pixels.
        assert_eq!(
            walk(Rect::from_xywh(112.0, 104.0, 2.0, 2.0)),
            1,
            "inside the blur"
        );
        assert_eq!(
            walk(Rect::from_xywh(200.0, 104.0, 2.0, 2.0)),
            0,
            "past the blur"
        );
    }

    /// A partial frame's walk enters the boundaries its damage can reach
    /// and skips the rest: over a row of a hundred boundaries, a damage
    /// rect over one of them enters as many nodes as over ten.
    #[test]
    fn a_partial_walk_enters_only_the_boundaries_its_damage_reaches() {
        use flui_layer::{BoundaryContent, Placement, SceneOp, SceneOwner};
        let entered = |row: usize| {
            let mut owner = SceneOwner::new();
            let mut writer = owner.writer();
            let root = writer.current().root();
            let children: Vec<SceneNodeId> = (0..row).map(|_| writer.mint_node()).collect();
            let mut content = BoundaryContent::builder();
            for &child in &children {
                content.slot(child);
            }
            for (index, &child) in children.iter().enumerate() {
                let mut own = BoundaryContent::builder();
                let version = writer.next_content_version().expect("a version");
                own.chunk(square(index + 2, version, (0.0, 0.0)));
                writer.push(SceneOp::Publish {
                    node: child,
                    content: std::sync::Arc::new(own.finish()),
                });
                writer.push(SceneOp::Place {
                    node: child,
                    placement: Placement::try_new(flui_foundation::geometry::Offset::new(
                        12.0 * index as f64,
                        0.0,
                    ))
                    .expect("a finite offset"),
                });
            }
            writer.push(SceneOp::Publish {
                node: root,
                content: std::sync::Arc::new(content.finish()),
            });
            owner
                .apply(flui_layer::SurfaceKey {
                    size: (2_000, 100),
                    generation: flui_foundation::SurfaceGeneration::ZERO,
                })
                .expect("a row applies");
            let version = owner.current();
            let (mut whole, mut partial) = (Trace::default(), Trace::default());
            walk_scene(SceneContent::Version(version), None, &mut whole);
            // Over the third boundary alone, clear of its neighbours' edges.
            let damage = Rect::from_xywh(25.0, 2.0, 6.0, 6.0);
            walk_scene(SceneContent::Version(version), Some(damage), &mut partial);
            (whole.entered, partial.entered)
        };
        let (ten_whole, ten) = entered(10);
        let (hundred_whole, hundred) = entered(100);
        assert_eq!(
            (ten_whole, hundred_whole),
            (21, 201),
            "a whole walk enters every node and chunk"
        );
        assert_eq!(
            ten, hundred,
            "the damage reaches the same boundaries in both rows"
        );
        assert_eq!(ten, 3, "the root, the damaged boundary and its chunk");
    }
}
