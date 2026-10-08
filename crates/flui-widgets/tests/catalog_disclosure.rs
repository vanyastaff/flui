//! Public disclosure flows: controlled changes, real focus, and composited output.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::{Matrix4, Point, Rect};
use flui_interaction::events::{Code, Key, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_painting::display_list::DrawOp;
use flui_painting::paint::Clip;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::layer::Layer;
use flui_rendering::semantics::{AccessibilityNodeId, SemanticsAction, SemanticsActionRequest};
use flui_testing::{Action, ActionRequest, NodeId, TreeId};
use flui_view::{BuildContext, IntoView, StatelessView, View};
use flui_widgets::{
    ColoredBox, Directionality, Disclosure, ExpansionState, GestureDetector, OverflowBox,
    Semantics, SizedBox,
};

use crate::common::{LaidOut, lay_out, loose, tight};

type Proposals = Rc<RefCell<Vec<ExpansionState>>>;

fn section(
    state: ExpansionState,
    proposals: Option<&Proposals>,
    body_taps: &Rc<Cell<usize>>,
) -> Disclosure {
    let taps = Rc::clone(body_taps);
    let header = Semantics::new()
        .label("Section")
        .child(SizedBox::new(60.0, 24.0));
    let body = Semantics::new().container(true).label("Body").child(
        GestureDetector::new()
            .on_tap(move |_| taps.set(taps.get() + 1))
            .child(SizedBox::new(60.0, 30.0).child(ColoredBox::new(Color::RED))),
    );
    let view = Disclosure::new(state, header, body);
    match proposals {
        Some(proposals) => {
            let proposals = Rc::clone(proposals);
            view.on_changed(move |_cx, state| proposals.borrow_mut().push(state))
        }
        None => view,
    }
}

fn header_node(app: &mut LaidOut) -> NodeId {
    app.enable_semantics();
    app.tick();
    app.a11y_tree()
        .expect("semantics enabled")
        .find_by_label("Section")
        .expect("one accessible header")
        .id()
}

fn action(app: &LaidOut, id: NodeId, action: Action) {
    app.invoke_semantics_action(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: id,
        data: None,
    })
    .expect("the header advertises the requested action");
}

fn key(app: &LaidOut, key: Key) -> bool {
    app.enter_owner_scope(|| {
        app.focus_manager()
            .dispatch_key_event(
                &KeyEventBuilder::new(Code::Unidentified)
                    .with_key(key)
                    .with_state(KeyState::Down)
                    .build(),
            )
            .is_handled()
    })
}

fn indicator_lines(app: &LaidOut) -> Vec<(Point, Point, f64, Color)> {
    let mut lines = Vec::new();
    for (_, node) in app.layer_tree().expect("a frame painted").iter() {
        if let Layer::Picture(picture) = node.layer() {
            for command in picture.picture() {
                if let DrawOp::Line { p1, p2, paint } = &command.op {
                    lines.push((*p1, *p2, paint.stroke_width, paint.color));
                }
            }
        }
    }
    lines
}

fn has_body_command(app: &LaidOut) -> bool {
    app.layer_tree()
        .expect("a frame painted")
        .iter()
        .any(|(_, node)| {
            if let Layer::Picture(picture) = node.layer() {
                picture.picture().iter().any(|command| {
                matches!(&command.op, DrawOp::Rect { paint, .. } if paint.color == Color::RED)
            })
            } else {
                false
            }
        })
}

struct BodyPaint {
    recorded: Rect,
    visible: Option<Rect>,
    clips: Vec<Rect>,
}

