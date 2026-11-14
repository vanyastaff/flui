//! Whatever a list paints inside its viewport stays within a screen
//! reader's reach, however the content is nested, merged, moved or clipped:
//! AccessKit's filter, reading the `clips_children` flags assembly publishes,
//! never leaves out a node whose content is painted in view.
//!
//! The rows are generated. Every width is the viewport's, so where content is
//! painted is a matter of vertical intervals the generator works out itself,
//! from layout, translations and clips, without reading the semantics tree it
//! checks.

use crate::common::{LaidOut, lay_out, loose};
use flui_view::{BoxedView, ViewExt as _};
use flui_widgets::{ClipRect, Column, ListView, MergeSemantics, Semantics, SizedBox, Transform};
use proptest::prelude::*;
use std::rc::Rc;

const VIEWPORT: f64 = 100.0;

/// A piece of generated content.
#[derive(Debug, Clone)]
enum Content {
    /// A labelled box: a node of its own, or, without `container`, merged
    /// into the nearest node above it.
    Leaf { height: f64, container: bool },
    /// A box holding a column: a labelled container, an unlabelled one (a
    /// generic container AccessKit looks through), or no node at all.
    Group {
        node: GroupNode,
        children: Vec<Content>,
    },
    /// Its content painted `dy` from where it is laid out. `moved_dy` is
    /// where the second frame paints it.
    Translate {
        dy: f64,
        moved_dy: f64,
        child: Box<Content>,
    },
    /// Its content clipped to its own box.
    Clip(Box<Content>),
    /// Its content merged into one node.
    Merge(Box<Content>),
}

#[derive(Debug, Clone, Copy)]
enum GroupNode {
    Labelled,
    Unlabelled,
    None,
}

impl Content {
    /// How many labels the content declares.
    fn labels(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Group { node, children } => {
                usize::from(matches!(node, GroupNode::Labelled))
                    + children.iter().map(Self::labels).sum::<usize>()
            }
            Self::Translate { child, .. } | Self::Clip(child) | Self::Merge(child) => {
                child.labels()
            }
        }
    }

    fn height(&self) -> f64 {
        match self {
            Self::Leaf { height, .. } => *height,
            Self::Group { children, .. } => children.iter().map(Self::height).sum(),
            Self::Translate { child, .. } | Self::Clip(child) | Self::Merge(child) => {
                child.height()
            }
        }
    }

    /// The view, with the translations of the first frame or, `moved`, the
    /// second; leaves are numbered in preorder from `next`.
    fn view(&self, moved: bool, next: &mut usize) -> BoxedView {
        match self {
            Self::Leaf { height, container } => {
                let label = token(*next);
                *next += 1;
                Semantics::new()
                    .container(*container)
                    .label(label)
                    .child(SizedBox::new(VIEWPORT, *height))
                    .boxed()
            }
            Self::Group { node, children } => {
                let column = SizedBox::new(VIEWPORT, self.height()).child(Column::new(
                    children
                        .iter()
                        .map(|child| child.view(moved, next))
                        .collect::<Vec<BoxedView>>(),
                ));
                match node {
                    GroupNode::Labelled => {
                        let label = token(*next);
                        *next += 1;
                        Semantics::new()
                            .container(true)
                            .label(label)
                            .child(column)
                            .boxed()
                    }
                    GroupNode::Unlabelled => Semantics::new().container(true).child(column).boxed(),
                    GroupNode::None => column.boxed(),
                }
            }
            Self::Translate {
                dy,
                moved_dy,
                child,
            } => Transform::translate(0.0, if moved { *moved_dy } else { *dy })
                .child(child.view(moved, next))
                .boxed(),
            Self::Clip(child) => ClipRect::new().child(child.view(moved, next)).boxed(),
            Self::Merge(child) => MergeSemantics::new().child(child.view(moved, next)).boxed(),
        }
    }

    /// The labels whose content is painted with an area inside `clip`, for
    /// content laid out at `top` and painted `shift` from there; leaves are
    /// numbered as [`view`](Self::view) numbers them.
    fn painted(
        &self,
        moved: bool,
        top: f64,
        shift: f64,
        clip: (f64, f64),
        next: &mut usize,
        out: &mut Vec<String>,
    ) {
        match self {
            Self::Leaf { height, .. } => {
                let start = (top + shift).max(clip.0);
                let end = (top + shift + height).min(clip.1);
                if end > start {
                    out.push(token(*next));
                }
                *next += 1;
            }
            Self::Group { node, children } => {
                let mut at = top;
                for child in children {
                    child.painted(moved, at, shift, clip, next, out);
                    at += child.height();
                }
                if let GroupNode::Labelled = node {
                    // The group's own label is painted where the group is.
                    let start = (top + shift).max(clip.0);
                    let end = (top + shift + self.height()).min(clip.1);
                    if end > start {
                        out.push(token(*next));
                    }
                    *next += 1;
                }
            }
            Self::Translate {
                dy,
                moved_dy,
                child,
            } => {
                let dy = if moved { *moved_dy } else { *dy };
                child.painted(moved, top, shift + dy, clip, next, out);
            }
            Self::Clip(child) => {
                let clip = (
                    clip.0.max(top + shift),
                    clip.1.min(top + shift + child.height()),
                );
                child.painted(moved, top, shift, clip, next, out);
            }
            Self::Merge(child) => child.painted(moved, top, shift, clip, next, out),
        }
    }
}

