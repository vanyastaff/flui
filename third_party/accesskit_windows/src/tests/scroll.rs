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
const SLIDER_ID: NodeId = NodeId(3);
const WIDE_ID: NodeId = NodeId(4);

/// A list 100 high that scrolls 300 vertically, scrolled to 75; a label that
/// does not scroll; the list's position slider, with its extents and no
/// scroll action; and a strip whose horizontal range is wider than `f64::MAX`.
fn get_initial_state() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_children(vec![LIST_ID, LABEL_ID, SLIDER_ID, WIDE_ID]);
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
    let mut slider = Node::new(Role::Slider);
    slider.set_author_id("slider");
    slider.set_numeric_value(75.0);
    slider.set_min_numeric_value(0.0);
    slider.set_max_numeric_value(300.0);
    slider.set_scroll_y(75.0);
    slider.set_scroll_y_min(0.0);
    slider.set_scroll_y_max(300.0);
    slider.add_action(Action::SetValue);
    let mut wide = Node::new(Role::ScrollView);
    wide.set_author_id("wide");
    wide.set_bounds(Rect::new(0.0, 100.0, 200.0, 140.0));
    wide.set_scroll_x(0.0);
    wide.set_scroll_x_min(-1e308);
    wide.set_scroll_x_max(1e308);
    wide.add_action(Action::SetScrollOffset);
    TreeUpdate {
        nodes: vec![
            (WINDOW_ID, root),
            (LIST_ID, list),
            (LABEL_ID, label),
            (SLIDER_ID, slider),
            (WIDE_ID, wide),
        ],
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

            let slider = find(s, "slider")?;
            assert!(
                unsafe { slider.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId) }
                    .is_err(),
                "an adjuster with scroll extents and no scroll action has no scroll pattern"
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

            let wide = find(s, "wide")?;
            let wide: IUIAutomationScrollPattern =
                unsafe { wide.GetCurrentPatternAs(UIA_ScrollPatternId) }?;
            assert_eq!(
                unsafe { wide.CurrentHorizontalScrollPercent() }?,
                50.0,
                "a range wider than f64::MAX reads finite"
            );
            unsafe { wide.SetScrollPercent(100.0, UIA_ScrollPatternNoScroll) }?;
            let recorded: Vec<_> = requests
                .lock()
                .drain(..)
                .map(|request| (request.target_node, request.data))
                .collect();
            assert_eq!(
                recorded,
                [(
                    WIDE_ID,
                    Some(ActionData::SetScrollOffset(Point::new(1e308, 0.0)))
                )],
                "and scrolls to a finite position"
            );
            Ok(())
        },
    )
}
