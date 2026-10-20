//! A mounted app's element, render and layer trees read as typed snapshots (ADR-0242): the
//! trees link to each other, budgets cut a read with counts of what was left out, and an id
//! from before a removal is gone.

use flui_foundation::inspect::{
    Inspect, Link, OwnedValue, SnapshotError, SnapshotNode, SnapshotQuery, Variant,
};
use flui_testing::widgets::{LaidOut, lay_out, tight};
use flui_view::prelude::*;
use flui_widgets::{Center, Column, RawButton, SizedBox, Text, column};

#[derive(Clone, StatefulView)]
struct CounterView;

#[derive(Default)]
struct CounterState {
    count: Signal<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState::default()
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
    }

    fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        Center::new().child(Column::new(column![
            Text::new(count.get(ctx).to_string()),
            SizedBox::height(16.0),
            RawButton::new(Text::new("Increment"))
                .on_press(move |cx| count.update(cx, |n| *n += 1)),
        ]))
    }
}

/// A view that describes its configuration through `View::inspect`, which the view derive
/// fills from the type's own `Inspect`.
#[derive(Clone, StatelessView, Inspect)]
#[view(inspect)]
struct Labelled {
    label: &'static str,
}

impl StatelessView for Labelled {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.label)
    }
}

fn counter() -> LaidOut {
    let mut app = lay_out(CounterView, tight(400.0, 300.0));
    app.enable_semantics();
    app.tick();
    app
}

fn everything() -> SnapshotQuery {
    SnapshotQuery::default()
}

fn from(link: Link) -> SnapshotQuery {
    SnapshotQuery {
        root: Some(link.into()),
        ..SnapshotQuery::default()
    }
}

/// The node itself, then its descendants in pre-order.
fn subtree(node: &SnapshotNode) -> Vec<&SnapshotNode> {
    let mut nodes = vec![node];
    let mut at = 0;
    while let Some(next) = nodes.get(at) {
        nodes.extend(next.children.iter());
        at += 1;
    }
    nodes
}

/// Every element the counter mounted links to a render object of the render tree, and the
/// button's render objects link to the semantics node that announces it.
///
/// Fails when the element tree reports no render links, when the render tree reports no
/// semantics links, or when either names a node the other tree does not hold.
#[test]
fn a_counters_elements_link_to_render_objects_that_link_to_semantics() {
    let app = counter();
    let elements = app
        .inspect_elements(everything())
        .expect("the app is mounted");
    let render = app
        .inspect_render(everything())
        .expect("the app is laid out");

    let view = elements
        .find_by_kind("CounterView")
        .unique()
        .expect("one counter");
    assert_eq!(view.get::<&str>("element"), Ok("StatefulElement"));
    assert_eq!(
        view.get::<Variant<'_>>("lifecycle").map(|it| it.name),
        Ok("Active")
    );

    let linked: Vec<_> = subtree(view)
        .into_iter()
        .filter_map(|node| node.links.render)
        .collect();
    assert!(
        linked.len() >= 3,
        "the paragraphs, the box and the button's render objects: {linked:?}"
    );
    for id in &linked {
        let node = render
            .find_by_link(Link::Render(*id))
            .unwrap_or_else(|| panic!("element links r{id:?}, which the render tree holds"));
        assert_eq!(node.links.render, Some(*id));
    }

    let button = elements
        .find_by_kind("RawButton")
        .unique()
        .expect("one button");
    let button_render = subtree(button)
        .into_iter()
        .find_map(|node| node.links.render)
        .expect("the button's subtree owns a render object");
    let announced = subtree(
        render
            .find_by_link(Link::Render(button_render))
            .expect("the button's render object is in the render tree"),
    )
    .into_iter()
    .find_map(|node| node.links.semantics)
    .expect("the button's render objects contribute a semantics node");
    let label = app.pipeline_owner().with(|owner| {
        owner
            .semantics_owner()
            .expect("semantics is enabled")
            .get(announced)
            .and_then(|node| node.label().map(str::to_owned))
    });
    assert_eq!(label.as_deref(), Some("Increment"));
}

/// Render facts are typed: protocol, size and offset read back as values, and the rect is the
/// laid-out box in the parent's coordinates.
#[test]
fn render_facts_read_back_typed() {
    let app = counter();
    let render = app.inspect_render(everything()).expect("laid out");
    let paragraph = render
        .iter()
        .find(|node| node.kind == "RenderParagraph")
        .expect("a paragraph");
    let size = paragraph
        .get::<flui_foundation::geometry::Size>("size")
        .expect("a laid-out box has a size");
    assert!(size.width > 0.0 && size.height > 0.0, "{size:?}");
    let rect = paragraph.rect.expect("a laid-out box has a rect");
    assert_eq!((rect.width(), rect.height()), (size.width, size.height));
    assert!(
        paragraph.get::<&[OwnedValue]>("properties").is_ok(),
        "the render object's own properties follow the facts"
    );
}