fn body_paint(app: &LaidOut) -> Vec<BodyPaint> {
    let tree = app.layer_tree().expect("a frame painted");
    let mut output = Vec::new();
    for (id, node) in tree.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        for command in picture.picture() {
            let DrawOp::Rect { rect, paint } = &command.op else {
                continue;
            };
            if paint.color != Color::RED {
                continue;
            }
            let mut recorded = command.transform.transform_rect(rect);
            let mut visible =
                (recorded.width() > 0.0 && recorded.height() > 0.0).then_some(recorded);
            let mut clips: Vec<Rect> = Vec::new();
            let mut parent = tree.parent(id);
            while let Some(id) = parent {
                let layer = tree.get_layer(id).expect("live layer ancestor");
                let transform = match layer {
                    Layer::Offset(offset) => Some(Matrix4::translation(
                        offset.offset().dx,
                        offset.offset().dy,
                        0.0,
                    )),
                    Layer::Transform(transform) => Some(*transform.transform()),
                    _ => None,
                };
                if let Some(transform) = transform {
                    recorded = transform.transform_rect(&recorded);
                    visible = visible.map(|rect| transform.transform_rect(&rect));
                    for clip in &mut clips {
                        *clip = transform.transform_rect(clip);
                    }
                }
                if let Layer::ClipRect(clip) = layer
                    && clip.clip_behavior() != Clip::None
                {
                    let bounds = clip.clip_rect();
                    visible = visible
                        .and_then(|rect| rect.intersect(&bounds))
                        .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0);
                    clips.push(bounds);
                }
                parent = tree.parent(id);
            }
            output.push(BodyPaint {
                recorded,
                visible,
                clips,
            });
        }
    }
    output
}

fn paints_body(app: &LaidOut) -> bool {
    body_paint(app).iter().any(|paint| paint.visible.is_some())
}

