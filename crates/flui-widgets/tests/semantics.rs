//! Widget-level coverage for the accessibility semantics wrappers.

use crate::common::{lay_out, loose};
use flui_rendering::semantics::{ActionCall, ActionDecodeError, SemanticsAction};
use flui_widgets::{MergeSemantics, Semantics, SemanticsRole, SizedBox};

struct CollectionChanges;
impl accesskit_consumer::TreeChangeHandler for CollectionChanges {
    fn node_added(&mut self, _: &accesskit_consumer::NodeRef<'_>) {}
    fn node_updated(
        &mut self,
        _: &accesskit_consumer::NodeRef<'_>,
        _: &accesskit_consumer::NodeRef<'_>,
    ) {
    }
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef<'_>>,
        _: Option<&accesskit_consumer::NodeRef<'_>>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef<'_>) {}
}

fn collection_consumer(
    laid: &mut crate::common::LaidOut,
) -> std::sync::Arc<std::sync::Mutex<accesskit_consumer::Tree>> {
    use std::sync::{Arc, Mutex};
    let consumer = Arc::new(Mutex::new(accesskit_consumer::Tree::new(
        laid.a11y_tree().expect("semantics enabled").raw().clone(),
        true,
    )));
    let delivered = Arc::clone(&consumer);
    laid.pipeline_owner().with_mut(|owner| {
        owner.set_semantics_update_callback(Arc::new(move |packet| {
            delivered
                .lock()
                .expect("consumer lock")
                .update_and_process_changes(packet.clone(), &mut CollectionChanges);
        }));
    });
    consumer
}

pub(crate) fn lazy_collection_totals_reach_the_real_consumer_after_incremental_updates() {
    use accesskit_consumer::common_filter;
    use flui_view::ViewExt;
    use flui_widgets::ListView;
    let content = |count, offset| {
        ListView::builder(count, 20.0, |index| {
            Some(
                Semantics::new()
                    .container(true)
                    .role(SemanticsRole::ListItem)
                    .label(format!("member {index}"))
                    .child(SizedBox::new(100.0, 20.0))
                    .boxed(),
            )
        })
        .offset(offset)
        .repaint_boundaries(false)
    };
    let mut laid = lay_out(content(100, 0.0), crate::common::tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let consumer = collection_consumer(&mut laid);
    for (count, offset, index) in [
        (100, 0.0, 0),
        (100, 600.0, 30),
        (7, 0.0, 0),
        (0, 0.0, 0),
        (20, 100.0, 5),
    ] {
        laid.pump_widget(content(count, offset));
        let published = laid.a11y_tree().expect("semantics enabled");
        if count == 0 {
            assert!(!published.nodes().any(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("member "))
            }));
            continue;
        }
        let member = published
            .find_by_label(&format!("member {index}"))
            .expect("real lazy row is published");
        let consumer = consumer.lock().expect("consumer lock");
        let member = consumer
            .state()
            .node_by_tree_local_id(member.id(), TreeId::ROOT)
            .expect("the delivered consumer holds this row");
        assert_eq!(member.position_in_set(), Some(index));
        assert_eq!(
            member.size_of_set_from_container(&common_filter),
            Some(count),
            "declared total must belong to an included container, not the member or resident band"
        );
    }
}

pub(crate) fn collection_consumer_preserves_unknown_and_explicit_nested_sets() {
    use accesskit_consumer::common_filter;
    use flui_view::{ViewExt, element::ItemCount};
    use flui_widgets::{IndexedSemantics, SliverList, Viewport};

    let content = |known_inner: bool, explicit: bool| {
        Viewport::new((SliverList::new(
            3,
            100.0,
            Rc::new(move |index| {
                let body = if index == 0 && explicit {
                    IndexedSemantics::new(7)
                        .child(SizedBox::new(100.0, 100.0))
                        .boxed()
                } else if index == 0 {
                    let count = if known_inner {
                        ItemCount::Exact(2)
                    } else {
                        ItemCount::Unknown
                    };
                    SizedBox::new(100.0, 100.0)
                        .child(Viewport::new((SliverList::new(
                            count,
                            20.0,
                            Rc::new(|inner| {
                                (inner < 2).then(|| {
                                    Semantics::new()
                                        .container(true)
                                        .role(SemanticsRole::ListItem)
                                        .label(format!("inner member {inner}"))
                                        .child(SizedBox::new(100.0, 20.0))
                                        .boxed()
                                })
                            }),
                        ),)))
                        .boxed()
                } else {
                    SizedBox::new(100.0, 100.0).boxed()
                };
                Some(
                    Semantics::new()
                        .container(true)
                        .role(SemanticsRole::ListItem)
                        .label(format!("outer member {index}"))
                        .child(body)
                        .boxed(),
                )
            }),
        ),))
    };
    let mut laid = lay_out(content(true, false), crate::common::tight(100.0, 300.0));
    laid.enable_semantics();
    laid.tick();
    let consumer = collection_consumer(&mut laid);
    for (known_inner, explicit) in [
        (true, false),
        (false, false),
        (true, false),
        (true, true),
        (true, false),
    ] {
        laid.pump_widget(content(known_inner, explicit));
        let published = laid.a11y_tree().expect("semantics enabled");
        let tree = consumer.lock().expect("consumer lock");
        let member = |label: &str| {
            let id = published
                .find_by_label(label)
                .expect("member published")
                .id();
            let node = tree
                .state()
                .node_by_tree_local_id(id, TreeId::ROOT)
                .expect("delivered member");
            (
                node.position_in_set(),
                node.size_of_set_from_container(&common_filter),
            )
        };
        assert_eq!(
            member("outer member 0"),
            (
                Some(if explicit { 7 } else { 0 }),
                (known_inner && !explicit).then_some(3)
            )
        );
        assert_eq!(
            member("outer member 1"),
            (Some(1), (known_inner && !explicit).then_some(3))
        );
        if !explicit {
            assert_eq!(
                member("inner member 0"),
                (Some(0), known_inner.then_some(2))
            );
        }
    }

    use flui_rendering::semantics::{
        SemanticsCollection, SemanticsCollectionKind, SemanticsConfiguration,
    };
    let mut authored = SemanticsConfiguration::new();
    authored.set_collection(SemanticsCollection::new(
        SemanticsCollectionKind::List,
        Some(10),
    ));
    laid.pump_widget(
        Semantics::from_configuration(authored).child(
            Semantics::new()
                .container(true)
                .role(SemanticsRole::ListItem)
                .label("authored member")
                .child(IndexedSemantics::new(7).child(SizedBox::new(100.0, 20.0))),
        ),
    );
    let published = laid.a11y_tree().expect("semantics enabled");
    let id = published
        .find_by_label("authored member")
        .expect("authored member published")
        .id();
    let tree = consumer.lock().expect("consumer lock");
    let node = tree
        .state()
        .node_by_tree_local_id(id, TreeId::ROOT)
        .expect("authored member delivered");
    assert_eq!(
        (
            node.position_in_set(),
            node.size_of_set_from_container(&common_filter)
        ),
        (Some(7), Some(10))
    );
}