/// A label no other label contains.
fn token(index: usize) -> String {
    format!("<{index}>")
}

fn content() -> impl Strategy<Value = Content> {
    let leaf = (
        prop_oneof![Just(10.0), Just(20.0), Just(50.0)],
        any::<bool>(),
    )
        .prop_map(|(height, container)| Content::Leaf { height, container });
    leaf.prop_recursive(4, 24, 3, |inner| {
        let offset = prop_oneof![
            Just(0.0),
            Just(-130.0),
            Just(-60.0),
            Just(60.0),
            Just(130.0),
        ];
        prop_oneof![
            (
                prop_oneof![
                    Just(GroupNode::Labelled),
                    Just(GroupNode::Unlabelled),
                    Just(GroupNode::None),
                ],
                prop::collection::vec(inner.clone(), 1..4),
            )
                .prop_map(|(node, children)| Content::Group { node, children }),
            (offset.clone(), offset, inner.clone()).prop_map(|(dy, moved_dy, child)| {
                Content::Translate {
                    dy,
                    moved_dy,
                    child: Box::new(child),
                }
            }),
            inner
                .clone()
                .prop_map(|child| Content::Clip(Box::new(child))),
            inner.prop_map(|child| Content::Merge(Box::new(child))),
        ]
    })
}

fn screen(rows: &[Content], moved: bool) -> BoxedView {
    let mut first = 0;
    let rows: Rc<Vec<(Content, usize)>> = Rc::new(
        rows.iter()
            .map(|row| {
                let start = first;
                first += row.labels();
                (row.clone(), start)
            })
            .collect(),
    );
    let count = rows.len();
    SizedBox::new(VIEWPORT, VIEWPORT)
        .child(ListView::builder(count, 50.0, move |index| {
            rows.get(index).map(|(row, first)| {
                let mut next = *first;
                row.view(moved, &mut next)
            })
        }))
        .boxed()
}

fn painted_in_view(rows: &[Content], moved: bool) -> Vec<String> {
    let mut next = 0;
    let mut out = Vec::new();
    let mut top = 0.0;
    for row in rows {
        row.painted(moved, top, 0.0, (0.0, VIEWPORT), &mut next, &mut out);
        top += row.height();
    }
    out
}

/// The labels of the nodes AccessKit's filter keeps, reading the published
/// `clips_children` flags or, `clipping` false, with every flag cleared.
fn reached(laid: &LaidOut, clipping: bool) -> Vec<String> {
    use accesskit_consumer::{FilterResult, common_filter};

    let mut update = laid.a11y_tree().expect("semantics enabled").raw().clone();
    if !clipping {
        for (_, node) in &mut update.nodes {
            node.clear_clips_children();
        }
    }
    let tree = accesskit_consumer::Tree::new(update, true);
    let mut labels = Vec::new();
    let mut pending = vec![tree.state().root()];
    while let Some(node) = pending.pop() {
        if common_filter(&node) == FilterResult::ExcludeSubtree {
            continue;
        }
        labels.extend(node.label());
        pending.extend(node.children());
    }
    labels
}

/// Every label painted in view that the filter reaches without the flags
/// (hidden content aside) it also reaches with them.
fn check(laid: &LaidOut, rows: &[Content], moved: bool) -> Result<(), TestCaseError> {
    let unclipped = reached(laid, false);
    let reached = reached(laid, true);
    for label in painted_in_view(rows, moved) {
        if !unclipped.iter().any(|node| node.contains(&label)) {
            continue;
        }
        prop_assert!(
            reached.iter().any(|node| node.contains(&label)),
            "{label} is painted in view but out of a reader's reach; reached {reached:?}"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Built, and then moved: the move re-places what it can and assembles
    /// the rest, and the debug check holds each re-placed tree to a rebuild.
    #[test]
    fn content_painted_in_view_stays_in_a_readers_reach(
        rows in prop::collection::vec(content(), 1..6),
    ) {
        let mut laid = lay_out(screen(&rows, false), loose(400.0));
        laid.enable_semantics();
        laid.tick();
        check(&laid, &rows, false)?;
        laid.pump_widget(screen(&rows, true));
        laid.tick();
        check(&laid, &rows, true)?;
    }
}
