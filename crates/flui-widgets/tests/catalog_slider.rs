//! Mounted Slider contracts: input proposals, allocated paint and live numeric actions.
use crate::common::{lay_out, tight};
use flui_foundation::geometry::Size;
use flui_interaction::events::{Code, Key, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_painting::{DrawOp, PaintStyle, typography::TextDirection};
use flui_rendering::{
    constraints::BoxConstraints,
    layer::Layer,
    semantics::{
        AccessibilityNodeId, ActionArgs, NumericRange, SemanticsAction, SemanticsActionRequest,
    },
};
use flui_testing::{Action, ActionData, ActionRequest, NodeId, TreeId, widgets::LaidOut};
use flui_widgets::{Directionality, SizedBox, Slider};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Proposals = Rc<RefCell<Vec<f64>>>;
fn range(value: f64, min: f64, max: f64, step: f64) -> NumericRange {
    NumericRange::new(value, min, max, step).expect("valid range")
}
fn slider(range: NumericRange, log: &Proposals) -> Slider {
    let log = Rc::clone(log);
    Slider::new(range)
        .label("Volume")
        .on_changed(move |_cx, value| log.borrow_mut().push(value))
}
fn standard(log: &Proposals) -> Slider {
    slider(range(25.0, 0.0, 100.0, 10.0), log)
}
fn ops(laid: &LaidOut) -> Vec<DrawOp> {
    let mut ops = Vec::new();
    for (_, node) in laid.layer_tree().expect("mounted frame painted").iter() {
        if let Layer::Picture(picture) = node.layer() {
            ops.extend(
                picture
                    .picture()
                    .commands()
                    .iter()
                    .map(|command| command.op.clone()),
            );
        }
    }
    ops
}
fn thumb(laid: &LaidOut) -> f64 {
    ops(laid)
        .iter()
        .find_map(|op| match op {
            DrawOp::Circle { center, paint, .. } if paint.style == PaintStyle::Fill => {
                Some(center.x)
            }
            _ => None,
        })
        .expect("thumb was painted")
}
fn rings(laid: &LaidOut) -> usize {
    ops(laid)
        .iter()
        .filter(
            |op| matches!(op, DrawOp::Circle { paint, .. } if paint.style == PaintStyle::Stroke),
        )
        .count()
}
fn tap(laid: &LaidOut, x: f64, y: f64) {
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);
}
fn semantic_node(laid: &LaidOut) -> NodeId {
    laid.a11y_tree()
        .expect("semantics enabled")
        .find_by_label("Volume")
        .expect("numeric slider node")
        .id()
}
fn request(action: Action, node: NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data,
    }
}
fn semantic_mount(log: &Proposals) -> LaidOut {
    let mut laid = lay_out(standard(log), tight(100.0, 32.0));
    laid.enable_semantics();
    laid.pump();
    laid
}
fn key(laid: &LaidOut, named: NamedKey) -> bool {
    laid.focus_manager().dispatch_key_event(
        &KeyEventBuilder::new(Code::Unidentified)
            .with_state(KeyState::Down)
            .with_key(Key::Named(named))
            .build(),
    )
}

pub(crate) fn slider_uses_allocated_fractional_bounds_for_paint_and_pointer_mapping() {
    fn check(constraints: BoxConstraints) {
        let log = Proposals::default();
        let laid = lay_out(standard(&log), constraints);
        assert_eq!(laid.size(laid.root()), Size::new(80.5, 18.25));
        assert_eq!(thumb(&laid), 24.125);
        assert!(ops(&laid).iter().any(|op| matches!(op, DrawOp::ClipRect { rect, .. } if rect.width() == 80.5 && rect.height() == 18.25)));
        tap(&laid, 8.0, 9.0);
        tap(&laid, 72.5, 9.0);
        assert_eq!(&*log.borrow(), &[0.0, 100.0]);
        tap(&laid, 90.0, 9.0);
        assert_eq!(
            log.borrow().len(),
            2,
            "outside allocated bounds cannot hit control"
        );
    }
    fn tight_case() {
        check(tight(80.5, 18.25));
    }
    fn loose_case() {
        check(BoxConstraints::new(0.0, 80.5, 0.0, 18.25));
    }
    fn unbounded_case() {
        let log = Proposals::default();
        let laid = lay_out(standard(&log), BoxConstraints::UNCONSTRAINED);
        assert_eq!(laid.size(laid.root()), Size::new(160.0, 32.0));
    }
    crate::common::cases::run_cases(
        "slider allocated geometry",
        &[
            ("tight_fractional", tight_case),
            ("loose_fractional", loose_case),
            ("unbounded_preference", unbounded_case),
        ],
    );
}

