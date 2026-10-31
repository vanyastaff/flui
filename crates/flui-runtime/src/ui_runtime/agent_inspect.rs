//! An agent's reads of a window's element, render and layer trees (ADR-0238 §6, ADR-0242), through
//! the owner inbox.
//!
//! The owner takes the typed snapshot the trees describe themselves with
//! (`flui_foundation::inspect`) and turns it into the protocol's owned [`InspectNode`] tree. Ids
//! on the wire are the trees' own handles (`v3.2`); a root named as a [`NodeRef`] (`p3/v3.2`) is
//! accepted too, and a reference to another or a closed window is
//! [`AgentError::UnknownPresentation`], never the node of that name in this window.
//!
//! The element and render trees are read at the owner's turn. The layer tree belongs to the scene
//! a frame paints, which the sink takes, so the window keeps no copy of it: a read of the layers
//! is parked on the presentation, one repaint is asked for, and the next scene the window shows is
//! snapshot before it goes to the sink. A frame that shows nothing costs nothing here beyond a
//! check that no read is parked.

use std::num::NonZeroU32;

use flui_foundation::PresentationId;
use flui_foundation::inspect::{
    InspectId, Level, Link, NodeRef, OwnedField, OwnedValue, SnapshotError, SnapshotNode,
    SnapshotQuery, Unit, snapshot,
};
use flui_layer::LayerTree;
use flui_protocol::{
    InspectField, InspectLevel, InspectNode, InspectParams, InspectReading, InspectUnit,
    InspectValue, LogicalRect,
};
use flui_view::dev_agent::InspectTarget;

use super::UiRuntime;
use super::agent::{AgentError, ReplySender, send_reply};
use crate::presentation::PresentationState;

/// The most nodes one inspection reports when the caller names no limit, and at most.
const MAX_NODES: u32 = 4096;

/// The most layer reads one presentation parks for its next scene. A read past it is answered
/// [`AgentError::TreeBusy`], never dropped or put in place of an older one.
pub(crate) const MAX_PARKED_LAYER_READS: usize = 16;

/// A read of the layer tree, parked until the window shows its next scene. Owner thread only.
pub(crate) struct PendingLayerRead {
    query: SnapshotQuery,
    reply: ReplySender<InspectReading>,
}

impl PendingLayerRead {
    pub(crate) fn abandon(self) {
        send_reply(&self.reply, None, Err(AgentError::PresentationGone));
    }
}

impl UiRuntime {
    /// The owner half of [`SemanticsAgent::inspect`](super::SemanticsAgent::inspect) for the
    /// element and render trees.
    pub(super) fn serve_agent_inspect(
        &self,
        presentation: PresentationId,
        target: InspectTarget,
        params: &InspectParams,
    ) -> Result<InspectReading, AgentError> {
        let state = self
            .presentations
            .get(presentation)
            .ok_or(AgentError::PresentationGone)?;
        let query = query_of(params, presentation, target)?;
        let taken = match target {
            InspectTarget::Widgets => state.widgets().inspect_elements(query),
            InspectTarget::Render => state.pipeline().inspect_render(query),
            _ => {
                return Err(AgentError::InvalidArgument {
                    reason: "this tree is not read at the owner's turn",
                });
            }
        };
        reading_from(taken)
    }

    /// The owner half of a read of the layer tree: parks it for the window's next scene and asks
    /// for a repaint, so a window with nothing dirty shows one. Answers at once what cannot wait:
    /// a root that is not a layer handle, a reference to another window, a full parking list.
    /// Returns whether the presentation was still open.
    pub(super) fn park_agent_layer_read(
        &self,
        presentation: PresentationId,
        params: &InspectParams,
        reply: ReplySender<InspectReading>,
    ) -> bool {
        let Some(state) = self.presentations.get(presentation) else {
            send_reply(&reply, None, Err(AgentError::PresentationGone));
            return false;
        };
        let query = match query_of(params, presentation, InspectTarget::Layers) {
            Ok(query) => query,
            Err(error) => {
                send_reply(&reply, None, Err(error));
                return true;
            }
        };
        {
            let mut parked = state.agent_layer_reads.borrow_mut();
            if parked.len() >= MAX_PARKED_LAYER_READS {
                drop(parked);
                send_reply(
                    &reply,
                    None,
                    Err(AgentError::TreeBusy(
                        "the window parks as many layer reads as it admits",
                    )),
                );
                return true;
            }
            parked.push(PendingLayerRead { query, reply });
        }
        state.pipeline().with_mut(|pipeline| {
            if let Some(root) = pipeline.root_id() {
                pipeline.mark_needs_paint(root);
            }
        });
        self.request_redraw_for(state);
        true
    }
}

