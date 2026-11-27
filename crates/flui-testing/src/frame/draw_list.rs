//! [`DrawList`]: the draw commands of a presented scene, owned, and the walk that places them.

use flui_foundation::RenderId;
use flui_layer::SceneVersion;
use flui_painting::DrawCommandBuf;

/// Draw commands of a presented scene, in paint order, each placed in the window: what
/// [`Frame::draw_list`](super::Frame::draw_list) and
/// [`Frame::draw_commands`](super::Frame::draw_commands) copy out.
///
/// An owned list a test asserts over in Rust, where `flutter_test`'s `paints` matcher
/// (`mock_canvas.dart:54`) is a pattern language (ADR-0400 §5).
#[derive(Debug, Clone, Default)]
pub struct DrawList {
    commands: Vec<DrawCommandBuf>,
}

impl DrawList {
    /// The commands of `version`'s frame whose painter `selects`; empty when no scene was
    /// presented. The frame is the first slot of the version's root, as the host's sink reads it.
    pub(super) fn of_scene(
        version: Option<&SceneVersion>,
        selects: impl Fn(RenderId) -> bool,
    ) -> Self {
        let commands = version
            .and_then(|version| {
                let frame = version.node(version.root())?.content().slots().next()?;
                Some(scene_commands(version, frame, selects))
            })
            .unwrap_or_default();
        Self { commands }
    }

    /// The commands, in paint order.
    #[must_use]
    pub fn commands(&self) -> &[DrawCommandBuf] {
        &self.commands
    }

    /// How many commands there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether there is none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The commands in paint order.
    pub fn iter(&self) -> std::slice::Iter<'_, DrawCommandBuf> {
        self.commands.iter()
    }
}

impl IntoIterator for DrawList {
    type Item = DrawCommandBuf;
    type IntoIter = std::vec::IntoIter<DrawCommandBuf>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.into_iter()
    }
}

impl<'a> IntoIterator for &'a DrawList {
    type Item = &'a DrawCommandBuf;
    type IntoIter = std::slice::Iter<'a, DrawCommandBuf>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter()
    }
}