pub(crate) fn collection_consumer_uses_member_totals_for_separated_and_composed_delegates() {
    use accesskit_consumer::common_filter;
    use flui_view::{
        ViewExt,
        element::{ItemCount, SemanticSetMapping},
    };
    use flui_widgets::{SliverList, Viewport};
    let row = |label| {
        Semantics::new()
            .container(true)
            .role(SemanticsRole::ListItem)
            .label(label)
            .child(SizedBox::new(100.0, 10.0))
            .boxed()
    };
    let content = move |composed| {
        let mut first = SliverList::separated(
            3,
            10.0,
            Rc::new(move |index| Some(row(format!("first {index}")))),
            Rc::new(|index| {
                Some(
                    Semantics::new()
                        .container(true)
                        .label(format!("separator {index}"))
                        .child(SizedBox::new(100.0, 10.0))
                        .boxed(),
                )
            }),
        );
        let mut second = SliverList::new(
            2,
            10.0,
            Rc::new(move |index| Some(row(format!("second {index}")))),
        );
        if composed {
            first = first.semantics(
                SemanticSetMapping::interleaved(
                    Rc::new(|index| index.is_multiple_of(2).then_some(index / 2)),
                    3,
                )
                .composed_at(0, flui_widgets::SetSize::Known(5)),
            );
            second = second.semantics(
                SemanticSetMapping::one_to_one(ItemCount::Exact(2))
                    .composed_at(3, flui_widgets::SetSize::Known(5)),
            );
        }
        Viewport::new((first, second))
    };
    let mut laid = lay_out(content(false), crate::common::tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let consumer = collection_consumer(&mut laid);
    for composed in [false, true, false] {
        laid.pump_widget(content(composed));
        let published = laid.a11y_tree().expect("semantics enabled");
        let tree = consumer.lock().expect("consumer lock");
        let member = |label: &str| {
            let id = published
                .find_by_label(label)
                .expect("member published")
                .id();
            let node = tree
                .state()
                .node_by_tree_local_id(id, TreeId::ROOT)
                .expect("member delivered");
            (
                node.position_in_set(),
                node.size_of_set_from_container(&common_filter),
            )
        };
        assert_eq!(
            member("first 2"),
            (Some(2), Some(if composed { 5 } else { 3 }))
        );
        assert_eq!(
            member("second 0"),
            (
                Some(if composed { 3 } else { 0 }),
                Some(if composed { 5 } else { 2 })
            )
        );
        assert_eq!(
            member("separator 0").0,
            None,
            "logical slots are not all members"
        );
    }
}

pub(crate) fn rich_text_accessible_labels_follow_span_overrides_and_replacement() {
    use flui_painting::typography::TextSpan;
    use flui_widgets::RichText;

    let content = |label: &str| {
        RichText::new(
            TextSpan::new("Dr.")
                .with_semantics_label(label)
                .with_child(TextSpan::new(" A.").with_semantics_label(" Alice")),
        )
    };
    let mut laid = lay_out(content("Doctor"), loose(300.0));
    laid.enable_semantics();
    laid.tick();
    let paragraph = laid.find_by_render_type("RenderParagraph");
    let original_size = laid.size(paragraph);
    for label in ["Doctor", "Professor", "", "Doctor"] {
        laid.pump_widget(content(label));
        let expected = format!("{label} Alice");
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert!(
            tree.find_by_label(&expected).is_ok(),
            "authored span labels must reach the published tree: {}",
            tree.describe()
        );
        assert!(tree.find_by_label("Dr. A.").is_err());
        assert_eq!(laid.find_by_render_type("RenderParagraph"), paragraph);
        assert_eq!(
            laid.size(paragraph),
            original_size,
            "labels do not change glyph geometry"
        );
    }
}

pub(crate) fn a_semantics_barrier_crosses_proxies_but_stops_at_its_boundary() {
    use flui_widgets::Column;

    let calls = Rc::new(Cell::new(0));
    let content = |blocking| {
        let calls = calls.clone();
        let label = |text| {
            Semantics::new()
                .container(true)
                .label(text)
                .child(SizedBox::new(20.0, 20.0))
        };
        Column::new((
            label("outside boundary"),
            Semantics::new().container(true).child(Column::new((
                Semantics::new()
                    .container(true)
                    .label("covered sibling")
                    .on_click(move |_| calls.set(calls.get() + 1))
                    .child(SizedBox::new(20.0, 20.0)),
                SizedBox::new(20.0, 20.0).child(
                    Semantics::new()
                        .block_previous_siblings(blocking)
                        .child(label("barrier child")),
                ),
                label("later sibling"),
            ))),
        ))
    };
    let mut laid = lay_out(content(false), loose(200.0));
    laid.enable_semantics();
    laid.tick();
    let covered = laid
        .a11y_tree()
        .expect("semantics enabled")
        .find_by_label("covered sibling")
        .expect("premise: background is published")
        .id();
    for blocking in [false, true, false, true] {
        laid.pump_widget(content(blocking));
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert_eq!(
            tree.find_by_label("covered sibling").is_ok(),
            !blocking,
            "{}",
            tree.describe()
        );
        for label in ["outside boundary", "barrier child", "later sibling"] {
            assert!(
                tree.find_by_label(label).is_ok(),
                "must retain {label}: {}",
                tree.describe()
            );
        }
        let before = calls.get();
        let result = laid.invoke_semantics_action(request(Action::Click, covered, None));
        assert_eq!(
            result.is_ok(),
            !blocking,
            "covered actions must lose authority"
        );
        assert_eq!(calls.get(), before + usize::from(!blocking));
    }
}

pub(crate) fn semantics_sort_keys_reorder_published_siblings_after_local_updates() {
    use flui_rendering::semantics::{SemanticsConfiguration, SemanticsSortKey};
    use flui_widgets::Row;

    let item = |label: &str, order: f64| {
        let mut config = SemanticsConfiguration::new();
        config.set_label(label);
        config.set_sort_key(SemanticsSortKey::new(order));
        Semantics::from_configuration(config)
            .container(true)
            .child(SizedBox::new(20.0, 20.0))
    };
    let content = |a, b| {
        Semantics::new()
            .container(true)
            .child(Row::new((item("A", a), item("B", b))))
    };
    let mut laid = lay_out(content(2.0, 1.0), loose(100.0));
    laid.enable_semantics();
    laid.tick();
    for (a, b, expected) in [
        (2.0, 1.0, ["B", "A"]),
        (0.0, 1.0, ["A", "B"]),
        (3.0, 1.0, ["B", "A"]),
        (1.0, 1.0, ["A", "B"]),
    ] {
        laid.pump_widget(content(a, b));
        let tree = laid.a11y_tree().expect("semantics enabled");
        let labels: Vec<_> = tree.nodes().filter_map(|node| node.label()).collect();
        assert_eq!(labels, expected, "published reading order for A={a}, B={b}");
    }
}

pub(crate) fn sliver_fade_keeps_geometry_and_retargets_paint_and_semantics() {
    use flui_animation::{Animation, AnimationController};
    use flui_widgets::{SliverFadeTransition, SliverToBoxAdapter, Viewport};
    use std::time::Duration;
    let first = Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
    let view = |opacity: Rc<dyn Animation<f64>>| {
        Viewport::new((SliverFadeTransition::new(
            opacity,
            SliverToBoxAdapter::new().child(
                Semantics::new()
                    .container(true)
                    .label("fading sliver")
                    .child(SizedBox::new(100.0, 50.0)),
            ),
        ),))
    };
    let mut laid = lay_out(view(first.clone()), crate::common::tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let nodes = laid.find_all_by_render_type("RenderSliverAnimatedOpacity");
    assert_eq!(nodes.len(), 1);
    let fade = nodes[0];
    let alpha = |laid: &crate::common::LaidOut| {
        laid.layer_tree().and_then(|tree| {
            tree.iter().find_map(|(_, node)| {
                if let flui_rendering::layer::Layer::Opacity(opacity) = node.layer() {
                    Some(opacity.alpha())
                } else {
                    None
                }
            })
        })
    };
    for value in [0.0, 0.5, 1.0, 0.0] {
        first.set_value(value);
        laid.tick();
        assert_eq!(
            laid.sliver_geometry(fade).scroll_extent,
            50.0,
            "fading must preserve scroll geometry"
        );
        assert_eq!(alpha(&laid), (value > 0.0 && value < 1.0).then_some(value));
        assert_eq!(
            laid.a11y_tree()
                .expect("semantics enabled")
                .find_by_label("fading sliver")
                .is_ok(),
            value > 0.0
        );
    }
    let second = Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
    second.set_value(0.25);
    let second_witness = Rc::downgrade(&second);
    laid.pump_widget(view(second.clone()));
    assert_eq!(
        laid.find_all_by_render_type("RenderSliverAnimatedOpacity"),
        [fade],
        "replacement preserves the render object"
    );
    assert_eq!(alpha(&laid), Some(0.25));
    first.set_value(0.75);
    laid.tick();
    assert_eq!(
        alpha(&laid),
        Some(0.25),
        "retired animation cannot update this presentation"
    );
    let first_witness = Rc::downgrade(&first);
    drop(first);
    assert!(
        first_witness.upgrade().is_none(),
        "replaced source ownership is released"
    );
    second.set_value(0.75);
    laid.tick();
    assert_eq!(alpha(&laid), Some(0.75));
    drop(laid);
    drop(second);
    assert!(
        second_witness.upgrade().is_none(),
        "unmounted fade releases its source without a proxy cycle"
    );
}

pub(crate) fn sliver_annotations_update_and_exclude_without_adapting_geometry() {
    use flui_widgets::{SliverSemantics, SliverToBoxAdapter, Viewport};
    let content = |label: &str, excluding| {
        Viewport::new((SliverSemantics::new()
            .container(true)
            .explicit_child_nodes(true)
            .label(label)
            .exclude_semantics(excluding)
            .child(
                SliverToBoxAdapter::new().child(
                    Semantics::new()
                        .container(true)
                        .label("section member")
                        .child(SizedBox::new(40.0, 30.0)),
                ),
            ),))
    };
    let mut laid = lay_out(
        content("section A", false),
        crate::common::tight(100.0, 100.0),
    );
    laid.enable_semantics();
    laid.tick();
    let annotation = laid.find_by_render_type("RenderSliverSemanticsAnnotations");
    for (label, excluding) in [
        ("section A", false),
        ("section B", true),
        ("section C", false),
    ] {
        laid.pump_widget(content(label, excluding));
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert!(tree.find_by_label(label).is_ok(), "{}", tree.describe());
        assert_eq!(tree.find_by_label("section member").is_ok(), !excluding);
        assert_eq!(
            laid.find_by_render_type("RenderSliverSemanticsAnnotations"),
            annotation
        );
        assert_eq!(laid.sliver_geometry(annotation).scroll_extent, 30.0);
    }
}

pub(crate) fn flow_repaint_and_delegate_replacement_update_accessible_children() {
    use flui_foundation::{
        ChangeNotifier, Listenable,
        geometry::{Matrix4, Size},
    };
    use flui_rendering::{
        constraints::BoxConstraints,
        delegates::{FlowDelegate, FlowPaintingContext},
    };
    use flui_widgets::Flow;
    use std::{any::Any, cell::Cell};

    #[derive(Debug)]
    struct Delegate {
        selected: Rc<Cell<usize>>,
        opacity: Rc<Cell<f64>>,
        notifier: Rc<ChangeNotifier>,
    }
    impl FlowDelegate for Delegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }
        fn get_constraints_for_child(
            &self,
            _index: usize,
            constraints: BoxConstraints,
        ) -> BoxConstraints {
            BoxConstraints::loose(constraints.biggest())
        }
        fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
            context.paint_child_with_opacity(
                self.selected.get(),
                Matrix4::IDENTITY,
                flui_rendering::traits::PaintOpacity::new(self.opacity.get()),
            );
        }
        fn should_relayout(&self, _old: &dyn FlowDelegate) -> bool {
            false
        }
        fn should_repaint(&self, _old: &dyn FlowDelegate) -> bool {
            true
        }
        fn repaint(&self) -> Option<Rc<dyn Listenable>> {
            Some(self.notifier.clone())
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    let selected = Rc::new(Cell::new(0));
    let opacity = Rc::new(Cell::new(1.0));
    let notifier = Rc::new(ChangeNotifier::new());
    let view = |delegate: Rc<dyn FlowDelegate>| {
        Flow::new(
            delegate,
            (
                Semantics::new()
                    .container(true)
                    .label("first flow child")
                    .child(SizedBox::new(20.0, 20.0)),
                Semantics::new()
                    .container(true)
                    .label("second flow child")
                    .child(SizedBox::new(20.0, 20.0)),
            ),
        )
    };
    let mut laid = lay_out(
        view(Rc::new(Delegate {
            selected: selected.clone(),
            opacity: opacity.clone(),
            notifier: notifier.clone(),
        })),
        loose(100.0),
    );
    laid.enable_semantics();
    laid.tick();
    for index in [0, 1, 0] {
        selected.set(index);
        notifier.notify_listeners();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert_eq!(tree.find_by_label("first flow child").is_ok(), index == 0);
        assert_eq!(tree.find_by_label("second flow child").is_ok(), index == 1);
    }
    for alpha in [0.0, 0.234_567_89, 1.0] {
        opacity.set(alpha);
        notifier.notify_listeners();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert_eq!(tree.find_by_label("first flow child").is_ok(), alpha > 0.0);
        assert!(tree.find_by_label("second flow child").is_err());
    }
    laid.pump_widget(view(Rc::new(Delegate {
        selected: Rc::new(Cell::new(1)),
        opacity: Rc::new(Cell::new(1.0)),
        notifier: Rc::new(ChangeNotifier::new()),
    })));
    assert_eq!(
        notifier.len(),
        0,
        "replacement cancels the old repaint source"
    );
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(tree.find_by_label("first flow child").is_err());
    assert!(tree.find_by_label("second flow child").is_ok());
}

pub(crate) fn custom_painter_regions_follow_semantics_only_updates_and_layout_size() {
    use flui_foundation::geometry::{Point, Rect, Size};
    use flui_painting::Canvas;
    use flui_rendering::semantics::{SemanticsConfiguration, SemanticsRegion, SemanticsRegionKey};
    use flui_widgets::{CustomPaint, CustomPainter};

    #[derive(Debug)]
    struct Painter {
        label: &'static str,
        rebuild_semantics: bool,
    }
    impl CustomPainter for Painter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}
        fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
            false
        }
        fn should_rebuild_semantics(&self, _old: &dyn CustomPainter) -> bool {
            self.rebuild_semantics
        }
        fn semantics_regions(&self, size: Size) -> Vec<SemanticsRegion> {
            let mut config = SemanticsConfiguration::new();
            config.set_label(self.label);
            vec![SemanticsRegion::new(
                SemanticsRegionKey(1),
                Rect::from_origin_size(Point::ZERO, Size::new(size.width / 2.0, size.height)),
                config,
            )]
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let view = |width, label, rebuild_semantics| {
        CustomPaint::new()
            .size(Size::new(width, 20.0))
            .painter(Rc::new(Painter {
                label,
                rebuild_semantics,
            }))
    };
    let mut laid = lay_out(view(40.0, "first", true), loose(200.0));
    laid.enable_semantics();
    laid.tick();
    let identity = laid
        .a11y_tree()
        .unwrap()
        .find_by_label("first")
        .unwrap()
        .id();
    for (width, label, rebuild_semantics) in [(40.0, "relabelled", true), (60.0, "resized", false)]
    {
        laid.pump_widget(view(width, label, rebuild_semantics));
        let tree = laid.a11y_tree().unwrap();
        let node = tree
            .find_by_label(label)
            .expect("current painter contributes accessible content");
        assert_eq!(
            node.id(),
            identity,
            "same host/key retains identity across widget updates"
        );
        let bounds = node.bounds().expect("region has committed bounds");
        assert_eq!(
            (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0),
            (width / 2.0, 20.0)
        );
    }
}