pub(crate) fn slider_drag_proposals_do_not_commit_without_parent_update() {
    let log = Proposals::default();
    let mut laid = semantic_mount(&log);
    let before = thumb(&laid);
    laid.dispatch_pointer_down(29.0, 16.0);
    laid.dispatch_pointer_move(71.0, 16.0);
    laid.dispatch_pointer_move(50.0, 16.0);
    laid.dispatch_pointer_up(50.0, 16.0);
    assert!(
        log.borrow().contains(&75.0),
        "real arena accepted horizontal drag"
    );
    assert_eq!(log.borrow().last(), Some(&50.0));
    assert!(key(&laid, NamedKey::ArrowRight));
    assert_eq!(
        log.borrow().last(),
        Some(&35.0),
        "step after a drag proposal reads the owner's unchanged value"
    );
    laid.tick();
    assert_eq!(
        thumb(&laid),
        before,
        "proposals alone do not commit the thumb"
    );
    assert_eq!(
        laid.a11y_tree()
            .expect("tree")
            .find_by_label("Volume")
            .expect("slider")
            .raw()
            .numeric_value(),
        Some(25.0)
    );
    laid.pump_widget(slider(range(50.0, 0.0, 100.0, 10.0), &log));
    assert_eq!(thumb(&laid), 50.0);
    assert_eq!(
        laid.a11y_tree()
            .expect("tree")
            .find_by_label("Volume")
            .expect("slider")
            .raw()
            .numeric_value(),
        Some(50.0)
    );
}

pub(crate) fn slider_ambient_rtl_and_explicit_override_mirror_input_and_thumb() {
    let log = Proposals::default();
    let mut laid = lay_out(
        Directionality::new(TextDirection::Rtl, standard(&log)),
        tight(100.0, 32.0),
    );
    assert_eq!(thumb(&laid), 71.0);
    tap(&laid, 8.0, 16.0);
    assert_eq!(log.borrow().last(), Some(&100.0));
    assert!(key(&laid, NamedKey::ArrowRight));
    assert_eq!(log.borrow().last(), Some(&15.0));
    laid.pump_widget(Directionality::new(
        TextDirection::Rtl,
        standard(&log).text_direction(TextDirection::Ltr),
    ));
    assert_eq!(thumb(&laid), 29.0);
    tap(&laid, 8.0, 16.0);
    assert_eq!(log.borrow().last(), Some(&0.0));
    assert!(key(&laid, NamedKey::ArrowRight));
    assert_eq!(log.borrow().last(), Some(&35.0));
    laid.pump_widget(Directionality::new(TextDirection::Ltr, standard(&log)));
    assert_eq!(
        thumb(&laid),
        29.0,
        "ambient dependency updates actual painter"
    );
}