pub(crate) fn disclosure_proposes_controlled_changes_and_retains_real_header_focus() {
    let proposals = Rc::new(RefCell::new(Vec::new()));
    let taps = Rc::new(Cell::new(0));
    let mut app = lay_out(
        section(ExpansionState::Collapsed, Some(&proposals), &taps),
        loose(200.0),
    );
    let id = header_node(&mut app);
    let tree = app.a11y_tree().expect("enabled");
    let header = tree.find_by_label("Section").expect("one coherent header");
    assert!(header.supports_action(Action::Focus));
    assert!(header.supports_action(Action::Expand));
    assert!(!header.supports_action(Action::Collapse));
    assert_eq!(header.raw().is_expanded(), Some(false));
    let lines = indicator_lines(&app);
    assert_eq!(lines.len(), 2, "the collapsed affordance actually paints");
    assert!(lines.iter().all(|line| line.3 == Color::BLACK));
    assert!(
        lines[0].1.x > lines[0].0.x,
        "LTR collapsed chevron points forward"
    );
    assert!(!has_body_command(&app));
    assert!(
        app.a11y_tree()
            .expect("enabled")
            .find_by_label("Body")
            .is_err()
    );
    app.dispatch_pointer_down(20.0, 10.0);
    app.dispatch_pointer_up(20.0, 10.0);
    app.tick();
    assert_eq!(*proposals.borrow(), vec![ExpansionState::Expanded]);
    assert!(
        !has_body_command(&app),
        "a proposal does not mutate controlled state"
    );
    let focused = app
        .focus_manager()
        .primary_focus()
        .expect("header tap takes focus");
    assert!(
        indicator_lines(&app)[0].2 > lines[0].2,
        "focus changes the painted indicator"
    );
    assert!(key(&app, Key::Named(NamedKey::Enter)));
    assert!(key(&app, Key::character(" ")));
    assert_eq!(
        proposals.borrow().len(),
        3,
        "both actual keys reach the header action"
    );
    let retained_expand = app
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(id.0).expect("exported identity"),
                action: SemanticsAction::Expand,
                arguments: None,
            })
        })
        .expect("collapsed header advertises Expand");
    action(&app, id, Action::Expand);
    assert_eq!(proposals.borrow().len(), 4);

    app.pump_widget(section(ExpansionState::Expanded, Some(&proposals), &taps));
    let expanded_id = header_node(&mut app);
    let tree = app.a11y_tree().expect("enabled");
    let expanded_header = tree.find_by_label("Section").expect("one coherent header");
    assert!(expanded_header.supports_action(Action::Focus));
    assert!(expanded_header.supports_action(Action::Collapse));
    assert!(!expanded_header.supports_action(Action::Expand));
    assert_eq!(
        app.a11y_tree()
            .expect("enabled")
            .find_by_label("Section")
            .expect("one coherent header")
            .raw()
            .is_expanded(),
        Some(true)
    );
    assert_eq!(
        app.focus_manager()
            .primary_focus()
            .expect("focus survives owner update")
            .id(),
        focused.id()
    );
    assert!(
        paints_body(&app),
        "accepted expansion paints the actual body"
    );
    let tree = app.a11y_tree().expect("enabled");
    let body = tree
        .find_by_label("Body")
        .expect("expanded body is accessible");
    let rect = body.bounds().expect("body has actual bounds");
    app.dispatch_pointer_down(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
    app.dispatch_pointer_up(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
    assert_eq!(taps.get(), 1, "expanded body receives actual input");
    app.enter_owner_scope(|| retained_expand.invoke());
    assert_eq!(
        proposals.borrow().len(),
        4,
        "retained Expand is idempotent after the owner accepts expansion"
    );
    action(&app, expanded_id, Action::Collapse);
    assert_eq!(proposals.borrow().last(), Some(&ExpansionState::Collapsed));
    let retained_collapse = app
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(expanded_id.0).expect("exported identity"),
                action: SemanticsAction::Collapse,
                arguments: None,
            })
        })
        .expect("expanded header advertises Collapse");
    app.pump_widget(section(ExpansionState::Collapsed, Some(&proposals), &taps));
    app.enter_owner_scope(|| retained_collapse.invoke());
    assert_eq!(
        proposals.borrow().len(),
        5,
        "retained Collapse is idempotent after the owner accepts collapse"
    );
    assert!(!has_body_command(&app));
    app.dispatch_pointer_down(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
    app.dispatch_pointer_up(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
    assert_eq!(
        taps.get(),
        1,
        "collapsed body cannot receive input at its old location"
    );
}

pub(crate) fn disclosure_disabled_and_replaced_handlers_do_not_run_old_proposals() {
    let old = Rc::new(RefCell::new(Vec::new()));
    let next = Rc::new(RefCell::new(Vec::new()));
    let taps = Rc::new(Cell::new(0));
    let mut app = lay_out(
        section(ExpansionState::Collapsed, Some(&old), &taps),
        loose(200.0),
    );
    let id = header_node(&mut app);
    action(&app, id, Action::Focus);
    app.tick();
    assert!(
        app.focus_manager().primary_focus().is_some(),
        "platform focus reaches actual keyboard node"
    );
    app.pump_widget(section(ExpansionState::Collapsed, Some(&next), &taps));
    assert!(key(&app, Key::Named(NamedKey::Enter)));
    assert!(old.borrow().is_empty());
    assert_eq!(*next.borrow(), vec![ExpansionState::Expanded]);
    app.dispatch_pointer_down(20.0, 10.0);
    app.pump_widget(section(ExpansionState::Collapsed, None, &taps));
    app.dispatch_pointer_up(20.0, 10.0);
    assert_eq!(next.borrow().len(), 1, "disabling cancels the old contact");
    let _disabled_id = header_node(&mut app);
    let tree = app.a11y_tree().expect("enabled");
    let header = tree
        .find_by_label("Section")
        .expect("disabled header remains accessible");
    assert!(header.is_disabled());
    assert!(!header.supports_action(Action::Expand));
    assert!(!header.supports_action(Action::Collapse));
    assert!(!header.supports_action(Action::Focus));
    app.dispatch_pointer_down(20.0, 10.0);
    app.dispatch_pointer_up(20.0, 10.0);
    let _ = key(&app, Key::character(" "));
    assert_eq!(next.borrow().len(), 1, "disabled input proposes nothing");
    app.pump_widget(section(ExpansionState::Expanded, Some(&next), &taps));
    app.dispatch_pointer_down(20.0, 10.0);
    app.dispatch_pointer_up(20.0, 10.0);
    assert_eq!(
        next.borrow().len(),
        2,
        "the fresh recognizer accepts a new tap"
    );
    assert_eq!(next.borrow().last(), Some(&ExpansionState::Collapsed));
    let new_id = header_node(&mut app);
    action(&app, new_id, Action::Focus);
    assert!(key(&app, Key::character(" ")));
    assert_eq!(next.borrow().last(), Some(&ExpansionState::Collapsed));
    app.pump_widget(section(ExpansionState::Expanded, Some(&next), &taps).label("Updated"));
    app.tick();
    let tree = app.a11y_tree().expect("enabled");
    let header = tree
        .find_by_label("Updated")
        .expect("explicit name reached the current header");
    assert!(
        tree.find_by_label("Section").is_err(),
        "explicit naming excludes old header descendant semantics"
    );
    assert!(
        tree.find_by_label("Body").is_ok(),
        "explicit naming does not exclude body semantics"
    );
    assert!(header.supports_action(Action::Focus));
    assert!(!header.supports_action(Action::Expand));
    assert!(header.supports_action(Action::Collapse));
    assert_eq!(header.raw().is_expanded(), Some(true));
}

pub(crate) fn disclosure_indicator_obeys_constraints_and_explicit_reading_direction() {
    fn loose_ltr() {
        geometry(loose(200.0), TextDirection::Ltr, None);
    }
    fn fractional_rtl() {
        geometry(tight(100.5, 65.25), TextDirection::Rtl, None);
    }
    fn tiny() {
        geometry(tight(8.5, 7.25), TextDirection::Ltr, None);
    }
    fn zero() {
        geometry(tight(0.0, 0.0), TextDirection::Rtl, None);
    }
    fn explicit_ltr() {
        geometry(loose(200.0), TextDirection::Rtl, Some(TextDirection::Ltr));
    }
    crate::common::cases::run_cases(
        "disclosure geometry",
        &[
            ("loose LTR", loose_ltr as fn()),
            ("fractional RTL", fractional_rtl as fn()),
            ("tiny", tiny as fn()),
            ("zero", zero as fn()),
            ("explicit LTR", explicit_ltr as fn()),
        ],
    );
}

pub(crate) fn disclosure_focus_reentry_cannot_activate_after_focus_manager_close() {
    let proposals = Rc::new(RefCell::new(Vec::new()));
    let taps = Rc::new(Cell::new(0));
    let app = lay_out(
        section(ExpansionState::Collapsed, Some(&proposals), &taps),
        loose(200.0),
    );
    let manager = app.focus_manager();
    let closing = Rc::clone(&manager);
    manager.add_listener(Rc::new(move |_old, next| {
        if next.is_some() {
            closing.close();
        }
    }));
    app.dispatch_pointer_down(20.0, 10.0);
    app.dispatch_pointer_up(20.0, 10.0);
    assert!(
        manager.is_closed(),
        "focus notification really closed the focus manager"
    );
    assert!(
        proposals.borrow().is_empty(),
        "no proposal may escape after reentrant close"
    );
    let mut next = lay_out(
        section(ExpansionState::Collapsed, Some(&proposals), &taps),
        loose(200.0),
    );
    next.dispatch_pointer_down(20.0, 10.0);
    next.dispatch_pointer_up(20.0, 10.0);
    assert_eq!(
        *proposals.borrow(),
        vec![ExpansionState::Expanded],
        "an independent live control still works"
    );
    next.tick();
}

pub(crate) fn disclosure_retained_expand_after_unmount_cannot_reach_a_new_control() {
    let old = Rc::new(RefCell::new(Vec::new()));
    let next = Rc::new(RefCell::new(Vec::new()));
    let taps = Rc::new(Cell::new(0));
    let mut app = lay_out(
        section(ExpansionState::Collapsed, Some(&old), &taps),
        loose(200.0),
    );
    let id = header_node(&mut app);
    let cached = app
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(id.0).expect("exported identity"),
                action: SemanticsAction::Expand,
                arguments: None,
            })
        })
        .expect("mounted header advertises Expand");
    app.pump_widget(SizedBox::new(20.0, 20.0));
    app.enter_owner_scope(|| cached.invoke());
    assert!(
        old.borrow().is_empty(),
        "retained action cannot invoke unmounted owner"
    );
    app.pump_widget(section(ExpansionState::Collapsed, Some(&next), &taps));
    let id = header_node(&mut app);
    action(&app, id, Action::Expand);
    assert!(old.borrow().is_empty());
    assert_eq!(*next.borrow(), vec![ExpansionState::Expanded]);
}