pub(crate) fn retained_visibility_hides_child_semantics_by_default() {
    use flui_widgets::Visibility;

    for (visible, maintain_semantics, announced) in [
        (true, false, true),
        (false, false, false),
        (false, true, true),
        (true, true, true),
    ] {
        let mut laid = lay_out(
            Visibility::new(
                Semantics::new()
                    .container(true)
                    .label("retained visibility child")
                    .child(SizedBox::new(40.0, 20.0)),
            )
            .visible(visible)
            .maintain_state(true)
            .maintain_animation(true)
            .maintain_size(true)
            .maintain_semantics(maintain_semantics),
            loose(200.0),
        );
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert_eq!(
            tree.find_by_label("retained visibility child").is_ok(),
            announced,
            "visible={visible}, maintain_semantics={maintain_semantics}: {}",
            tree.describe()
        );
    }
}

pub(crate) fn retained_visibility_updates_semantics_without_changing_layout() {
    use flui_widgets::Visibility;

    let view = |visible, maintain_semantics| {
        Visibility::new(
            Semantics::new()
                .container(true)
                .label("retained visibility child")
                .child(SizedBox::new(40.0, 20.0)),
        )
        .visible(visible)
        .maintain_state(true)
        .maintain_animation(true)
        .maintain_size(true)
        .maintain_semantics(maintain_semantics)
    };
    let mut laid = lay_out(view(true, false), loose(200.0));
    laid.enable_semantics();
    laid.tick();
    for (visible, maintain_semantics, announced) in [
        (true, false, true),
        (false, false, false),
        (false, true, true),
        (false, false, false),
        (true, true, true),
        (false, true, true),
        (true, false, true),
        (false, false, false),
    ] {
        laid.pump_widget(view(visible, maintain_semantics));
        let tree = laid.a11y_tree().expect("semantics enabled");
        assert_eq!(
            tree.find_by_label("retained visibility child").is_ok(),
            announced,
            "visible={visible}, maintain_semantics={maintain_semantics}: {}",
            tree.describe()
        );
        assert_eq!(laid.size(laid.root()), crate::common::size(40.0, 20.0));
    }
}

// ===========================================================================
// RenderParagraph — plain text publishes its own label
// ===========================================================================

/// Merging collapses its descendants in the **accessibility tree**, not just in
/// the render tree.
///
/// The sibling tests above assert that `RenderMergeSemantics` mounts. That
/// proves the render object exists; it says nothing about what a screen reader
/// is handed, and the two are separated by the whole assemble-and-translate
/// path. This asserts the end product.
///
/// The premise runs first and is load-bearing: without the wrapper both labels
/// must be separately findable. A test that only checked "the children are not
/// findable" would pass just as well against a tree where they never
/// contributed semantics at all — which is the ordinary state of affairs for a
/// bare `SizedBox`, and why the children here carry real `Semantics`.
pub(crate) fn merge_semantics_collapses_its_descendants_in_the_a11y_tree() {
    use flui_view::ViewExt as _;
    use flui_widgets::Column;

    let labelled = |text: &'static str| {
        Semantics::new()
            .container(true)
            .label(text)
            .child(SizedBox::new(40.0, 20.0))
    };

    // Premise: unmerged, each child is its own node.
    let mut apart = lay_out(
        Column::new((labelled("first").boxed(), labelled("second").boxed())),
        loose(200.0),
    );
    apart.enable_semantics();
    apart.pump();
    let apart_tree = apart.a11y_tree().expect("semantics enabled");
    assert!(
        apart_tree.find_by_label("first").is_ok() && apart_tree.find_by_label("second").is_ok(),
        "premise: without merging, both labels are separately findable — \
         otherwise the assertion below proves nothing. Tree was:\n{}",
        apart_tree.describe()
    );

    let mut merged = lay_out(
        MergeSemantics::new().child(Column::new((
            labelled("first").boxed(),
            labelled("second").boxed(),
        ))),
        loose(200.0),
    );
    merged.enable_semantics();
    merged.pump();
    let merged_tree = merged.a11y_tree().expect("semantics enabled");

    // The combined label is the assertion that distinguishes merging from
    // DESTROYING. "neither label is findable" plus "fewer nodes" would both
    // hold if the merge path dropped its descendants outright, or kept only
    // one of them — which is exclusion's contract, not merging's.
    assert!(
        merged_tree.find_by_label("first second").is_ok(),
        "merging must carry its descendants' labels into one node, combined \
         and in order. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.find_by_label("first").is_err() && merged_tree.find_by_label("second").is_err(),
        "and neither descendant may remain addressable on its own; a reader \
         should meet one node, not three. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.len() < apart_tree.len(),
        "the merged tree must have strictly fewer nodes than the unmerged one \
         ({} vs {})",
        merged_tree.len(),
        apart_tree.len()
    );
}

// ===========================================================================
// Actions: a platform request, routed back to the widget's own callback
// ===========================================================================

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_rendering::semantics::{AccessibilityNodeId, SemanticsActionRequest};
use flui_testing::widgets::LaidOut;
use flui_testing::{Action, ActionData, ActionRequest, NodeId, TreeId};
use flui_view::{SignalWriteExt as _, View};

use crate::common::{ProbeSignals, SignalProbe};