/// The draw commands of the chunks under `node` in `version` whose painter
/// `selects`, in paint order, each placed in the space `node` sits in; what
/// [`DrawList::of_scene`] and the deprecated [`Tester::draw_commands`](crate::tester::Tester::draw_commands) read.
///
/// A content's entries are in paint order and a slot paints where it
/// stands, so the walk opens the child's content there and resumes the
/// parent's after it. An explicit stack of open contents, so a deep chain of
/// boundaries cannot overflow the test thread's stack. A follower is placed
/// as the engine places it ([`SceneVersion::follower_transform`]), and one
/// its leader does not place is skipped with everything under it.
///
/// [`SceneVersion::follower_transform`]: flui_layer::SceneVersion::follower_transform
pub(crate) fn scene_commands(
    version: &flui_layer::SceneVersion,
    node: flui_layer::SceneNodeId,
    selects: impl Fn(flui_foundation::RenderId) -> bool,
) -> Vec<flui_painting::DrawCommandBuf> {
    use flui_foundation::geometry::Matrix4;
    use flui_layer::{BoundaryContent, Entry, EntryAddress, EntryIndex, Layer, SceneNodeId};

    /// A content the walk is inside, and the space each of its entries so
    /// far paints its children in: `None` under a follower nothing places.
    struct Open<'a> {
        node: SceneNodeId,
        content: &'a BoundaryContent,
        effects: Vec<(EntryIndex, &'a Layer)>,
        next_effect: usize,
        origin: Matrix4,
        spaces: Vec<Option<Matrix4>>,
    }
    let open = |node: SceneNodeId, above: Matrix4| {
        let held = version.node(node)?;
        let offset = held.placement().offset();
        let content: &BoundaryContent = held.content();
        Some(Open {
            node,
            content,
            effects: content.effect_layers().collect(),
            next_effect: 0,
            origin: above * Matrix4::translation(offset.dx, offset.dy, 0.0),
            spaces: Vec::with_capacity(content.entries().len()),
        })
    };

    let mut commands = Vec::new();
    let mut stack: Vec<Open<'_>> = open(node, Matrix4::IDENTITY).into_iter().collect();
    while let Some(top) = stack.last_mut() {
        let Some(entry) = top.content.entries().get(top.spaces.len()) else {
            stack.pop();
            continue;
        };
        let under = match entry.parent() {
            None => Some(top.origin),
            Some(effect) => top.spaces[effect.get()],
        };
        let child = match entry {
            Entry::Effect { .. } => {
                let (index, layer) = *top
                    .effects
                    .get(top.next_effect)
                    .expect("BUG: each effect entry has a layer, in paint order");
                top.next_effect += 1;
                let local = match layer {
                    Layer::Transform(layer) => Some(*layer.transform()),
                    Layer::Follower(_) => version.follower_transform(EntryAddress {
                        node: top.node,
                        entry: index,
                    }),
                    layer => {
                        let offset = layer.local_translation();
                        Some(Matrix4::translation(offset.dx, offset.dy, 0.0))
                    }
                };
                top.spaces
                    .push(under.zip(local).map(|(under, local)| under * local));
                None
            }
            Entry::Chunk { chunk, .. } => {
                top.spaces.push(under);
                // A chunk is one render object's run (ADR-0304 §2).
                if let Some(under) = under
                    && selects(chunk.owner())
                {
                    commands.extend(chunk.commands().iter().map(|command| {
                        let mut command = command.to_buf();
                        command.transform = under * command.transform;
                        command
                    }));
                }
                None
            }
            Entry::Slot { node: child, .. } => {
                top.spaces.push(under);
                under.map(|under| (*child, under))
            }
        };
        if let Some((child, under)) = child
            && let Some(child) = open(child, under)
        {
            stack.push(child);
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
    use flui_layer::{
        FollowerLayer, Layer, LayerLink, LayerNode, LayerTree, LeaderLayer, OffsetLayer,
        PictureLayer, SceneVersion,
    };
    use flui_painting::{Canvas, DrawOp, paint::Paint, styling::Color};

    /// A one-pixel swatch of `shade`, painted by a render object of that slot.
    fn swatch(shade: u8) -> LayerNode {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_origin_size(Point::ZERO, Size::new(1.0, 1.0)),
            &Paint::fill(Color::rgb(shade, 0, 0)),
        );
        LayerNode::new(Layer::Picture(PictureLayer::new(canvas.finish()))).with_render(
            flui_foundation::RenderId::new_gen(
                u32::from(shade),
                flui_foundation::generation::Generation::FIRST,
            ),
        )
    }

    fn placed(tree: &LayerTree) -> Vec<(u8, Matrix4)> {
        let version = SceneVersion::from_tree(tree);
        super::scene_commands(&version, version.root(), |_| true)
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Rect { paint, .. } => {
                    let shade = (paint.color.to_rgba_f32_array()[0] * 255.0).round();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a channel the test wrote as a u8"
                    )]
                    let shade = shade as u8;
                    Some((shade, command.transform))
                }
                _ => None,
            })
            .collect()
    }

    /// A follower is placed by its leader, and one with no leader is left
    /// out with its subtree while the drawing after it is still read.
    ///
    /// Fails if the walk places a follower by its own translation (zero),
    /// takes its resolved transform as the window's or applies the
    /// follower's ancestors twice, reads an unplaced follower's subtree, or
    /// stops at it.
    #[test]
    fn followers_are_placed_by_their_leader_or_left_out() {
        let led = LayerLink::new();
        let unled = LayerLink::new();
        let mut tree = LayerTree::new(Layer::Offset(OffsetLayer::zero()));
        let root = tree.root();
        tree.push_child(
            root,
            Layer::Leader(LeaderLayer::with_offset(
                led,
                Size::new(5.0, 5.0),
                Offset::new(30.0, 40.0),
            )),
        );
        // Under a slot of its own, as an overlay boundary is: the resolved
        // transform is relative to that slot, which the walk applies above it.
        let slot = tree.push_child(
            root,
            Layer::Offset(OffsetLayer::new(Offset::new(100.0, 0.0))),
        );
        let follower = tree.push_child(slot, Layer::Follower(FollowerLayer::new(led)));
        tree.push_child(follower, swatch(1));
        let hidden = tree.push_child(
            root,
            Layer::Follower(FollowerLayer::new(unled).with_show_when_unlinked(false)),
        );
        tree.push_child(hidden, swatch(2));
        tree.push_child(root, swatch(3));

        assert_eq!(
            placed(&tree),
            [
                (1, Matrix4::translation(30.0, 40.0, 0.0)),
                (3, Matrix4::IDENTITY),
            ]
        );
    }
}