impl PresentationState {
    /// Answers the layer reads parked for the scene the window is about to show. Called with the
    /// scene's tree before the sink takes it.
    pub(crate) fn answer_agent_layer_reads(&self, scene: &LayerTree) {
        let parked = std::mem::take(&mut *self.agent_layer_reads.borrow_mut());
        for read in parked {
            let taken = snapshot(scene, read.query);
            send_reply(&read.reply, None, reading_from(taken));
        }
    }

    pub(crate) fn abandon_agent_layer_reads(&self) {
        for read in std::mem::take(&mut *self.agent_layer_reads.borrow_mut()) {
            read.abandon();
        }
    }
}

/// What `params` asks for, as the query the trees answer.
fn query_of(
    params: &InspectParams,
    presentation: PresentationId,
    target: InspectTarget,
) -> Result<SnapshotQuery, AgentError> {
    Ok(SnapshotQuery {
        root: params
            .root
            .as_deref()
            .map(|text| root_of(text, presentation, target))
            .transpose()?,
        max_depth: params.max_depth,
        max_nodes: Some(
            NonZeroU32::try_from(
                params
                    .max_nodes
                    .and_then(NonZeroU32::new)
                    .map_or(MAX_NODES, |nodes| nodes.get().min(MAX_NODES)),
            )
            .expect("BUG: the node limit is clamped to a non-zero count"),
        ),
        level: level_of(params.level),
    })
}

fn reading_from(taken: Result<SnapshotNode, SnapshotError>) -> Result<InspectReading, AgentError> {
    match taken {
        Ok(node) => Ok(reading_of(node)),
        Err(SnapshotError::Empty) => Ok(InspectReading::default()),
        Err(SnapshotError::Gone(node)) => Err(AgentError::NodeGone {
            node: node.to_string(),
        }),
        Err(SnapshotError::Busy(why)) => Err(AgentError::TreeBusy(why)),
        Err(_) => Err(AgentError::Malformed),
    }
}

/// The node a caller names as the root of an inspection of `target`: a tree-local handle, or a
/// reference that must name this window.
fn root_of(
    text: &str,
    presentation: PresentationId,
    target: InspectTarget,
) -> Result<InspectId, AgentError> {
    const NOT_A_NODE: AgentError = AgentError::InvalidArgument {
        reason: "root is not a node handle",
    };
    let link = if text.contains('/') {
        let reference = text.parse::<NodeRef>().map_err(|_| NOT_A_NODE)?;
        if reference.presentation != presentation {
            return Err(AgentError::UnknownPresentation);
        }
        reference.link
    } else {
        text.parse::<Link>().map_err(|_| NOT_A_NODE)?
    };
    let of_target = matches!(
        (target, link),
        (InspectTarget::Widgets, Link::Element(_))
            | (InspectTarget::Render, Link::Render(_))
            | (InspectTarget::Layers, Link::Layer(_))
    );
    if !of_target {
        return Err(AgentError::InvalidArgument {
            reason: "root names a node of another tree",
        });
    }
    Ok(InspectId::from(link))
}

fn level_of(level: InspectLevel) -> Level {
    match level {
        InspectLevel::Detail => Level::Detail,
        InspectLevel::Debug => Level::Debug,
        _ => Level::Summary,
    }
}

fn level_to_wire(level: Level) -> InspectLevel {
    match level {
        Level::Detail => InspectLevel::Detail,
        Level::Debug => InspectLevel::Debug,
        Level::Summary => InspectLevel::Summary,
    }
}

fn unit_to_wire(unit: Unit) -> Option<InspectUnit> {
    Some(match unit {
        Unit::LogicalPx => InspectUnit::LogicalPx,
        Unit::DevicePx => InspectUnit::DevicePx,
        Unit::Seconds => InspectUnit::Seconds,
        Unit::Degrees => InspectUnit::Degrees,
        Unit::Radians => InspectUnit::Radians,
        Unit::Fraction => InspectUnit::Fraction,
        _ => return None,
    })
}