/// Retained overlay state remains live while its page is structurally absent
/// from both assembled semantics and the actual delivered update stream.
pub(crate) fn a_covered_retained_form_stays_absent_after_a_late_controller_update() {
    use flui_interaction::FocusNode;
    use flui_testing::A11yTree;
    use flui_testing::a11y::Role;
    use flui_view::ViewExt as _;
    use flui_widgets::{
        Column, Form, InsertPosition, Overlay, OverlayEntry, OverlayHandle, RawTextFormField, Text,
        TextEditingController,
    };
    use std::sync::{Arc, Mutex};

    let controller = TextEditingController::with_text("retained draft");
    let focus = FocusNode::new();
    let calls = Rc::new(Cell::new(0_u32));
    let lower_controller = controller.clone();
    let lower_focus = Rc::clone(&focus);
    let lower_calls = Rc::clone(&calls);
    let lower = OverlayEntry::new(move |_cx| {
        let calls = Rc::clone(&lower_calls);
        Form::new(Column::new((
            Text::new("Retained page").boxed(),
            RawTextFormField::new(lower_controller.clone())
                .focus_node(Rc::clone(&lower_focus))
                .boxed(),
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .label("Back")
                .on_click(move |_cx| calls.set(calls.get() + 1))
                .child(SizedBox::new(80.0, 40.0))
                .boxed(),
        )))
        .boxed()
    });
    lower.set_maintain_state(true);
    let upper = OverlayEntry::new(move |_cx| {
        Column::new((
            Semantics::new()
                .container(true)
                .label("Active page")
                .child(SizedBox::new(80.0, 30.0))
                .boxed(),
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .label("Back")
                .on_click(|_cx| {})
                .child(SizedBox::new(80.0, 40.0))
                .boxed(),
        ))
        .boxed()
    });
    upper.set_opaque(true);
    let overlay = OverlayHandle::new();
    overlay.insert(&lower, &InsertPosition::Top);
    let mut laid = lay_out(Overlay::new(overlay.clone()), loose(300.0));
    laid.enable_semantics();
    laid.tick();
    let delivered = Arc::new(Mutex::new(
        laid.a11y_tree().expect("semantics enabled").raw().clone(),
    ));
    let sink = Arc::clone(&delivered);
    laid.pipeline_owner().with_mut(|owner| {
        owner.set_semantics_update_callback(Arc::new(move |update| {
            let ids: std::collections::HashSet<_> =
                update.nodes.iter().map(|(id, _)| *id).collect();
            assert_eq!(
                ids.len(),
                update.nodes.len(),
                "published packet repeats an identity"
            );
            let mut current = sink.lock().expect("publication mirror lock");
            if let Some(tree) = &update.tree {
                current.tree = Some(tree.clone());
            }
            for (id, node) in &update.nodes {
                if let Some((_, previous)) = current.nodes.iter_mut().find(|(key, _)| key == id) {
                    previous.clone_from(node);
                } else {
                    current.nodes.push((*id, node.clone()));
                }
            }
            current.focus = update.focus;
            current.tree_id = update.tree_id;
        }));
    });
    let published = || A11yTree::new(delivered.lock().expect("publication mirror lock").clone());
    let before = published();
    let field = before
        .find(Role::TextInput)
        .expect("premise: lower Edit published");
    assert_eq!(field.value(), Some("retained draft"));
    assert_eq!(
        before.find_all(Role::Form).len(),
        1,
        "premise: lower Form published"
    );
    let lower_back = before
        .find_by_label("Back")
        .expect("premise: lower Back published")
        .id();
    laid.invoke_semantics_action(request(Action::Click, lower_back, None))
        .expect("lower action initially works");
    assert_eq!(calls.get(), 1);
    let bounds = field.bounds().expect("lower editor laid out");
    laid.dispatch_pointer_down(bounds.x0 + 2.0, f64::midpoint(bounds.y0, bounds.y1));
    laid.dispatch_pointer_up(bounds.x0 + 2.0, f64::midpoint(bounds.y0, bounds.y1));
    laid.tick();
    assert!(
        focus.has_primary_focus(),
        "premise: retained editor really focused"
    );

    overlay.insert(&upper, &InsertPosition::Top);
    laid.tick();
    let covered = |laid: &LaidOut| {
        for (source, tree) in [
            ("assembled", laid.a11y_tree().expect("semantics enabled")),
            ("delivered", published()),
        ] {
            assert!(
                tree.find_by_label("Active page").is_ok(),
                "{source}: active page published"
            );
            let back = tree
                .find_by_label("Back")
                .expect("one active Back, no covered duplicate");
            assert_ne!(
                back.id(),
                lower_back,
                "{source}: Back belongs to active page"
            );
            assert!(
                tree.find_all(Role::Form).is_empty(),
                "{source}: covered Form disconnected"
            );
            assert!(
                tree.find_all(Role::TextInput).is_empty(),
                "{source}: covered Edit disconnected"
            );
        }
    };
    covered(&laid);
    assert!(
        laid.invoke_semantics_action(request(Action::Click, lower_back, None))
            .is_err(),
        "covered lower action refused even though its element is retained"
    );
    // This is a real public controller notification to the retained producer,
    // not input evidence or a forced root rebuild. Its old rect must not
    // authorize republishing the covered Form's cached semantic anchor.
    controller.set_text("late retained draft");
    for _ in 0..3 {
        laid.tick();
    }
    covered(&laid);
    assert_eq!(calls.get(), 1, "refused action had no callback effect");
    upper.remove();
    laid.tick();
    for tree in [laid.a11y_tree().expect("semantics enabled"), published()] {
        assert!(
            tree.find_all_by_label("Active page").is_empty(),
            "removed upper page disconnected"
        );
        assert_eq!(
            tree.find(Role::TextInput).expect("restored Edit").value(),
            Some("late retained draft")
        );
        assert_eq!(tree.find_all(Role::Form).len(), 1);
        assert_eq!(
            tree.find_by_label("Back")
                .expect("restored lower Back")
                .id(),
            lower_back
        );
    }
    laid.invoke_semantics_action(request(Action::Click, lower_back, None))
        .expect("restored lower action works");
    assert_eq!(calls.get(), 2);
}

/// Mounts `root` with semantics enabled and returns the single node carrying
/// `label`, together with a live view of the a11y tree.
///
/// The tree is returned alongside its own description so a failing assertion
/// can show what was actually there, matching this file's other a11y tests.
fn pump_labelled(root: impl View) -> (LaidOut, flui_testing::A11yTree, NodeId) {
    let mut laid = lay_out(root, loose(200.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node_id = labelled_node(&tree);
    (laid, tree, node_id)
}

/// The node carrying [`LABEL`] in `tree`.
fn labelled_node(tree: &flui_testing::A11yTree) -> NodeId {
    tree.find_by_label(LABEL)
        .unwrap_or_else(|error| {
            panic!(
                "expected one node labelled {LABEL:?}: {error}\n{}",
                tree.describe()
            )
        })
        .id()
}

/// A container node labelled [`LABEL`] around a fixed-size box, for the
/// action builders to decorate.
fn host() -> Semantics {
    Semantics::new().container(true).label(LABEL)
}

/// The label every action test mounts under; unique within its own tree.
const LABEL: &str = "Action Host";

/// Builds an `ActionRequest` addressed at `node_id`.
fn request(action: Action, node_id: NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node_id,
        data,
    }
}

/// A tap handler reaches its callback through a platform click request.
///
/// Two halves, and both are necessary: the node must *tell* the platform the
/// action exists, and pressing it must *do* something. A node that passes only
/// the first is the dead control this whole surface exists to rule out — an
/// action advertised outbound that nothing routes inbound.
pub(crate) fn a_tap_handler_round_trips_from_a_platform_click_to_the_callback() {
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);

    let (laid, tree, node_id) = pump_labelled(
        host()
            .on_click(move |_cx| counted.set(counted.get() + 1))
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("node was located a moment ago")
            .supports_action(Action::Click),
        "a node with an on_tap handler must advertise a click, or no assistive \
         technology can reach it. Tree was:\n{}",
        tree.describe()
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("a click on a node advertising one must resolve");

    assert_eq!(
        activations.get(),
        1,
        "the handler must have run exactly once"
    );
}

/// An action handler receives the dispatch's `EventCx`: its signal write
/// lands, and the signal's reader rebuilds on the next frame.
pub(crate) fn an_action_handler_writes_a_signal_and_rebuilds_its_reader() {
    let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
        host()
            .on_increment(move |cx| count.update(cx, |n| *n += 1))
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _tree, node_id) = pump_labelled(probe.view());

    laid.invoke_semantics_action(request(Action::Increment, node_id, None))
        .expect("an increment on a node advertising one must resolve");
    assert_eq!(probe.value(), Ok(1), "the handler wrote through its cx");

    laid.tick();
    assert_eq!(
        probe.reads().last(),
        Some(&1),
        "the reader rebuilt with the write"
    );
}

/// A handler whose write is refused (its signal's slot is released) is
/// reported at the dispatch boundary, and the next action still runs.
pub(crate) fn a_refused_write_in_an_action_handler_is_reported_not_panicked() {
    let probe = SignalProbe::new(|ProbeSignals { count, released }| {
        host()
            .on_decrement(move |cx| released.set(cx, 1))
            .on_increment(move |cx| count.set(cx, 1))
            .child(SizedBox::new(40.0, 20.0))
    });
    let (laid, _tree, node_id) = pump_labelled(probe.view());

    let (outcome, log) = flui_testing::log_capture::capture(|| {
        laid.invoke_semantics_action(request(Action::Decrement, node_id, None))
    });
    outcome.expect("a decrement on a node advertising one must resolve");
    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );

    laid.invoke_semantics_action(request(Action::Increment, node_id, None))
        .expect("the node still resolves after a refused write");
    assert_eq!(probe.value(), Ok(1), "the next action still wrote");
}