pub(crate) fn slider_extreme_range_interpolation_and_step_remain_finite() {
    let log = Proposals::default();
    let mut laid = lay_out(
        slider(range(0.0, -f64::MAX, f64::MAX, f64::MAX), &log),
        tight(100.0, 32.0),
    );
    assert_eq!(thumb(&laid), 50.0);
    tap(&laid, 29.0, 16.0);
    tap(&laid, 50.0, 16.0);
    tap(&laid, 71.0, 16.0);
    assert_eq!(&*log.borrow(), &[-f64::MAX / 2.0, 0.0, f64::MAX / 2.0]);
    laid.pump_widget(slider(
        range(f64::MAX / 2.0, -f64::MAX, f64::MAX, f64::MAX),
        &log,
    ));
    assert!(key(&laid, NamedKey::ArrowRight));
    assert_eq!(
        log.borrow().last(),
        Some(&f64::MAX),
        "overflowing positive step saturates"
    );
    laid.pump_widget(slider(
        range(-f64::MAX / 2.0, -f64::MAX, f64::MAX, f64::MAX),
        &log,
    ));
    assert!(key(&laid, NamedKey::ArrowLeft));
    assert_eq!(log.borrow().last(), Some(&-f64::MAX));
    assert!(log.borrow().iter().all(|value| value.is_finite()));
}

pub(crate) fn slider_degenerate_geometry_or_span_is_inert() {
    let log = Proposals::default();
    let laid = lay_out(standard(&log), tight(0.0, 32.0));
    tap(&laid, 0.0, 16.0);
    assert!(
        log.borrow().is_empty(),
        "zero-width pointer mapping is inert"
    );
    // Geometry gates pointers, not otherwise valid numeric input: platform
    // input after a resize to zero width is still admitted.
    let mut numeric = lay_out(
        SizedBox::new(100.0, 32.0).child(standard(&log)),
        BoxConstraints::new(0.0, 100.0, 0.0, 32.0),
    );
    numeric.enable_semantics();
    numeric.pump();
    let node = semantic_node(&numeric);
    numeric
        .invoke_semantics_action(request(Action::Focus, node, None))
        .expect("focus before resize");
    numeric.pump_widget(SizedBox::new(0.0, 32.0).child(standard(&log)));
    let node = semantic_node(&numeric);
    let listener = numeric
        .accessibility_action_listener()
        .expect("platform action listener after resize");
    listener(request(
        Action::SetValue,
        node,
        Some(ActionData::NumericValue(33.125)),
    ));
    numeric.tick();
    assert!(key(&numeric, NamedKey::ArrowRight));
    assert_eq!(
        &*log.borrow(),
        &[33.125, 35.0],
        "zero geometry does not add a numeric admission rule"
    );
    log.borrow_mut().clear();
    let mut laid = lay_out(slider(range(7.0, 7.0, 7.0, 1.0), &log), tight(100.0, 32.0));
    laid.enable_semantics();
    laid.pump();
    tap(&laid, 50.0, 16.0);
    assert!(log.borrow().is_empty());
    assert!(!key(&laid, NamedKey::ArrowRight));
    assert!(
        !laid
            .a11y_tree()
            .expect("tree")
            .find_by_label("Volume")
            .expect("slider")
            .supports_action(Action::Increment)
    );
}