fn value_to_wire(value: &OwnedValue) -> InspectValue {
    match value {
        OwnedValue::Bool(value) => InspectValue::Bool(*value),
        OwnedValue::Int(value) => InspectValue::Int(*value),
        OwnedValue::Uint(value) => InspectValue::Uint(*value),
        OwnedValue::Float(value) => InspectValue::Float(*value),
        OwnedValue::Str(text) => InspectValue::Str(text.clone()),
        OwnedValue::Variant { ty, name } => InspectValue::Variant {
            ty: (*ty).to_owned(),
            name: (*name).to_owned(),
        },
        OwnedValue::Size(size) => InspectValue::Size {
            width: size.width,
            height: size.height,
        },
        OwnedValue::Offset(offset) => InspectValue::Offset {
            dx: offset.dx,
            dy: offset.dy,
        },
        OwnedValue::Rect(rect) => InspectValue::Rect(rect_to_wire(*rect)),
        OwnedValue::Color(color) => InspectValue::Color {
            red: color.red,
            green: color.green,
            blue: color.blue,
            alpha: color.alpha,
        },
        OwnedValue::EdgeInsets(insets) => InspectValue::EdgeInsets {
            top: insets.top,
            right: insets.right,
            bottom: insets.bottom,
            left: insets.left,
        },
        OwnedValue::Matrix(matrix) => InspectValue::Matrix(matrix.m),
        OwnedValue::Duration(duration) => InspectValue::Duration {
            nanos: u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX),
        },
        OwnedValue::Link(link) => InspectValue::Link(link.to_string()),
        // An object nodes share has no wire form of its own: it is named by type and number.
        OwnedValue::Shared { ty, number } => InspectValue::Str(format!("{ty}#{number}")),
        OwnedValue::Debug(text) => InspectValue::Debug(text.clone()),
        OwnedValue::Seq(items) => {
            InspectValue::Seq(items.iter().map(value_to_wire).collect::<Vec<_>>())
        }
        // `None` and a value added later: nothing to say.
        _ => InspectValue::None,
    }
}

fn rect_to_wire(rect: flui_foundation::geometry::Rect) -> LogicalRect {
    LogicalRect::new(
        rect.min.x,
        rect.min.y,
        rect.max.x - rect.min.x,
        rect.max.y - rect.min.y,
    )
}

fn field_to_wire(field: &OwnedField) -> InspectField {
    let mut wire = InspectField::new(field.name, value_to_wire(&field.value));
    wire.level = level_to_wire(field.level);
    wire.unit = field.unit.and_then(unit_to_wire);
    wire.default = field.default.as_ref().map(value_to_wire);
    wire
}

/// The node itself, with no children.
fn node_to_wire(node: &SnapshotNode) -> InspectNode {
    let mut wire = InspectNode::new(node.id.to_string(), node.kind);
    wire.kind_args = node
        .kind_args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<_>>();
    wire.fields = node
        .facts
        .iter()
        .chain(&node.fields)
        .map(field_to_wire)
        .collect::<Vec<_>>();
    wire.rect = node.rect.map(rect_to_wire);
    wire.links = node
        .links
        .iter()
        .map(|link| link.to_string())
        .collect::<Vec<_>>();
    wire.omitted_children = node.omitted_children;
    wire.debug_fields = node.debug_fields;
    wire
}

/// The snapshot as the protocol's tree. The walk keeps its own stack, as the snapshot's does, so
/// a deep chain costs no Rust stack.
fn reading_of(mut root: SnapshotNode) -> InspectReading {
    let mut count = 0_u32;
    let mut truncated = false;
    // A snapshot node frees its own children, so they are taken out rather than moved out.
    let mut convert = |node: &mut SnapshotNode| {
        count = count.saturating_add(1);
        truncated |= node.omitted_children > 0;
        (
            node_to_wire(node),
            std::mem::take(&mut node.children).into_iter(),
        )
    };
    let mut open = Vec::new();
    let mut current = convert(&mut root);
    let done = loop {
        if let Some(mut child) = current.1.next() {
            let parent = std::mem::replace(&mut current, convert(&mut child));
            open.push(parent);
            continue;
        }
        match open.pop() {
            Some(mut parent) => {
                parent.0.children.push(current.0);
                current = parent;
            }
            None => break current.0,
        }
    };
    let mut reading = InspectReading::default();
    reading.roots.push(done);
    reading.count = count;
    reading.truncated = truncated;
    reading
}