/// A handler is owner-local, so an action invoked with no UI runtime entered — a
/// caller holding a `SemanticsActionInvocation` outside the owner's
/// dispatch — has nowhere to run it: the action is dropped with a warning,
/// and nothing panics.
pub(crate) fn an_action_invoked_outside_its_ui_runtime_is_dropped_with_a_warning() {
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);
    let (laid, _tree, node_id) = pump_labelled(
        host()
            .on_click(move |_cx| counted.set(counted.get() + 1))
            .child(SizedBox::new(40.0, 20.0)),
    );

    let invocation = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node_id.0)
                    .expect("an exported node id is non-zero"),
                call: ActionCall::Click,
            })
        })
        .expect("the node advertises a tap");
    let ((), log) = flui_testing::log_capture::capture(|| invocation.invoke());

    assert_eq!(activations.get(), 0, "the handler did not run");
    assert!(
        log.contains("a semantics action was dropped"),
        "the drop is reported: {log}"
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("the same node resolves inside its ui_runtime");
    assert_eq!(
        activations.get(),
        1,
        "inside the ui_runtime the handler runs"
    );
}

/// A rebuild that hands the node a fresh closure — the ordinary case, a
/// closure literal in `build` — leaves the mounted configuration equal, so it
/// raises no semantics update, and the action runs the rebuilt closure rather
/// than the first one.
pub(crate) fn rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one() {
    let ran = Rc::new(Cell::new(0_u32));
    let tree_for = |version: u32| {
        let ran = Rc::clone(&ran);
        host()
            .on_click(move |_cx| ran.set(version))
            .child(SizedBox::new(40.0, 20.0))
    };
    let (mut laid, _tree, node_id) = pump_labelled(tree_for(1));
    let before = mounted_configuration(&laid);

    laid.pump_widget(tree_for(2));
    let after = mounted_configuration(&laid);
    assert!(
        before == after,
        "a rebuild that changes only the closure must keep the configuration \
         equal, or every rebuild re-assembles this node's semantics"
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("the rebuilt node still resolves a click");
    assert_eq!(ran.get(), 2, "the rebuilt closure ran, not the first one");
}

/// Unmounting a node releases its action table from the owner lane, so the
/// state a handler captures is dropped with the node rather than kept until
/// the UI runtime closes. The action is delivered through `Harness`, whose UI runtime
/// entry is what lets the handler run at all.
pub(crate) fn unmounting_a_node_releases_its_action_table() {
    let captured = Rc::new(());
    let witness = Rc::downgrade(&captured);
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);
    let mut harness = crate::common::harness::mount(
        host()
            .on_click(move |_cx| {
                std::hint::black_box(&captured);
                counted.set(counted.get() + 1);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );
    harness.enable_semantics();
    harness.tick();
    let tree = harness
        .a11y_tree()
        .expect("semantics enabled before the frame");
    harness
        .invoke_semantics_action(request(Action::Click, labelled_node(&tree), None))
        .expect("a click on a node advertising one must resolve");
    assert_eq!(
        activations.get(),
        1,
        "the handler ran inside the ui_runtime"
    );

    harness.swap_root(SizedBox::shrink());
    assert!(
        witness.upgrade().is_none(),
        "the unmounted node's handler, and what it captured, must leave the \
         owner lane with the node"
    );
}

/// A node mounted with no owner advertises none of its actions: nothing
/// could run them, and an advertised action nothing runs is a dead control.
pub(crate) fn a_detached_mount_advertises_no_actions() {
    use flui_view::RenderView as _;

    let render = host()
        .on_click(|_cx| {})
        .create_render_object(&flui_view::RenderObjectContext::detached());
    let configuration = render.configuration();
    assert!(
        configuration.label().is_some(),
        "the detached node still carries its own configuration"
    );
    assert!(
        !configuration.has_action(SemanticsAction::Click),
        "a detached node must not advertise a tap it cannot run"
    );
}

/// The configuration the single `Semantics` wrapper has mounted.
fn mounted_configuration(laid: &LaidOut) -> flui_rendering::semantics::SemanticsConfiguration {
    let [id] = laid.find_semantics_wrappers()[..] else {
        panic!("one Semantics wrapper is mounted");
    };
    laid.pipeline_owner().with_mut(|owner| {
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| {
                node.downcast_render_object_mut::<flui_objects::RenderSemanticsAnnotations>()
            })
            .map(|render| render.configuration().clone())
            .expect("the wrapper is a RenderSemanticsAnnotations")
    })
}

/// A set-value request that arrives without its text is refused, not emptied.
///
/// `""` is a thing a platform can legitimately mean — clear the field — so
/// synthesizing one for a request that lost its payload would turn a failure to
/// route into a silent erasure of whatever the field held. The request is
/// refused while it is decoded, before it reaches the node.
pub(crate) fn a_set_value_request_without_its_text_is_refused_rather_than_emptied() {
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);

    let (laid, _tree, node_id) = pump_labelled(
        host()
            .on_set_value(move |_cx, text| sink.borrow_mut().push(text.to_owned()))
            .child(SizedBox::new(40.0, 20.0)),
    );

    let refused = laid.invoke_semantics_action(request(Action::SetValue, node_id, None));
    assert!(
        matches!(
            refused,
            Err(flui_testing::a11y::InvokeActionError::Decode(
                ActionDecodeError::MissingPayload { .. }
            ))
        ),
        "{refused:?}"
    );
    assert!(
        seen.borrow().is_empty(),
        "the handler must not run: an empty string is an edit the platform \
         never asked for",
    );
}

/// Platform actions preserve direction and exact values across queued delivery.
/// The signal write forces the real frame producer to republish the result.
pub(crate) fn queued_directional_actions_and_numeric_values_reach_the_frame_producer() {
    use flui_rendering::semantics::NumericRange;

    let state = Rc::new(Cell::new(false));
    let described = Rc::clone(&state);
    let expanded = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let expand = Rc::clone(&described);
        let collapse = Rc::clone(&described);
        let toggle = Rc::clone(&described);
        host()
            .expandable(
                described.get(),
                move |cx| {
                    expand.set(true);
                    count.set(cx, 1)
                },
                move |cx| {
                    collapse.set(false);
                    count.set(cx, 0)
                },
            )
            .on_click(move |cx| {
                let next = !toggle.get();
                toggle.set(next);
                count.set(cx, u32::from(next))
            })
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _, node_id) = pump_labelled(expanded.view());
    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(request(Action::Expand, node_id, None));
    listener(request(Action::Expand, node_id, None));
    laid.tick();
    assert_eq!(expanded.value(), Ok(1), "two expands must not toggle twice");
    let tree = laid.a11y_tree().expect("published expanded frame");
    let node = tree.find_by_label(LABEL).expect("control remains live");
    assert_eq!(node.raw().is_expanded(), Some(true));
    assert!(node.supports_action(Action::Collapse));
    assert!(!node.supports_action(Action::Expand));
    listener(request(Action::Collapse, node_id, None));
    listener(request(Action::Collapse, node_id, None));
    laid.tick();
    assert_eq!(expanded.value(), Ok(0), "two collapses remain collapsed");
    listener(request(Action::Click, node_id, None));
    listener(request(Action::Expand, node_id, None));
    laid.tick();
    assert_eq!(
        expanded.value(),
        Ok(1),
        "a pending pointer toggle cannot reverse an explicit expand"
    );

    let value = Rc::new(Cell::new(0.0_f64));
    let current = Rc::clone(&value);
    let numeric = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let changed = Rc::clone(&current);
        host()
            .numeric_range(
                NumericRange::new(current.get(), 0.0, 10.0, 1.0).expect("finite fixture"),
            )
            .on_set_numeric_value(move |cx, exact| {
                changed.set(exact);
                count.update(cx, |n| *n += 1)
            })
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _, node_id) = pump_labelled(numeric.view());
    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(2.375)),
    ));
    laid.tick();
    assert_eq!(
        value.get(),
        2.375,
        "the step must not round the requested value"
    );
    assert_eq!(numeric.value(), Ok(1));
    let tree = laid.a11y_tree().expect("numeric frame republished");
    let node = tree
        .find_by_label(LABEL)
        .expect("range control remains live");
    assert_eq!(node.raw().numeric_value(), Some(2.375));
    assert_eq!(node.raw().min_numeric_value(), Some(0.0));
    assert_eq!(node.raw().max_numeric_value(), Some(10.0));
    assert_eq!(node.raw().numeric_value_step(), Some(1.0));
    for rejected in [f64::NAN, f64::INFINITY, -0.5, 10.5] {
        listener(request(
            Action::SetValue,
            node_id,
            Some(ActionData::NumericValue(rejected)),
        ));
        laid.tick();
        assert_eq!(
            value.get(),
            2.375,
            "invalid numeric payload reached the callback"
        );
        assert_eq!(
            numeric.value(),
            Ok(1),
            "rejected request must not perform a signal write"
        );
    }
    listener(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(10.0)),
    ));
    laid.tick();
    assert_eq!(
        value.get(),
        10.0,
        "the next valid request survives rejected values"
    );
    assert_eq!(numeric.value(), Ok(2));
}