pub(crate) fn slider_focus_keys_and_semantic_actions_share_controlled_proposals() {
    let log = Proposals::default();
    let mut laid = semantic_mount(&log);
    let node = semantic_node(&laid);
    assert_eq!(rings(&laid), 0);
    laid.invoke_semantics_action(request(Action::Focus, node, None))
        .expect("real focus route");
    laid.tick();
    assert_eq!(rings(&laid), 1, "focus edge repaints outline");
    for named in [
        NamedKey::ArrowRight,
        NamedKey::ArrowLeft,
        NamedKey::Home,
        NamedKey::End,
    ] {
        assert!(key(&laid, named));
    }
    assert_eq!(&*log.borrow(), &[35.0, 15.0, 0.0, 100.0]);
    for modifiers in [
        flui_interaction::events::Modifiers::NONE,
        flui_interaction::events::Modifiers::SHIFT,
    ] {
        assert!(
            laid.focus_manager().dispatch_key_event(
                &KeyEventBuilder::new(Code::Unidentified)
                    .with_state(KeyState::Down)
                    .with_key(Key::character("+"))
                    .with_modifiers(modifiers)
                    .build()
            )
        );
        assert_eq!(log.borrow().last(), Some(&35.0));
    }
    assert!(
        !laid.focus_manager().dispatch_key_event(
            &KeyEventBuilder::new(Code::Unidentified)
                .with_state(KeyState::Down)
                .with_key(Key::Named(NamedKey::ArrowRight))
                .with_modifiers(flui_interaction::events::Modifiers::SHIFT)
                .build()
        )
    );
    log.borrow_mut().truncate(4);
    let listener = laid
        .accessibility_action_listener()
        .expect("platform action listener");
    listener(request(Action::Increment, node, None));
    laid.tick();
    listener(request(Action::Decrement, node, None));
    laid.tick();
    listener(request(
        Action::SetValue,
        node,
        Some(ActionData::NumericValue(33.125)),
    ));
    laid.tick();
    assert_eq!(
        &log.borrow()[4..],
        &[35.0, 15.0, 33.125],
        "setter is exact rather than step-snapped"
    );
    let count = log.borrow().len();
    for data in [
        None,
        Some(ActionData::NumericValue(f64::NAN)),
        Some(ActionData::NumericValue(f64::INFINITY)),
        Some(ActionData::NumericValue(101.0)),
        Some(ActionData::Value("not numeric".into())),
    ] {
        listener(request(Action::SetValue, node, data));
        laid.tick();
    }
    assert_eq!(
        log.borrow().len(),
        count,
        "malformed and excluded values do not invoke callbacks"
    );

    laid.pump_widget(standard(&log));
    // The focused node persists but the actual canvas shrinks with its parent.
    // A separate mount exercises the outline where its radius exceeds bounds.
    let mut tiny = lay_out(standard(&log), tight(3.5, 2.25));
    tiny.enable_semantics();
    tiny.pump();
    let node = semantic_node(&tiny);
    tiny.invoke_semantics_action(request(Action::Focus, node, None))
        .expect("focus tiny control");
    tiny.tick();
    assert_eq!(rings(&tiny), 1);
    let commands = ops(&tiny);
    let clip = commands.iter().position(|op| matches!(op, DrawOp::ClipRect { rect, .. } if rect.width() == 3.5 && rect.height() == 2.25)).expect("actual tiny allocation clip");
    let outline = commands
        .iter()
        .position(
            |op| matches!(op, DrawOp::Circle { paint, .. } if paint.style == PaintStyle::Stroke),
        )
        .expect("outline paint");
    assert!(clip < outline, "outline is emitted inside allocated clip");
}