#[derive(Clone)]
struct RetirementProbe {
    name: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
    fails: Rc<Cell<bool>>,
}

impl RetirementProbe {
    fn observe(&self) {
        let _ = std::hint::black_box(self);
    }
}

impl Drop for RetirementProbe {
    fn drop(&mut self) {
        self.log.borrow_mut().push(self.name);
        assert!(!self.fails.get(), "{} retirement", self.name);
    }
}

struct HostileView {
    probe: RetirementProbe,
    clone_fails: bool,
}

impl Clone for HostileView {
    fn clone(&self) -> Self {
        assert!(!self.clone_fails, "{} clone", self.probe.name);
        Self {
            probe: self.probe.clone(),
            clone_fails: false,
        }
    }
}

impl View for HostileView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for HostileView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

struct FailingConversion(&'static str);

impl IntoView for FailingConversion {
    type View = SizedBox;
    fn into_view(self) -> Self::View {
        panic!("{} conversion", self.0);
    }
}

pub(crate) fn disclosure_owns_constructor_clone_and_retirement_failure_tails() {
    fn constructor() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let body = HostileView {
            probe: RetirementProbe {
                name: "body",
                log: Rc::clone(&log),
                fails: Rc::new(Cell::new(true)),
            },
            clone_fails: false,
        };
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Disclosure::new(ExpansionState::Collapsed, FailingConversion("header"), body)
        }));
        assert_panic(failure, "header conversion");
        assert!(
            log.borrow().is_empty(),
            "incoming constructor unwind retains hostile sibling"
        );
    }
    fn body_conversion() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let header = HostileView {
            probe: RetirementProbe {
                name: "header",
                log: Rc::clone(&log),
                fails: Rc::new(Cell::new(true)),
            },
            clone_fails: false,
        };
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Disclosure::new(ExpansionState::Collapsed, header, FailingConversion("body"))
        }));
        assert_panic(failure, "body conversion");
        assert!(
            log.borrow().is_empty(),
            "converted header remains protected"
        );
    }
    fn partial_clone() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let fails = Rc::new(Cell::new(true));
        let view = Disclosure::new(
            ExpansionState::Collapsed,
            HostileView {
                probe: RetirementProbe {
                    name: "header",
                    log: Rc::clone(&log),
                    fails: Rc::clone(&fails),
                },
                clone_fails: false,
            },
            HostileView {
                probe: RetirementProbe {
                    name: "body",
                    log: Rc::clone(&log),
                    fails: Rc::clone(&fails),
                },
                clone_fails: true,
            },
        );
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| view.clone()));
        assert_panic(failure, "body clone");
        assert!(
            log.borrow().is_empty(),
            "completed header clone stays protected when body clone throws"
        );
        fails.set(false);
        drop(view);
        assert_eq!(*log.borrow(), vec!["header", "body"]);
    }
    fn competing_drops() {
        drop_case(true, true, true);
    }
    fn body_drop() {
        drop_case(false, true, false);
    }
    fn header_drop() {
        drop_case(true, false, false);
    }
    fn healthy() {
        drop_case(false, false, false);
    }
    fn callback_drop() {
        drop_case(false, false, true);
    }
    fn active_unwind() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _view = Disclosure::new(
                ExpansionState::Collapsed,
                HostileView {
                    probe: RetirementProbe {
                        name: "header",
                        log: Rc::clone(&log),
                        fails: Rc::new(Cell::new(true)),
                    },
                    clone_fails: false,
                },
                HostileView {
                    probe: RetirementProbe {
                        name: "body",
                        log: Rc::clone(&log),
                        fails: Rc::new(Cell::new(true)),
                    },
                    clone_fails: false,
                },
            );
            panic!("outer failure");
        }));
        assert_panic(failure, "outer failure");
        assert!(
            log.borrow().is_empty(),
            "incoming unwind retains independently hostile children"
        );
    }
    crate::common::cases::run_cases(
        "disclosure retirement",
        &[
            ("constructor conversion", constructor as fn()),
            ("body conversion", body_conversion as fn()),
            ("partial clone", partial_clone as fn()),
            ("competing drops", competing_drops as fn()),
            ("body drop", body_drop as fn()),
            ("header drop", header_drop as fn()),
            ("healthy", healthy as fn()),
            ("callback drop", callback_drop as fn()),
            ("active unwind", active_unwind as fn()),
        ],
    );
}