/// Native range metadata cannot admit non-finite values or reversed bounds.
pub(crate) fn numeric_range_admission_and_owner_payload_validation() {
    use flui_rendering::semantics::{ActionCall, NumericRange, NumericRangeError};
    for (value, min, max, step, error) in [
        (f64::NAN, 0.0, 1.0, 1.0, NumericRangeError::NonFinite),
        (
            0.0,
            f64::NEG_INFINITY,
            1.0,
            1.0,
            NumericRangeError::NonFinite,
        ),
        (0.0, 1.0, 0.0, 1.0, NumericRangeError::ReversedBounds),
        (2.0, 0.0, 1.0, 1.0, NumericRangeError::ValueOutOfRange),
        (0.0, 0.0, 1.0, 0.0, NumericRangeError::NonPositiveStep),
    ] {
        assert_eq!(NumericRange::new(value, min, max, step), Err(error));
    }
    let count = Rc::new(Cell::new(0));
    let changed = Rc::clone(&count);
    let (laid, tree, node_id) = pump_labelled(
        host()
            .numeric_range(NumericRange::new(4.0, 4.0, 4.0, 1.0).expect("zero-span range is valid"))
            .on_set_numeric_value(move |_cx, value| {
                assert_eq!(value, 4.0);
                changed.set(changed.get() + 1);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );
    assert!(
        tree.find_by_label(LABEL)
            .expect("range exists")
            .supports_action(Action::SetValue)
    );
    for call in [
        ActionCall::SetNumericValue(4.1),
        ActionCall::SetValue("4.1".into()),
        ActionCall::SetValue("four".into()),
    ] {
        let refused = laid.pipeline_owner().with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest::new(
                AccessibilityNodeId::from_u64(node_id.0).expect("published identity"),
                call,
            ))
        });
        assert!(refused.is_err(), "malformed numeric request was admitted");
    }
    assert_eq!(count.get(), 0);
    laid.invoke_semantics_action(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(4.0)),
    ))
    .expect("exact zero-span value is admitted");
    assert_eq!(count.get(), 1);
}

/// A horizontal scrollable's item step is a character, as AccessKit defines
/// it for horizontal scrolling: 16 by default, not the vertical line.
pub(crate) fn a_horizontal_item_scroll_moves_one_character() {
    use flui_widgets::{ScrollController, Scrollable};

    let scroll = ScrollController::new();
    let (mut laid, tree, _host) = pump_labelled(
        host().child(
            Scrollable::new()
                .scroll_direction(flui_foundation::geometry::Axis::Horizontal)
                .controller(scroll.clone())
                .child(SizedBox::new(1000.0, 40.0)),
        ),
    );
    let scroller = tree
        .nodes()
        .find(|node| node.supports_action(Action::ScrollRight))
        .unwrap_or_else(|| {
            panic!(
                "a horizontal scrollable scrolls right:\n{}",
                tree.describe()
            )
        })
        .id();
    laid.invoke_semantics_action(request(
        Action::ScrollRight,
        scroller,
        Some(flui_testing::a11y::ActionData::ScrollUnit(
            flui_testing::a11y::ScrollUnit::Item,
        )),
    ))
    .expect("an item scroll-right resolves");
    laid.tick();
    assert_eq!(scroll.pixels(), 16.0, "an item step moves one character");
}

/// A screen reader scrolls a list through the same position a drag drives:
/// the scrollable advertises scroll-down/up, and each page request moves the
/// offset by most of a viewport and each item request by one line, clamped to
/// the extents.
pub(crate) fn assistive_scroll_actions_move_a_scrollable() {
    use flui_widgets::{ScrollController, Scrollable};

    let scroll = ScrollController::new();
    scroll
        .update_dimensions(100.0, 0.0, 900.0)
        .expect("ordered extents");
    let (mut laid, tree, _host) = pump_labelled(
        host().child(
            Scrollable::new()
                .controller(scroll.clone())
                .child(SizedBox::new(100.0, 1000.0)),
        ),
    );
    let scroller = tree
        .nodes()
        .find(|node| node.supports_action(Action::ScrollDown))
        .unwrap_or_else(|| {
            panic!(
                "a scrollable must advertise scroll-down to assistive technology. \
                 Tree was:\n{}",
                tree.describe()
            )
        });
    assert!(
        !scroller.supports_action(Action::ScrollUp),
        "blocked direction is omitted"
    );
    assert_eq!(scroller.raw().role(), flui_testing::a11y::Role::ScrollView);
    let scroller = scroller.id();

    laid.invoke_semantics_action(request(Action::ScrollDown, scroller, None))
        .expect("scroll-down resolves");
    laid.tick();
    let after_down = scroll.pixels();
    assert!(
        after_down > 0.0,
        "scroll-down moved the list, got {after_down}"
    );

    laid.invoke_semantics_action(request(Action::ScrollUp, scroller, None))
        .expect("scroll-up resolves");
    laid.tick();
    assert_eq!(scroll.pixels(), 0.0, "scroll-up returns to the start");

    // An item step moves one wheel line (53 by default), not a page.
    laid.invoke_semantics_action(request(
        Action::ScrollDown,
        scroller,
        Some(flui_testing::a11y::ActionData::ScrollUnit(
            flui_testing::a11y::ScrollUnit::Item,
        )),
    ))
    .expect("an item scroll-down resolves");
    laid.tick();
    assert!(
        after_down > 53.0,
        "the page step exceeds a line: {after_down}"
    );
    assert_eq!(scroll.pixels(), 53.0, "an item step moves one line");
    laid.invoke_semantics_action(request(Action::ScrollUp, scroller, None))
        .expect("scroll-up resolves");
    laid.tick();
    let adjustment = laid
        .a11y_tree()
        .expect("tree")
        .nodes()
        .find(|node| node.supports_action(Action::SetValue) && node.raw().numeric_value().is_some())
        .expect("native scroll adjustment")
        .id();
    laid.invoke_semantics_action(request(
        Action::SetValue,
        adjustment,
        Some(flui_testing::a11y::ActionData::NumericValue(160.0)),
    ))
    .expect("native range action resolves on the concrete scrollable");
    laid.tick();
    assert_eq!(scroll.pixels(), 160.0);
    let republished = laid.a11y_tree().expect("live tree");
    let republished = republished
        .nodes()
        .find(|node| node.id() == scroller)
        .expect("same scroll node");
    assert!(republished.supports_action(Action::ScrollUp));
    let published = laid.a11y_tree().expect("tree");
    assert_eq!(
        published
            .nodes()
            .find(|node| node.id() == adjustment)
            .expect("adjuster")
            .raw()
            .numeric_value(),
        Some(160.0)
    );
}

// ===========================================================================
// Published bounds are physical pixels
// ===========================================================================

/// Running slides publish their current bounds, including unchanged
/// transformed ancestors reconstructed for a partial semantics pass.
pub(crate) fn animated_slide_publishes_bounds_at_its_current_sample() {
    use flui_animation::Animatable;
    use flui_animation::{Animation, AnimationController, Tween, Vsync};
    use flui_foundation::geometry::Matrix4;
    use flui_objects::TranslationFraction;
    use flui_painting::Alignment;
    use flui_painting::typography::TextDirection;
    use flui_view::ViewExt;
    use flui_widgets::{Align, SlideTransition, Transform, VsyncScope};
    use std::rc::Rc;
    use std::time::Duration;

    for (direction, anchored) in [
        (TextDirection::Ltr, false),
        (TextDirection::Rtl, false),
        (TextDirection::Ltr, true),
        (TextDirection::Rtl, true),
    ] {
        let vsync = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
        let scalar: Rc<dyn Animation<f64>> = Rc::new(owner.controller().clone());
        let position = Rc::new(
            Tween::new(
                TranslationFraction::ZERO,
                TranslationFraction::new(2.0, 1.0),
            )
            .animate(scalar),
        );
        let slide = SlideTransition::new(
            position,
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .label("Moving control")
                .child(SizedBox::new(40.0, 40.0)),
        )
        .text_direction(direction);
        // The nested boundary is the partial pass's anchor. Its unchanged
        // ancestors still contribute translation and scale to every graft.
        let (content, x, y, sx, sy) = if anchored {
            (
                Transform::new(
                    Matrix4::translation(10.0, 5.0, 0.0) * Matrix4::scaling(1.25, 1.5, 1.0),
                )
                .child(
                    Semantics::new()
                        .container(true)
                        .label("Motion group")
                        .child(slide),
                )
                .boxed(),
                90.0,
                85.0,
                1.25,
                1.5,
            )
        } else {
            (slide.boxed(), 80.0, 80.0, 1.0, 1.0)
        };
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), Align::new(Alignment::CENTER).child(content)),
            crate::common::tight(200.0, 200.0),
            vsync,
        );
        laid.enable_semantics();
        laid.tick();
        assert_rect_near(
            effective_bounds(
                &laid.a11y_tree().expect("semantics enabled"),
                "Moving control",
            ),
            (x, y, x + 40.0 * sx, y + 40.0 * sy),
            "initial control",
        );
        owner.controller().forward().expect("fresh slide run");
        laid.tick();
        for elapsed in [0.25, 0.5] {
            laid.pump_for(Duration::from_millis(250));
            assert!((owner.controller().value() - elapsed).abs() < 1e-9);
            let dx = sx
                * 80.0
                * elapsed
                * if direction == TextDirection::Rtl {
                    -1.0
                } else {
                    1.0
                };
            let dy = sy * 40.0 * elapsed;
            assert_rect_near(
                effective_bounds(
                    &laid.a11y_tree().expect("moving semantics"),
                    "Moving control",
                ),
                (x + dx, y + dy, x + 40.0 * sx + dx, y + 40.0 * sy + dy),
                "current animation sample",
            );
        }
    }
}

