//! A semantics tree assembled by hand, and the handles an agent reads it by.
//!
//! The agent tests (`locate`, `actionability`) share one shape: nodes numbered
//! by the render object that owns them, rectangles in the window's logical
//! pixels at a device ratio of 1, and a published tree to read.

use std::num::NonZeroU32;
use std::sync::Arc;

use flui_foundation::geometry::{DevicePixelRatio, Rect};
use flui_foundation::{RenderId, SemanticsId};
use flui_protocol::{ElementId, ElementRef, WindowId};
use flui_semantics::{
    AccessibilityNodeId, Placement, SemanticsConfiguration, SemanticsNode, SemanticsOwner,
    TreeUpdate,
};

/// The render object that owns fixture node `index`, at generation `generation`.
pub fn render_id(index: u32, generation: u32) -> RenderId {
    RenderId::new_gen(
        index,
        NonZeroU32::new(generation).expect("a fixture generation is non-zero"),
    )
}

/// The window the fixture tree is the root of.
pub fn window() -> WindowId {
    WindowId::from_u64(1).expect("a window handle is non-zero")
}

/// The wire handle of fixture node `index` at the first generation.
pub fn element(index: u32) -> ElementRef {
    element_of(render_id(index, 5))
}

/// The wire handle of the node `render` owns.
pub fn element_of(render: RenderId) -> ElementRef {
    ElementRef::new(
        window(),
        ElementId::from_u64(AccessibilityNodeId::from(render).as_u64())
            .expect("an accessibility id is non-zero"),
    )
}

pub fn placement() -> Placement {
    Placement::new(DevicePixelRatio::new(1.0).expect("1 is a ratio"), window())
}

/// An owner and the arena ids of the nodes put in it.
pub struct Fixture {
    pub owner: SemanticsOwner,
    ids: Vec<(u32, SemanticsId)>,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            owner: SemanticsOwner::new(Arc::new(|_: &TreeUpdate| {})),
            ids: Vec::new(),
        }
    }

    /// Node `index` under node `parent` (the root when `None`), laid out at
    /// `rect` as `(left, top, width, height)`, configured by `configure`.
    pub fn add(
        &mut self,
        parent: Option<u32>,
        index: u32,
        rect: (f64, f64, f64, f64),
        configure: impl FnOnce(&mut SemanticsConfiguration),
    ) -> &mut Self {
        self.add_generation(parent, index, 5, rect, configure)
    }

    /// As [`Self::add`], owned by a render object of `generation`.
    pub fn add_generation(
        &mut self,
        parent: Option<u32>,
        index: u32,
        generation: u32,
        (left, top, width, height): (f64, f64, f64, f64),
        configure: impl FnOnce(&mut SemanticsConfiguration),
    ) -> &mut Self {
        let mut node = SemanticsNode::new().with_source_render_id(render_id(index, generation));
        node.set_rect(Rect::from_ltwh(left, top, width, height));
        configure(node.config_mut());
        let id = self.owner.insert(node);
        match parent {
            Some(parent) => self.owner.add_child(self.id_of(parent), id),
            None => self.owner.set_root(Some(id)),
        }
        self.ids.push((index, id));
        self
    }

    pub fn id_of(&self, index: u32) -> SemanticsId {
        self.ids
            .iter()
            .rev()
            .find(|(candidate, _)| *candidate == index)
            .map(|(_, id)| *id)
            .expect("the fixture has that node")
    }

    /// Publishes what has been added.
    pub fn flush(&mut self) -> &mut Self {
        self.owner.flush();
        self
    }
}