/// A view's own `View::inspect` fields follow the element's facts, and its kind is the view's
/// type name without its path.
#[test]
fn a_views_inspect_fields_follow_the_element_facts() {
    let app = lay_out(Labelled { label: "hello" }, tight(100.0, 100.0));
    let elements = app.inspect_elements(everything()).expect("mounted");
    let labelled = elements
        .find_by_kind("Labelled")
        .unique()
        .expect("one view");
    assert_eq!(labelled.get::<&str>("label"), Ok("hello"));
    let names: Vec<_> = labelled.fields.iter().map(|field| field.name).collect();
    assert_eq!(names.first(), Some(&"element"), "facts first: {names:?}");
    assert_eq!(
        names.last(),
        Some(&"label"),
        "the view's fields last: {names:?}"
    );
}

/// Depth and node budgets cut every tree, and each cut node counts the children it left out.
///
/// Fails when a tree ignores the budget or drops the count.
#[test]
fn budgets_truncate_every_tree_with_omitted_children_counts() {
    let app = counter();
    type Read = fn(&LaidOut, SnapshotQuery) -> Result<SnapshotNode, SnapshotError>;
    let trees: [(&str, Read); 3] = [
        ("elements", LaidOut::inspect_elements),
        ("render", LaidOut::inspect_render),
        ("layers", LaidOut::inspect_layers),
    ];
    for (name, read) in trees {
        let whole = read(&app, everything()).unwrap_or_else(|error| panic!("{name}: {error}"));
        let total = whole.iter().count();
        assert!(total > 1, "{name}: the app draws more than one node");
        assert!(
            whole.iter().all(|node| node.omitted_children == 0),
            "{name}: an unbudgeted read leaves nothing out"
        );

        let one = read(
            &app,
            SnapshotQuery {
                max_nodes: Some(1),
                ..everything()
            },
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            one.children.is_empty(),
            "{name}: one node is the root alone"
        );
        assert_eq!(
            one.omitted_children as usize,
            whole.children.len(),
            "{name}: the root counts every child it left out"
        );

        let shallow = read(
            &app,
            SnapshotQuery {
                max_depth: Some(0),
                ..everything()
            },
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            shallow.iter().count(),
            1,
            "{name}: depth 0 is the root alone"
        );
        assert_eq!(shallow.omitted_children as usize, whole.children.len());
    }
}

/// A read that starts at an element or render object removed since answers `gone`, even
/// when a newer node now holds its slot.
///
/// Fails when a removed node reads as empty, as the root, or as whatever holds its slot now.
#[test]
fn an_id_from_before_a_removal_is_gone() {
    let mut app = counter();
    let elements = app.inspect_elements(everything()).expect("mounted");
    let view = elements
        .find_by_kind("CounterView")
        .unique()
        .expect("one counter");
    let element = view.id.link();
    let render = subtree(view)
        .into_iter()
        .find_map(|node| node.links.render)
        .map(Link::Render)
        .expect("the counter owns render objects");
    assert!(app.inspect_elements(from(element)).is_ok());
    assert!(app.inspect_render(from(render)).is_ok());

    app.pump_widget(Labelled { label: "replaced" });
    app.tick();

    let Link::Element(old) = element else {
        unreachable!("an element tree names elements")
    };
    let after = app.inspect_elements(everything()).expect("mounted");
    assert!(
        after.iter().any(|node| matches!(
            node.id.link(),
            Link::Element(id) if id.index() == old.index() && id != old
        )),
        "the replacement reuses the counter's slot, so only the generation tells them apart"
    );

    assert_eq!(
        app.inspect_elements(from(element)),
        Err(SnapshotError::Gone(element.into()))
    );
    assert_eq!(
        app.inspect_render(from(render)),
        Err(SnapshotError::Gone(render.into()))
    );
}

/// A layer id kept from an earlier frame answers `gone` after a repaint, although the new
/// frame's tree mints the same slots from the first.
///
/// Fails when the layer tree resolves a kept id by its slot alone, which reads an unrelated layer
/// of the newer scene.
#[test]
fn a_layer_id_from_an_earlier_frame_is_gone() {
    let mut app = counter();
    let layers = app.inspect_layers(everything()).expect("painted");
    let kept = layers.id.link();
    assert!(app.inspect_layers(from(kept)).is_ok());

    app.pump_widget(Labelled { label: "replaced" });
    app.tick();

    let after = app.inspect_layers(everything()).expect("painted");
    let (Link::Layer(old), Link::Layer(new)) = (kept, after.id.link()) else {
        unreachable!("a layer tree names layers")
    };
    assert_eq!(
        new.id, old.id,
        "both scenes mint their root in the first slot"
    );
    assert_ne!(new.scene, old.scene);
    assert_eq!(
        app.inspect_layers(from(kept)),
        Err(SnapshotError::Gone(kept.into()))
    );
}