pub(crate) fn animated_scale_and_rotation_publish_clipped_bounds() {
    use flui_animation::Animatable;
    use flui_animation::{Animation, AnimationController, Tween, Vsync};
    use flui_painting::Alignment;
    use flui_view::ViewExt;
    use flui_widgets::{
        Align, ClipRect, RotationTransition, ScaleTransition, Transform, VsyncScope,
    };
    use std::rc::Rc;
    use std::time::Duration;

    for (rotation, clipped, expected) in [
        (false, false, (40.0, 70.0, 160.0, 130.0)),
        (false, true, (60.0, 80.0, 140.0, 120.0)),
        (true, false, (80.0, 60.0, 120.0, 140.0)),
        (true, true, (80.0, 80.0, 120.0, 120.0)),
    ] {
        let vsync = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
        let scalar: Rc<dyn Animation<f64>> = Rc::new(owner.controller().clone());
        let child = Semantics::new()
            .container(true)
            .role(SemanticsRole::Button)
            .label("Projected control")
            .child(SizedBox::new(40.0, 20.0));
        let transition = if rotation {
            RotationTransition::new(Rc::new(Tween::new(0.0, 0.5).animate(scalar)), child).boxed()
        } else {
            ScaleTransition::new(Rc::new(Tween::new(1.0, 2.0).animate(scalar)), child).boxed()
        };
        let content = if clipped {
            ClipRect::new().child(transition).boxed()
        } else {
            transition
        };
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                Align::new(Alignment::CENTER).child(Transform::scale(2.0, 2.0).child(content)),
            ),
            crate::common::tight(200.0, 200.0),
            vsync,
        );
        laid.enable_semantics();
        laid.tick();
        assert_rect_near(
            effective_bounds(
                &laid.a11y_tree().expect("initial semantics"),
                "Projected control",
            ),
            (60.0, 80.0, 140.0, 120.0),
            "scaled ancestor",
        );
        owner.controller().forward().expect("fresh projected run");
        laid.tick();
        laid.pump_for(Duration::from_millis(500));
        assert_rect_near(
            effective_bounds(
                &laid.a11y_tree().expect("projected semantics"),
                "Projected control",
            ),
            expected,
            "rotated or scaled control under a transformed clip",
        );
    }
}

pub(crate) fn unrepresentable_projected_bounds_are_withdrawn_and_recover() {
    use flui_foundation::geometry::Matrix4;
    use flui_painting::Alignment;
    use flui_widgets::{Align, Transform};

    let view = |scale| {
        Align::new(Alignment::CENTER).child(
            Transform::new(Matrix4::scaling(scale, scale, 1.0)).child(
                Semantics::new()
                    .container(true)
                    .label("Finite projection")
                    .child(SizedBox::new(40.0, 20.0)),
            ),
        )
    };
    let mut laid = lay_out(view(1.0), crate::common::tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    assert_rect_near(
        effective_bounds(
            &laid.a11y_tree().expect("initial semantics"),
            "Finite projection",
        ),
        (80.0, 90.0, 120.0, 110.0),
        "initial finite projection",
    );
    laid.pump_widget(view(f64::MAX));
    assert!(
        laid.a11y_tree()
            .expect("overflow semantics")
            .find_by_label("Finite projection")
            .is_err(),
        "finite matrix entries whose corner arithmetic overflows cannot publish a bound"
    );
    laid.pump_widget(view(1.0));
    assert_rect_near(
        effective_bounds(
            &laid.a11y_tree().expect("recovered semantics"),
            "Finite projection",
        ),
        (80.0, 90.0, 120.0, 110.0),
        "recovered finite projection",
    );
}

/// Where an AccessKit adapter places the labelled node: its bounds under
/// its own transform and every ancestor's, which AccessKit defines as
/// physical pixels relative to the window's client area.
fn effective_bounds(tree: &flui_testing::A11yTree, label: &str) -> flui_testing::a11y::A11yRect {
    let update = tree.raw();
    let parent_of = |id| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.children().contains(&id))
            .map(|(key, node)| (*key, node))
    };
    let (target, node) = update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect("the labelled node is published");
    let mut rect = node.bounds().expect("the labelled node has bounds");
    if let Some(transform) = node.transform() {
        rect = transform.transform_rect_bbox(rect);
    }
    let mut current = *target;
    while let Some((parent, parent_node)) = parent_of(current) {
        if let Some(transform) = parent_node.transform() {
            rect = transform.transform_rect_bbox(rect);
        }
        current = parent;
    }
    rect
}

fn assert_rect_near(
    actual: flui_testing::a11y::A11yRect,
    expected: (f64, f64, f64, f64),
    what: &str,
) {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(
        close(actual.x0, expected.0)
            && close(actual.y0, expected.1)
            && close(actual.x1, expected.2)
            && close(actual.y1, expected.3),
        "{what}: published {actual:?}, expected {expected:?}"
    );
}

/// A window at 150% publishes physical bounds, and a scale change
/// republishes them.
///
/// The button sits 30 logical pixels down a column inside a scrollable
/// scrolled by 10, so its logical rect starts at y = 20: the
/// scroll offset and the parent offset reach the published rect before the
/// window's scale multiplies it. Assistive technology hit-tests and draws its
/// highlight from these numbers; at the logical size it lands on two thirds
/// of the control.
pub(crate) fn published_bounds_are_physical_and_follow_the_scale_factor() {
    use flui_view::ViewExt as _;
    use flui_widgets::{Column, ScrollController, Scrollable};
    use std::sync::{Arc, Mutex};

    let controller = ScrollController::new();
    let content = Column::new((
        SizedBox::new(200.0, 30.0).boxed(),
        Semantics::new()
            .container(true)
            .role(SemanticsRole::Button)
            .label("Physical")
            .on_click(|_cx| {})
            .child(SizedBox::new(200.0, 40.0))
            .boxed(),
        SizedBox::new(200.0, 600.0).boxed(),
    ));
    let mut laid = lay_out(
        Scrollable::new()
            .controller(controller.clone())
            .child(content.boxed()),
        crate::common::tight(200.0, 200.0),
    );

    let delivered: Arc<Mutex<Option<flui_testing::A11yTree>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&delivered);
    laid.pipeline_owner().with_mut(|owner| {
        owner.set_device_pixel_ratio(1.5);
        owner.set_semantics_update_callback(Arc::new(move |update| {
            let mut mirror = sink.lock().expect("publication mirror lock");
            let Some(current) = mirror.as_ref() else {
                *mirror = Some(flui_testing::A11yTree::new(update.clone()));
                return;
            };
            let mut merged = current.raw().clone();
            for (id, node) in &update.nodes {
                if let Some((_, previous)) = merged.nodes.iter_mut().find(|(key, _)| key == id) {
                    previous.clone_from(node);
                } else {
                    merged.nodes.push((*id, node.clone()));
                }
            }
            *mirror = Some(flui_testing::A11yTree::new(merged));
        }));
    });
    laid.enable_semantics();
    laid.tick();
    controller.jump_to(10.0);
    laid.tick();

    let published = || {
        delivered
            .lock()
            .expect("publication mirror lock")
            .clone()
            .expect("the adapter was published to")
    };
    let logical = || {
        published()
            .find_by_label("Physical")
            .expect("premise: the button is published")
            .bounds()
            .expect("premise: the button has bounds")
    };
    // The column offset (30) less the scroll offset (10) reached the rect.
    let before = logical();
    assert!(
        (before.y0 - 20.0).abs() < 1e-9 && (before.height() - 40.0).abs() < 1e-9,
        "premise: the semantics rect is logical and scrolled, got {before:?}"
    );
    let scaled = |rect: flui_testing::a11y::A11yRect, ratio: f64| {
        (
            rect.x0 * ratio,
            rect.y0 * ratio,
            rect.x1 * ratio,
            rect.y1 * ratio,
        )
    };
    assert_rect_near(
        effective_bounds(&published(), "Physical"),
        scaled(before, 1.5),
        "at a ratio of 1.5",
    );

    // Moving the window to a 200% monitor: the adapter must hear the new
    // scale.
    laid.pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    laid.tick();
    assert_rect_near(
        effective_bounds(&published(), "Physical"),
        scaled(logical(), 2.0),
        "after the ratio changed to 2.0",
    );
}

/// Custom actions from two `Semantics` sharing one node are both offered, the
/// inner one's after the outer one's, and each request runs the handler its
/// own widget registered, though the merge published the inner action under
/// another id than the one its widget gave it.
pub(crate) fn merged_custom_actions_each_reach_their_own_handler() {
    let ran = Rc::new(RefCell::new(Vec::new()));
    let (outer, inner) = (Rc::clone(&ran), Rc::clone(&ran));
    let (laid, tree, node_id) = pump_labelled(
        host()
            .custom_action("Archive", move |_cx| outer.borrow_mut().push("archive"))
            .child(
                Semantics::new()
                    .custom_action("Delete", move |_cx| inner.borrow_mut().push("delete"))
                    .child(SizedBox::new(40.0, 20.0)),
            ),
    );
    let published: Vec<(i32, String)> = tree
        .find_by_label(LABEL)
        .expect("node was located a moment ago")
        .raw()
        .custom_actions()
        .iter()
        .map(|action| (action.id, action.description.clone()))
        .collect();
    assert_eq!(
        published,
        [(1, "Archive".to_owned()), (2, "Delete".to_owned())],
        "both widgets' actions are offered on the one node. Tree was:\n{}",
        tree.describe()
    );

    for id in [2, 1] {
        laid.invoke_semantics_action(request(
            Action::CustomAction,
            node_id,
            Some(ActionData::CustomAction(id)),
        ))
        .expect("a published custom action resolves");
    }
    assert_eq!(
        *ran.borrow(),
        ["delete", "archive"],
        "each ran its own handler"
    );
}