pub(crate) fn slider_disable_callback_replacement_and_unmount_retire_old_actions() {
    let log = Proposals::default();
    let replacement = Proposals::default();
    let mut laid = semantic_mount(&log);
    let node = semantic_node(&laid);
    let invocation = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node.0).expect("valid node"),
                action: SemanticsAction::SetNumericValue,
                arguments: Some(ActionArgs::SetNumericValue { value: 80.0 }),
            })
        })
        .expect("cache admitted setter");
    laid.pump_widget(slider(range(25.0, 0.0, 50.0, 5.0), &replacement));
    laid.enter_owner_scope(|| invocation.invoke());
    assert!(
        replacement.borrow().is_empty(),
        "cached setter rechecks current narrowed range"
    );
    assert!(log.borrow().is_empty());
    // An accepted down survives an ordinary controlled configuration update.
    // Up invokes the current callback, not the one registered at contact time.
    laid.dispatch_pointer_down(71.0, 16.0);
    laid.pump_widget(slider(range(25.0, 0.0, 50.0, 5.0), &log));
    laid.dispatch_pointer_up(71.0, 16.0);
    assert_eq!(&*log.borrow(), &[37.5]);
    assert!(replacement.borrow().is_empty());
    log.borrow_mut().clear();
    laid.pump_widget(slider(range(25.0, 0.0, 50.0, 5.0), &replacement));
    let node = semantic_node(&laid);
    laid.invoke_semantics_action(request(
        Action::SetValue,
        node,
        Some(ActionData::NumericValue(33.125)),
    ))
    .expect("current setter");
    assert_eq!(&*replacement.borrow(), &[33.125]);
    let old = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node.0).expect("valid node"),
                action: SemanticsAction::Increase,
                arguments: None,
            })
        })
        .expect("cache increment");
    laid.dispatch_pointer_down(71.0, 16.0);
    laid.pump_widget(Slider::new(range(25.0, 0.0, 50.0, 5.0)).label("Volume"));
    laid.dispatch_pointer_up(71.0, 16.0);
    tap(&laid, 50.0, 16.0);
    assert!(!key(&laid, NamedKey::ArrowRight));
    laid.enter_owner_scope(|| old.invoke());
    assert_eq!(
        replacement.borrow().len(),
        1,
        "disablement retires current callbacks"
    );
    // Cache a fresh action, remove its owner, then exercise a replacement owner
    // in the same presentation to prove the stale request cannot break progress.
    laid.pump_widget(standard(&replacement));
    tap(&laid, 50.0, 16.0);
    assert_eq!(
        &*replacement.borrow(),
        &[33.125, 50.0],
        "next contact succeeds after mid-contact disablement"
    );
    // An arena-accepted drag is also cancelled by disabling the consumer.
    // Reenabling must admit a new contact after the interrupted terminal event.
    {
        let drag_log = Proposals::default();
        let mut drag = lay_out(standard(&drag_log), tight(100.0, 32.0));
        drag.dispatch_pointer_down(29.0, 16.0);
        drag.dispatch_pointer_move(71.0, 16.0);
        assert!(drag_log.borrow().contains(&75.0));
        let accepted = drag_log.borrow().len();
        drag.pump_widget(Slider::new(range(25.0, 0.0, 100.0, 10.0)));
        drag.dispatch_pointer_move(50.0, 16.0);
        drag.dispatch_pointer_up(50.0, 16.0);
        assert_eq!(drag_log.borrow().len(), accepted);
        drag.pump_widget(standard(&drag_log));
        tap(&drag, 50.0, 16.0);
        assert_eq!(drag_log.borrow().len(), accepted + 1);
        assert_eq!(drag_log.borrow().last(), Some(&50.0));
    }
    let node = semantic_node(&laid);
    let stale = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node.0).expect("valid node"),
                action: SemanticsAction::Increase,
                arguments: None,
            })
        })
        .expect("cache live action");
    laid.pump_widget(SizedBox::new(100.0, 32.0));
    laid.enter_owner_scope(|| stale.invoke());
    assert_eq!(replacement.borrow().len(), 2);
    laid.pump_widget(standard(&replacement));
    let node = semantic_node(&laid);
    laid.invoke_semantics_action(request(Action::Increment, node, None))
        .expect("replacement owner remains usable");
    assert_eq!(&*replacement.borrow(), &[33.125, 50.0, 35.0]);
}

pub(crate) fn slider_focus_manager_close_during_pointer_focus_rejects_proposal() {
    let log = Proposals::default();
    let laid = lay_out(standard(&log), tight(100.0, 32.0));
    let manager = laid.focus_manager();
    let closing = Rc::downgrade(&manager);
    let closed = Rc::new(Cell::new(false));
    let observed = Rc::clone(&closed);
    manager.add_listener(Rc::new(move |_previous, current| {
        if current.is_some() {
            observed.set(true);
            closing.upgrade().expect("live focus manager").close();
        }
    }));
    tap(&laid, 71.0, 16.0);
    assert!(closed.get(), "actual focus listener closed its manager");
    assert!(manager.is_closed());
    assert!(manager.primary_focus().is_none());
    assert!(
        log.borrow().is_empty(),
        "detached focus cannot propose after reentry"
    );
    // This closes only the focus manager; the next independent presentation
    // must still accept input through its own live manager and writer context.
    let healthy_log = Proposals::default();
    let healthy = lay_out(standard(&healthy_log), tight(100.0, 32.0));
    tap(&healthy, 71.0, 16.0);
    assert_eq!(&*healthy_log.borrow(), &[75.0]);
}