fn assert_panic<T>(failure: std::thread::Result<T>, expected: &str) {
    let Err(payload) = failure else {
        panic!("expected {expected}");
    };
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("ordinary string panic");
    assert!(message.contains(expected), "first failure: {message}");
}

fn drop_case(header_fails: bool, body_fails: bool, callback_fails: bool) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let callback = RetirementProbe {
        name: "callback",
        log: Rc::clone(&log),
        fails: Rc::new(Cell::new(callback_fails)),
    };
    let view = Disclosure::new(
        ExpansionState::Collapsed,
        HostileView {
            probe: RetirementProbe {
                name: "header",
                log: Rc::clone(&log),
                fails: Rc::new(Cell::new(header_fails)),
            },
            clone_fails: false,
        },
        HostileView {
            probe: RetirementProbe {
                name: "body",
                log: Rc::clone(&log),
                fails: Rc::new(Cell::new(body_fails)),
            },
            clone_fails: false,
        },
    )
    .on_changed(move |_cx, _state| callback.observe());
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(view)));
    assert_eq!(
        failure.is_err(),
        header_fails || body_fails || callback_fails
    );
    if let Err(payload) = failure {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .expect("ordinary string panic");
        let first = if header_fails {
            "header retirement"
        } else if body_fails {
            "body retirement"
        } else {
            "callback retirement"
        };
        assert!(
            message.contains(first),
            "first failure stays authoritative: {message}"
        );
    }
    let expected = if header_fails {
        vec!["header"]
    } else if body_fails {
        vec!["header", "body"]
    } else {
        vec!["header", "body", "callback"]
    };
    assert_eq!(
        *log.borrow(),
        expected,
        "first retirement failure owns the surviving tail"
    );
    let next = Disclosure::new(
        ExpansionState::Collapsed,
        SizedBox::new(1.0, 1.0),
        SizedBox::new(1.0, 1.0),
    );
    drop(next);
}

