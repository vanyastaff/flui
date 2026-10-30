// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{ActionHandler, ActionRequest, NodeId, TreeId};
use accesskit_consumer::{FullNodeId, Tree};
use hashbrown::HashMap;
use objc2::rc::{Retained, WeakId};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;
use std::fmt::Debug;
use std::{cell::RefCell, rc::Rc};

use crate::node::PlatformNode;

/// Told when the assistive technology's own cursor (VoiceOver's) moves to a
/// node of the tree or leaves it. This is not the tree's focus, which the
/// application owns and publishes.
pub trait PlatformFocusHandler {
    /// The cursor moved to `node`, or left the tree for `None`.
    fn platform_focus_changed(&mut self, node: Option<(NodeId, TreeId)>);
}

pub(crate) trait ActionHandlerNoMut {
    fn do_action(&self, request: ActionRequest);

    /// Tell the platform focus handler, if there is one.
    fn platform_focus_changed(&self, _node: Option<(NodeId, TreeId)>) {}
}

/// The platform focus handler, with the moves made while it runs: those are
/// told in order once it returns, so a re-entrant move is neither lost nor
/// told inside the handler.
struct PlatformFocusCell {
    handler: RefCell<Box<dyn PlatformFocusHandler>>,
    pending: RefCell<Vec<Option<(NodeId, TreeId)>>>,
}

pub(crate) struct ActionHandlerWrapper<H: ActionHandler>(RefCell<H>, Option<PlatformFocusCell>);

impl<H: 'static + ActionHandler> ActionHandlerWrapper<H> {
    pub(crate) fn new(inner: H) -> Self {
        Self(RefCell::new(inner), None)
    }

    pub(crate) fn with_platform_focus_handler(
        inner: H,
        handler: impl 'static + PlatformFocusHandler,
    ) -> Self {
        Self(
            RefCell::new(inner),
            Some(PlatformFocusCell {
                handler: RefCell::new(Box::new(handler)),
                pending: RefCell::new(Vec::new()),
            }),
        )
    }
}

impl<H: ActionHandler> ActionHandlerNoMut for ActionHandlerWrapper<H> {
    fn do_action(&self, request: ActionRequest) {
        self.0.borrow_mut().do_action(request)
    }

    fn platform_focus_changed(&self, node: Option<(NodeId, TreeId)>) {
        let Some(cell) = &self.1 else {
            return;
        };
        cell.pending.borrow_mut().push(node);
        let Ok(mut handler) = cell.handler.try_borrow_mut() else {
            return;
        };
        loop {
            let next = {
                let mut pending = cell.pending.borrow_mut();
                if pending.is_empty() {
                    break;
                }
                pending.remove(0)
            };
            handler.platform_focus_changed(next);
        }
    }
}

pub(crate) struct Context {
    pub(crate) view: WeakId<UIView>,
    pub(crate) tree: RefCell<Tree>,
    pub(crate) action_handler: Rc<dyn ActionHandlerNoMut>,
    platform_nodes: RefCell<HashMap<FullNodeId, Retained<PlatformNode>>>,
    pub(crate) platform_focus: RefCell<Option<FullNodeId>>,
    pub(crate) mtm: MainThreadMarker,
}

impl Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("view", &self.view)
            .field("tree", &self.tree)
            .field("action_handler", &"ActionHandler")
            .field("platform_nodes", &self.platform_nodes)
            .field("platform_focus", &self.platform_focus)
            .field("mtm", &self.mtm)
            .finish()
    }
}

impl Context {
    pub(crate) fn new(
        view: WeakId<UIView>,
        tree: Tree,
        action_handler: Rc<dyn ActionHandlerNoMut>,
        mtm: MainThreadMarker,
    ) -> Rc<Self> {
        Rc::new(Self {
            view,
            tree: RefCell::new(tree),
            action_handler,
            platform_nodes: RefCell::new(HashMap::new()),
            platform_focus: RefCell::new(None),
            mtm,
        })
    }

    pub(crate) fn get_or_create_platform_node(
        self: &Rc<Self>,
        id: FullNodeId,
    ) -> Option<Retained<PlatformNode>> {
        if let Some(result) = self.platform_nodes.borrow().get(&id) {
            return Some(result.clone());
        }

        let result = PlatformNode::new(self, id)?;
        self.platform_nodes.borrow_mut().insert(id, result.clone());
        Some(result)
    }

    pub(crate) fn remove_platform_node(&self, id: FullNodeId) -> Option<Retained<PlatformNode>> {
        let mut platform_nodes = self.platform_nodes.borrow_mut();
        platform_nodes.remove(&id)
    }

    pub(crate) fn do_action(&self, request: ActionRequest) {
        self.action_handler.do_action(request);
    }
}
