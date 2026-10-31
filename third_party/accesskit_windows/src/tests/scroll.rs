// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use std::sync::Arc;

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Point,
    Rect, Role, ScrollUnit, TreeId, TreeInfo, TreeUpdate,
};
use parking_lot::Mutex;
use windows::{
    Win32::{System::Variant::VARIANT, UI::Accessibility::*},
    core::*,
};

const WINDOW_TITLE: &str = "Scroll test";

const WINDOW_ID: NodeId = NodeId(0);
const LIST_ID: NodeId = NodeId(1);
const LABEL_ID: NodeId = NodeId(2);

/// A list 100 high that scrolls 300 vertically, scrolled to 75, and a label
/// that does not scroll.
fn get_initial_state() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_children(vec![LIST_ID, LABEL_ID]);
    let mut list = Node::new(Role::List);
    list.set_author_id("list");
    list.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    list.set_scroll_y(75.0);
    list.set_scroll_y_min(0.0);
    list.set_scroll_y_max(300.0);
    for action in [
        Action::ScrollUp,
        Action::ScrollDown,
        Action::SetScrollOffset,
    ] {
        list.add_action(action);
    }
    let mut label = Node::new(Role::Label);
    label.set_author_id("label");
    label.set_value("Not a scroller");
    TreeUpdate {
        nodes: vec![(WINDOW_ID, root), (LIST_ID, list), (LABEL_ID, label)],
        tree: Some(TreeInfo::new(WINDOW_ID)),
        tree_id: TreeId::ROOT,
        focus: WINDOW_ID,
    }
}

struct RecordingActionHandler(Arc<Mutex<Vec<ActionRequest>>>);

impl ActionHandler for RecordingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.lock().push(request);
    }
}

struct ScrollActivationHandler;

impl ActivationHandler for ScrollActivationHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(get_initial_state())
    }
}

fn find(s: &super::Scope, automation_id: &str) -> Result<IUIAutomationElement> {
    let root = unsafe { s.uia.ElementFromHandle(s.window.0) }?;
    let condition = unsafe {
        s.uia.CreatePropertyCondition(
            UIA_AutomationIdPropertyId,
            &VARIANT::from(BSTR::from(automation_id)),
        )
    }?;
    unsafe { root.FindFirst(TreeScope_Descendants, &condition) }
}

#[test]
fn scroll_pattern() -> Result<()> {
    let requests = Arc::new(Mutex::new(Vec::new()));
    super::scope(
        WINDOW_TITLE,
        ScrollActivationHandler,
        RecordingActionHandler(Arc::clone(&requests)),
        |s| {
            let label = find(s, "label")?;
            assert!(
                unsafe { label.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId) }
                    .is_err(),
                "a node without scroll extents has no scroll pattern"
            );

            let list = find(s, "list")?;
            let scroll: IUIAutomationScrollPattern =
                unsafe { list.GetCurrentPatternAs(UIA_ScrollPatternId) }?;
            assert!(bool::from(unsafe { scroll.CurrentVerticallyScrollable() }?));
            assert!(!bool::from(unsafe { scroll.CurrentHorizontallyScrollable() }?));
            assert_eq!(unsafe { scroll.CurrentVerticalScrollPercent() }?, 25.0);
            assert_eq!(
                unsafe { scroll.CurrentHorizontalScrollPercent() }?,
                UIA_ScrollPatternNoScroll
            );
            assert_eq!(unsafe { scroll.CurrentVerticalViewSize() }?, 25.0);
            assert_eq!(unsafe { scroll.CurrentHorizontalViewSize() }?, 100.0);

            unsafe { scroll.SetScrollPercent(UIA_ScrollPatternNoScroll, 50.0) }?;
            unsafe { scroll.Scroll(ScrollAmount_NoAmount, ScrollAmount_LargeIncrement) }?;
            unsafe { scroll.Scroll(ScrollAmount_NoAmount, ScrollAmount_SmallDecrement) }?;
            let recorded: Vec<_> = requests
                .lock()
                .drain(..)
                .map(|request| (request.target_node, request.action, request.data))
                .collect();
            assert_eq!(
                recorded,
                [
                    (
                        LIST_ID,
                        Action::SetScrollOffset,
                        Some(ActionData::SetScrollOffset(Point::new(0.0, 150.0)))
                    ),
                    (
                        LIST_ID,
                        Action::ScrollDown,
                        Some(ActionData::ScrollUnit(ScrollUnit::Page))
                    ),
                    (
                        LIST_ID,
                        Action::ScrollUp,
                        Some(ActionData::ScrollUnit(ScrollUnit::Item))
                    ),
                ]
            );

            assert!(
                unsafe { scroll.Scroll(ScrollAmount_LargeIncrement, ScrollAmount_NoAmount) }
                    .is_err(),
                "an axis that does not scroll refuses to"
            );
            assert!(
                unsafe { scroll.SetScrollPercent(UIA_ScrollPatternNoScroll, 101.0) }.is_err(),
                "a percentage past the end is refused"
            );
            assert!(requests.lock().is_empty(), "and nothing is asked");
            Ok(())
        },
    )
}