fn geometry(constraints: BoxConstraints, ambient: TextDirection, explicit: Option<TextDirection>) {
    let taps = Rc::new(Cell::new(0));
    let proposals = Rc::new(RefCell::new(Vec::new()));
    let body_taps = Rc::clone(&taps);
    let changed = Rc::clone(&proposals);
    let mut view = Disclosure::new(
        ExpansionState::Expanded,
        SizedBox::new(60.0, 24.0),
        OverflowBox::new()
            .with_min_width(90.0)
            .with_max_width(90.0)
            .with_min_height(90.0)
            .with_max_height(90.0)
            .child(
                GestureDetector::new()
                    .on_tap(move |_| body_taps.set(body_taps.get() + 1))
                    .child(ColoredBox::new(Color::RED)),
            ),
    )
    .on_changed(move |_cx, target| changed.borrow_mut().push(target))
    .indicator_color(Color::BLUE);
    if let Some(explicit) = explicit {
        view = view.text_direction(explicit);
    }
    let mut app = lay_out(Directionality::new(ambient, view), constraints);
    app.tick();
    let size = app.size(app.root());
    assert!(size.width.is_finite() && size.height.is_finite());
    let indicators = app.find_all_by_render_type("RenderCustomPaint");
    assert_eq!(indicators.len(), 1);
    let indicator = indicators[0];
    let offset = app.absolute_offset(indicator);
    let header = app
        .find_all_by_render_type("RenderFlex")
        .into_iter()
        .find(|id| app.children(*id).contains(&indicator))
        .expect("indicator belongs to the actual header row");
    let header_offset = app.absolute_offset(header);
    let header_size = app.size(header);
    let indicator_x = offset.dx - header_offset.dx;
    let indicator_size = app.size(indicator);
    assert!(
        indicator_size.width <= constraints.max_width
            && indicator_size.height <= constraints.max_height
    );
    let lines = indicator_lines(&app);
    if size.width == 0.0 || size.height == 0.0 {
        assert!(lines.is_empty(), "zero-sized indicator emits no paint");
    } else {
        assert_eq!(lines.len(), 2);
        assert!(
            lines.iter().all(|line| line.3 == Color::BLUE),
            "explicit color reaches actual paint"
        );
        assert!(
            lines[0].1.y > lines[0].0.y,
            "expanded indicator points down"
        );
        if explicit.unwrap_or(ambient) == TextDirection::Rtl {
            assert!(
                indicator_x < header_size.width / 2.0,
                "RTL indicator lies on the left"
            );
        } else if header_size.width >= 60.0 {
            assert!(
                indicator_x > header_size.width / 2.0,
                "LTR indicator lies on the right"
            );
        }
    }
    let sections = app.find_all_by_render_type("RenderClipRect");
    assert_eq!(sections.len(), 1, "the section owns one actual render clip");
    let section_id = sections[0];
    let section_size = app.size(section_id);
    let section_offset = app.absolute_offset(section_id);
    let section_clip = Rect::from_xywh(
        section_offset.dx,
        section_offset.dy,
        section_size.width,
        section_size.height,
    );
    let body = body_paint(&app);
    assert!(
        !body.is_empty(),
        "the oversized body records actual red paint"
    );
    for paint in &body {
        assert!(
            paint.clips.contains(&section_clip),
            "the body's picture has the section clip as an ancestor"
        );
        let expected = paint
            .recorded
            .intersect(&section_clip)
            .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0);
        assert_eq!(
            paint.visible, expected,
            "actual ancestor clip bounds determine effective body paint"
        );
    }
    if section_size.width < 90.0 || section_size.height < 90.0 {
        assert!(
            body.iter()
                .any(|paint| !section_clip.contains_rect(&paint.recorded)),
            "oversized body actually extends beyond the allocated clip"
        );
    }
    if section_size.width == 0.0 || section_size.height == 0.0 {
        assert!(!paints_body(&app), "zero allocation has no body paint");
    }
    let outside = Point::new(section_clip.max.x + 1.0, section_clip.max.y + 1.0);
    if section_size.width < 18.0 && section_size.height < 18.0 {
        assert!(
            body.iter().any(|paint| paint.recorded.contains(outside)),
            "outside-clip pointer lies on the actual overflowing body paint"
        );
    }
    app.dispatch_pointer_down(outside.x, outside.y);
    app.dispatch_pointer_up(outside.x, outside.y);
    assert_eq!(
        taps.get(),
        0,
        "clipped body cannot receive input beyond allocation"
    );
    assert!(proposals.borrow().is_empty());
    let mut collapsed = section(ExpansionState::Collapsed, Some(&proposals), &taps);
    if let Some(explicit) = explicit {
        collapsed = collapsed.text_direction(explicit);
    }
    app.pump_widget(Directionality::new(ambient, collapsed));
    let lines = indicator_lines(&app);
    if size.width > 0.0 && size.height > 0.0 {
        assert_eq!(lines.len(), 2);
        let dx = lines[0].1.x - lines[0].0.x;
        match explicit.unwrap_or(ambient) {
            TextDirection::Ltr => assert!(dx > 0.0, "collapsed LTR chevron points right"),
            TextDirection::Rtl => assert!(dx < 0.0, "collapsed RTL chevron points left"),
        }
    }
}