/// A label names one action of a widget: a second `custom_action` with the
/// same label replaces the first, so the platform is offered one "Delete"
/// and a request for it runs the latest handler.
pub(crate) fn a_repeated_custom_action_label_replaces_the_action() {
    let ran = Rc::new(RefCell::new(Vec::new()));
    let (first, second) = (Rc::clone(&ran), Rc::clone(&ran));
    let (laid, tree, node_id) = pump_labelled(
        host()
            .custom_action("Delete", move |_cx| first.borrow_mut().push("first"))
            .custom_action("Delete", move |_cx| second.borrow_mut().push("second"))
            .child(SizedBox::new(40.0, 20.0)),
    );
    let offered = tree
        .find_by_label(LABEL)
        .expect("node was located a moment ago")
        .raw()
        .custom_actions()
        .len();
    assert_eq!(offered, 1, "one action per label");
    laid.invoke_semantics_action(request(
        Action::CustomAction,
        node_id,
        Some(ActionData::CustomAction(1)),
    ))
    .expect("the offered action resolves");
    assert_eq!(*ran.borrow(), ["second"]);
}

/// A semantics diagnostic is reported with the pipeline checkout released: a
/// `tracing` subscriber may read the pipeline through its public handle while
/// it handles one, even disable semantics. A subscriber that panics does not
/// fail the frame, whose layout stays published; it leaves the diagnostic
/// pending, past the owner it disposed, and the next frame reports it again.
pub(crate) fn a_diagnostic_is_reported_outside_the_pipeline_checkout() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    thread_local! {
        static PIPELINE: RefCell<Option<flui_rendering::pipeline::PipelineCell>> =
            const { RefCell::new(None) };
    }

    /// Reads the pipeline on each semantics warning, and panics on the first.
    struct Reentrant {
        panicked: AtomicBool,
        reads: AtomicUsize,
    }
    impl tracing::Subscriber for Reentrant {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if *event.metadata().level() != tracing::Level::WARN
                || !event.metadata().target().starts_with("flui_semantics")
            {
                return;
            }
            PIPELINE.with(|cell| {
                let cell = cell.borrow();
                let pipeline = cell.as_ref().expect("the test stashed its pipeline");
                assert!(pipeline.with(|owner| owner.semantics_owner().is_some()));
                // The first report also disposes the owner, through the same
                // handle, before it panics.
                if !self.panicked.load(Ordering::SeqCst) {
                    pipeline.with_mut(|owner| owner.set_semantics_enabled(false));
                }
            });
            self.reads.fetch_add(1, Ordering::SeqCst);
            assert!(
                self.panicked.swap(true, Ordering::SeqCst),
                "the subscriber panics"
            );
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let mut laid = lay_out(host().child(SizedBox::new(40.0, 20.0)), loose(200.0));
    laid.enable_semantics();
    laid.pump();
    PIPELINE.with(|cell| *cell.borrow_mut() = Some(laid.pipeline_owner()));
    let subscriber = std::sync::Arc::new(Reentrant {
        panicked: AtomicBool::new(false),
        reads: AtomicUsize::new(0),
    });

    // A button does not carry `selected`: the frame leaves it out and queues
    // a diagnostic, which the subscriber reads the pipeline for, then panics.
    let selected_button = || {
        host()
            .role(SemanticsRole::Button)
            .selected(true)
            .child(SizedBox::new(40.0, 20.0))
    };
    let outcome = tracing::subscriber::with_default(std::sync::Arc::clone(&subscriber), || {
        catch_unwind(AssertUnwindSafe(|| laid.pump_widget(selected_button())))
    });
    assert!(outcome.is_err(), "the harness fails the frame");
    assert_eq!(
        subscriber.reads.load(Ordering::SeqCst),
        1,
        "it read the pipeline"
    );
    assert!(
        laid.pipeline_owner().try_with_layout(|_| ()).is_some(),
        "the subscriber's panic did not fail the frame"
    );

    // Semantics back on: the next frame reports the finding the panic left,
    // which outlived the disposed owner, and the one the new owner finds. The
    // harness fails every frame that left an attribute out, which this test
    // does on purpose; what it pins is the subscriber's reads.
    laid.enable_semantics();
    let _harness = tracing::subscriber::with_default(std::sync::Arc::clone(&subscriber), || {
        catch_unwind(AssertUnwindSafe(|| laid.pump()))
    });
    assert_eq!(
        subscriber.reads.load(Ordering::SeqCst),
        3,
        "the diagnostic the panic left survives the disposed owner and is reported next"
    );
    PIPELINE.with(|cell| *cell.borrow_mut() = None);
}

// ===========================================================================
// Identifier (ADR-0240): the node a control forms carries its author's id
// ===========================================================================

/// The published node whose `author_id` is `identifier`, if exactly one is.
fn identified_node<'a>(
    tree: &'a flui_testing::A11yTree,
    identifier: &str,
) -> Option<flui_testing::A11yNode<'a>> {
    let mut matches = tree
        .nodes()
        .filter(|node| node.raw().author_id() == Some(identifier));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// `root` mounted alone, semantics on, one frame pumped.
fn identifier_tree(root: impl View) -> flui_testing::A11yTree {
    let mut laid = lay_out(root, loose(200.0));
    laid.enable_semantics();
    laid.pump();
    laid.a11y_tree()
        .expect("semantics enabled before the frame")
}

/// Every control that builds its own semantics publishes the identifier its
/// author gave it on the node it forms, under the role it announces, and never
/// in what it announces; a wrapper that forms no node, and a node a merge
/// absorbs, publish none.
pub(crate) fn controls_publish_their_identifier_on_the_node_they_form() {
    use flui_view::{BoxedView, ViewExt as _};
    use flui_widgets::{
        Column, Disclosure, ExpansionState, RawButton, RawTextField, Text, TextEditingController,
    };

    struct Row {
        control: &'static str,
        identifier: &'static str,
        role: Option<flui_testing::a11y::Role>,
        view: fn() -> BoxedView,
    }
    let rows = [
        Row {
            control: "Semantics",
            identifier: "panel",
            role: None,
            view: || {
                Semantics::new()
                    .container(true)
                    .label("Panel")
                    .identifier("panel")
                    .child(SizedBox::new(40.0, 20.0))
                    .boxed()
            },
        },
        Row {
            control: "MergeSemantics",
            identifier: "merged",
            role: None,
            view: || {
                MergeSemantics::new()
                    .identifier("merged")
                    .child(
                        Semantics::new()
                            .container(true)
                            .label("Inner")
                            .identifier("absorbed")
                            .child(SizedBox::new(40.0, 20.0)),
                    )
                    .boxed()
            },
        },
        Row {
            control: "RawButton",
            identifier: "send",
            role: Some(flui_testing::a11y::Role::Button),
            view: || {
                RawButton::new(Text::new("Send"))
                    .on_press(|_cx| {})
                    .identifier("send")
                    .boxed()
            },
        },
        Row {
            control: "Disclosure",
            identifier: "details",
            role: Some(flui_testing::a11y::Role::Button),
            view: || {
                Disclosure::new(
                    ExpansionState::Collapsed,
                    Text::new("Details"),
                    Text::new("Body"),
                )
                .on_changed(|_cx, _state| {})
                .identifier("details")
                .boxed()
            },
        },
        Row {
            control: "RawTextField",
            identifier: "query",
            role: Some(flui_testing::a11y::Role::TextInput),
            view: || {
                RawTextField::new(TextEditingController::new())
                    .identifier("query")
                    .boxed()
            },
        },
    ];

    let mut failed = Vec::new();
    for row in &rows {
        let tree = identifier_tree((row.view)());
        let Some(node) = identified_node(&tree, row.identifier) else {
            failed.push(format!(
                "{}: no one node carries it\n{}",
                row.control,
                tree.describe()
            ));
            continue;
        };
        if row.role.is_some_and(|role| node.role() != role) {
            failed.push(format!("{}: on a {:?} node", row.control, node.role()));
        }
        let announced = [
            node.label(),
            node.value(),
            node.raw().description(),
            node.raw().tooltip(),
        ];
        if announced
            .iter()
            .flatten()
            .any(|text| text.contains(row.identifier))
        {
            failed.push(format!("{}: announced: {announced:?}", row.control));
        }
    }

    // A merge keeps its own identifier and drops its descendant's.
    let merged = identifier_tree((rows[1].view)());
    if identified_node(&merged, "absorbed").is_some() {
        failed.push(format!(
            "MergeSemantics: carried a merged descendant's identifier\n{}",
            merged.describe()
        ));
    }

    // A wrapper that forms no node of its own publishes no identifier and
    // changes nothing about the tree.
    let inner = || {
        Semantics::new()
            .container(true)
            .label("Inner")
            .child(SizedBox::new(40.0, 20.0))
    };
    let plain = identifier_tree(Column::new((inner().boxed(),)));
    let wrapped = identifier_tree(Column::new((Semantics::new()
        .identifier("ghost")
        .child(inner())
        .boxed(),)));
    if identified_node(&wrapped, "ghost").is_some() || wrapped.describe() != plain.describe() {
        failed.push(format!(
            "a non-boundary wrapper: changed the tree\n{}\nvs\n{}",
            wrapped.describe(),
            plain.describe()
        ));
    }

    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
