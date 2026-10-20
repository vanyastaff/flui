//! `inspect`: typed self-description through the public API (ADR-0242).
//!
//! The value-mapping and derive tables, the compact `Display` goldens with their round trips,
//! the outline renderer, typed access to a snapshot, and the structural diff. Zero allocation
//! for a counting sink is `tests/inspect_allocation.rs`, a target of its own because it
//! installs a counting allocator.

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Matrix4, Offset, Rect, Size};
use flui_foundation::inspect::{
    Change, DiffOptions, Field, FieldError, Fields, Inspect, InspectId, InspectTree, Level, Link,
    Links, MatchError, Meta, NodeView, OwnedValue, Plain, Rgba, SceneLayer, Seq, Sink,
    SnapshotError, SnapshotNode, SnapshotQuery, ToValue, Unit, Value, Variant, diff, kind_args_of,
    kind_of, render, snapshot,
};
use flui_foundation::{ElementId, LayerId, RenderId, SemanticsId};

/// One received field: name, value text, level, unit and default text.
type Row = (String, String, Level, Option<Unit>, Option<String>);

/// Every field a sink receives, sequences as `[a, b]`.
#[derive(Default)]
struct Collected(Vec<Row>);

impl Sink for Collected {
    fn field(&mut self, field: Field<'_>) {
        self.0.push((
            field.name.to_owned(),
            field.value.to_string(),
            field.meta.level,
            field.meta.unit,
            field.meta.default.map(|value| value.to_string()),
        ));
    }

    fn seq(&mut self, name: &'static str, meta: Meta<'_>, items: &dyn Seq) {
        let mut texts = Vec::new();
        items.each(&mut |item| texts.push(item.to_string()));
        self.0.push((
            name.to_owned(),
            format!("[{}]", texts.join(", ")),
            meta.level,
            meta.unit,
            meta.default.map(|value| value.to_string()),
        ));
    }
}

fn collect(object: &dyn Inspect) -> Vec<Row> {
    let mut sink = Collected::default();
    object.inspect(&mut Fields::new(&mut sink));
    sink.0
}

fn names_and_values(object: &dyn Inspect) -> Vec<(String, String)> {
    collect(object)
        .into_iter()
        .map(|(name, value, ..)| (name, value))
        .collect()
}

fn render_id(slot: u32) -> RenderId {
    RenderId::new_gen(slot, NonZeroU32::MIN)
}

/// A row's field value, owned by the row.
trait Boxed {
    fn into_boxed(self) -> Box<dyn ToValue>;
}

impl<T: ToValue + 'static> Boxed for T {
    fn into_boxed(self) -> Box<dyn ToValue> {
        Box::new(self)
    }
}

#[test]
fn each_field_type_maps_to_its_value_and_compact_text() {
    let matrix = Matrix4::translation(4.0, 2.0, 0.0);
    let identity = Matrix4::identity();
    let rows: Vec<(&str, Box<dyn ToValue>, &str, &str)> = vec![
        ("bool", true.into_boxed(), "bool", "true"),
        ("i8", (-3_i8).into_boxed(), "int", "-3"),
        ("i64", i64::MIN.into_boxed(), "int", "-9223372036854775808"),
        ("isize", (-7_isize).into_boxed(), "int", "-7"),
        ("u8", 255_u8.into_boxed(), "uint", "255"),
        ("usize", 12_usize.into_boxed(), "uint", "12"),
        ("u64", u64::MAX.into_boxed(), "uint", "18446744073709551615"),
        ("f32", 0.5_f32.into_boxed(), "float", "0.5"),
        ("f64", 0.1_f64.into_boxed(), "float", "0.1"),
        ("str", "Save".into_boxed(), "str", "\"Save\""),
        (
            "String with quotes",
            String::from("a \"b\"").into_boxed(),
            "str",
            "\"a \\\"b\\\"\"",
        ),
        (
            "size",
            Size::new(360.0, 48.0).into_boxed(),
            "size",
            "360x48",
        ),
        (
            "offset",
            Offset::new(8.0, -8.5).into_boxed(),
            "offset",
            "@8,-8.5",
        ),
        (
            "rect",
            Rect::from_xywh(8.0, 8.0, 344.0, 32.0).into_boxed(),
            "rect",
            "8,8 344x32",
        ),
        (
            "uniform insets",
            EdgeInsets::all(8.0).into_boxed(),
            "edge_insets",
            "8",
        ),
        (
            "insets",
            EdgeInsets::new(8.0, 4.0, 8.0, 4.0).into_boxed(),
            "edge_insets",
            "8,4,8,4",
        ),
        (
            "opaque colour",
            Rgba::new(1.0, 0.5, 0.0, 1.0).into_boxed(),
            "color",
            "#FF8000",
        ),
        (
            "translucent colour",
            Rgba::new(0.0, 0.0, 1.0, 0.5).into_boxed(),
            "color",
            "#0000FF80",
        ),
        ("identity", identity.into_boxed(), "matrix", "identity"),
        (
            "matrix",
            matrix.into_boxed(),
            "matrix",
            "[1,0,0,0,0,1,0,0,0,0,1,0,4,2,0,1]",
        ),
        (
            "milliseconds",
            Duration::from_millis(16).into_boxed(),
            "duration",
            "16ms",
        ),
        (
            "microseconds",
            Duration::from_micros(16_667).into_boxed(),
            "duration",
            "16667us",
        ),
        (
            "nanoseconds",
            Duration::from_nanos(40).into_boxed(),
            "duration",
            "40ns",
        ),
        ("render link", render_id(12).into_boxed(), "link", "r12"),
        (
            "element link, later generation",
            ElementId::new_gen(3, NonZeroU32::new(2).expect("2 is nonzero")).into_boxed(),
            "link",
            "v3.2",
        ),
        (
            "layer link",
            SceneLayer {
                scene: NonZeroU64::MIN,
                id: LayerId::new(4),
            }
            .into_boxed(),
            "link",
            "l4",
        ),
        (
            "layer link, later scene",
            SceneLayer {
                scene: NonZeroU64::new(37).expect("37 is nonzero"),
                id: LayerId::new(4),
            }
            .into_boxed(),
            "link",
            "l4.37",
        ),
        (
            "semantics link",
            SemanticsId::new(7).into_boxed(),
            "link",
            "s7",
        ),
        (
            "axis",
            flui_foundation::geometry::Axis::Vertical.into_boxed(),
            "variant",
            "Vertical",
        ),
        ("some", Some(3_u32).into_boxed(), "uint", "3"),
        ("none", None::<u32>.into_boxed(), "none", "none"),
        ("boxed", Box::new(2.5_f64).into_boxed(), "float", "2.5"),
    ];
    let mut failures = Vec::new();
    for (row, object, kind, text) in rows {
        let value = object.to_value();
        if value.kind_name() != kind || value.to_string() != text {
            failures.push(format!(
                "{row}: expected {kind} `{text}`, got {} `{value}`",
                value.kind_name()
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn values_compare_by_value_and_debug_equals_nothing() {
    assert_eq!(Value::Float(1.0), 1.0_f64.to_value());
    assert_ne!(
        Value::Float(1.0),
        Value::Int(1),
        "kinds never compare equal"
    );
    assert_eq!(Value::Str("a"), String::from("a").to_value());
    let text = "same";
    assert_ne!(
        Value::Debug(&text),
        Value::Debug(&text),
        "an untyped value cannot be shown equal, even to itself"
    );
}

/// Shortest round-trip floats, with no exponent, so every number parses back to its bits.
const AWKWARD: [f64; 7] = [
    0.0,
    -8.5,
    0.300_000_000_000_000_04,
    1e-7,
    1e21,
    f64::MAX,
    123.456,
];

fn parse(text: &str) -> f64 {
    text.parse()
        .unwrap_or_else(|error| panic!("`{text}` is not a float: {error}"))
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

#[test]
fn geometry_text_parses_back_to_the_same_values() {
    for (index, &a) in AWKWARD.iter().enumerate() {
        let b = AWKWARD[(index + 3) % AWKWARD.len()];

        let size = Size::new(a, b).to_string();
        let (width, height) = size.split_once('x').expect("a size is `WxH`");
        assert!(
            same(parse(width), a) && same(parse(height), b),
            "size `{size}`"
        );

        let offset = Offset::new(a, b).to_string();
        let (dx, dy) = offset
            .strip_prefix('@')
            .and_then(|rest| rest.split_once(','))
            .expect("an offset is `@x,y`");
        assert!(
            same(parse(dx), a) && same(parse(dy), b),
            "offset `{offset}`"
        );

        // A rect keeps its corners, so its size is what the corners give (`1e21 + 2.5` is
        // `1e21`): the text must carry exactly that.
        let value = Rect::from_xywh(a, b, 4.0, 2.5);
        let rect = value.to_string();
        let (origin, extent) = rect.split_once(' ').expect("a rect is `x,y WxH`");
        let (x, y) = origin.split_once(',').expect("the origin is `x,y`");
        let (width, height) = extent.split_once('x').expect("the size is `WxH`");
        assert!(
            same(parse(x), value.left())
                && same(parse(y), value.top())
                && same(parse(width), value.width())
                && same(parse(height), value.height()),
            "rect `{rect}`"
        );

        let insets = EdgeInsets::new(a, b, a, 1.0).to_string();
        let parsed: Vec<f64> = insets.split(',').map(parse).collect();
        assert_eq!(parsed.len(), 4, "unequal insets print all four: `{insets}`");
        assert!(
            same(parsed[0], a) && same(parsed[1], b) && same(parsed[2], a) && same(parsed[3], 1.0),
            "insets `{insets}`"
        );
    }
}

#[test]
fn colour_and_duration_text_parse_back() {
    for bytes in [[0, 0, 0, 255], [255, 128, 1, 255], [16, 32, 48, 64]] {
        let colour = Rgba::new(
            f32::from(bytes[0]) / 255.0,
            f32::from(bytes[1]) / 255.0,
            f32::from(bytes[2]) / 255.0,
            f32::from(bytes[3]) / 255.0,
        );
        let text = colour.to_string();
        let hex = text.strip_prefix('#').expect("a colour starts with `#`");
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex");
        let alpha = if hex.len() == 8 { channel(6) } else { 255 };
        assert_eq!(
            [channel(0), channel(2), channel(4), alpha],
            bytes,
            "`{text}`"
        );
    }
    for duration in [
        Duration::from_millis(250),
        Duration::from_micros(1_500),
        Duration::from_nanos(16_666_667),
        Duration::ZERO,
    ] {
        let text = Value::Duration(duration).to_string();
        let (digits, scale) = if let Some(digits) = text.strip_suffix("ms") {
            (digits, 1_000_000)
        } else if let Some(digits) = text.strip_suffix("us") {
            (digits, 1_000)
        } else {
            (
                text.strip_suffix("ns").expect("a duration ends in a unit"),
                1,
            )
        };
        let nanos: u64 = digits.parse().expect("whole units");
        assert_eq!(Duration::from_nanos(nanos * scale), duration, "`{text}`");
    }
}

#[derive(Inspect)]
enum Fit {
    Contain,
    Cover,
}

#[derive(Inspect)]
struct Card {
    #[inspect(unit = logical_px)]
    padding: EdgeInsets,
    #[inspect(default = Fit::Contain)]
    fit: Fit,
    #[inspect(name = "type", level = detail)]
    r#type: &'static str,
    #[inspect(default)]
    flex: u32,
    #[inspect(skip)]
    _cache: Vec<u8>,
    #[inspect(with = describe_seconds, unit = seconds)]
    delay: Duration,
    #[inspect(debug)]
    opaque: Opaque,
    labels: Vec<&'static str>,
    tint: Option<Rgba>,
}

#[derive(Debug)]
struct Opaque(
    #[expect(
        dead_code,
        reason = "read only through `Debug`, which dead-code analysis ignores"
    )]
    u8,
);

fn describe_seconds(duration: &Duration) -> Value<'_> {
    Value::Float(duration.as_secs_f64())
}

#[derive(Inspect)]
struct Pair(#[inspect(unit = logical_px)] f64, &'static str);

#[derive(Inspect)]
struct Marker;

#[derive(Inspect)]
enum Shape {
    Circle {
        #[inspect(unit = logical_px)]
        radius: f64,
    },
    Line(Offset, Offset),
    Empty,
}

#[derive(Inspect)]
struct Wrap<T> {
    value: T,
    #[inspect(debug)]
    every: Vec<T>,
}

#[test]
fn the_derive_describes_every_shape_with_its_attributes() {
    let card = Card {
        padding: EdgeInsets::all(8.0),
        fit: Fit::Cover,
        r#type: "card",
        flex: 0,
        _cache: vec![1, 2, 3],
        delay: Duration::from_millis(1_500),
        opaque: Opaque(7),
        labels: vec!["a", "b"],
        tint: None,
    };
    assert_eq!(card.kind(), "Card");
    assert_eq!(
        collect(&card),
        vec![
            (
                "padding".into(),
                "8".into(),
                Level::Summary,
                Some(Unit::LogicalPx),
                None
            ),
            (
                "fit".into(),
                "Cover".into(),
                Level::Summary,
                None,
                Some("Contain".into())
            ),
            ("type".into(), "\"card\"".into(), Level::Detail, None, None),
            (
                "flex".into(),
                "0".into(),
                Level::Summary,
                None,
                Some("0".into())
            ),
            (
                "delay".into(),
                "1.5".into(),
                Level::Summary,
                Some(Unit::Seconds),
                None
            ),
            (
                "opaque".into(),
                "Opaque(7)".into(),
                Level::Summary,
                None,
                None
            ),
            (
                "labels".into(),
                "[\"a\", \"b\"]".into(),
                Level::Summary,
                None,
                None
            ),
            ("tint".into(), "none".into(), Level::Summary, None, None),
        ],
        "skip drops `_cache`; name, level, unit, default, with and debug apply"
    );

    let line = Shape::Line(Offset::ZERO, Offset::new(1.0, 2.0));
    type ShapeRow<'a> = (&'a str, &'a dyn Inspect, &'a str, Vec<(&'a str, &'a str)>);
    let rows: Vec<ShapeRow<'_>> = vec![
        (
            "tuple struct",
            &Pair(4.0, "x"),
            "Pair",
            vec![("0", "4"), ("1", "\"x\"")],
        ),
        ("unit struct", &Marker, "Marker", vec![]),
        (
            "struct variant",
            &Shape::Circle { radius: 2.0 },
            "Shape",
            vec![("variant", "Circle"), ("radius", "2")],
        ),
        (
            "tuple variant",
            &line,
            "Shape",
            vec![("variant", "Line"), ("0", "@0,0"), ("1", "@1,2")],
        ),
        (
            "unit variant",
            &Shape::Empty,
            "Shape",
            vec![("variant", "Empty")],
        ),
        (
            "fieldless enum",
            &Fit::Contain,
            "Fit",
            vec![("variant", "Contain")],
        ),
    ];
    for (row, object, kind, fields) in rows {
        assert_eq!(object.kind(), kind, "{row}");
        let expected: Vec<(String, String)> = fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        assert_eq!(names_and_values(object), expected, "{row}");
    }

    assert_eq!(
        Fit::Cover.to_value(),
        Value::Variant {
            ty: "Fit",
            name: "Cover"
        },
        "a fieldless enum is also a value"
    );
}

#[test]
fn a_generic_kind_reports_its_type_arguments() {
    let wrap = Wrap {
        value: 3_u16,
        every: vec![1, 2],
    };
    let mut args = Vec::new();
    wrap.kind_args(&mut |arg| args.push(arg));
    assert_eq!((wrap.kind(), args), ("Wrap", vec!["u16"]));
    assert_eq!(
        names_and_values(&wrap),
        vec![
            ("value".to_owned(), "3".to_owned()),
            ("every".to_owned(), "[1, 2]".to_owned())
        ]
    );
    let mut none = Vec::new();
    Marker.kind_args(&mut |arg| none.push(arg));
    assert!(none.is_empty(), "a non-generic kind has no arguments");
}

/// A render-object stand-in that describes itself by hand.
struct Node {
    kind: &'static str,
    fields: Vec<(&'static str, f64, Option<Unit>, Option<f64>)>,
    text: Option<&'static str>,
}

impl Inspect for Node {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        for (name, value, unit, default) in &self.fields {
            let mut field = out.field(name, value);
            if let Some(unit) = unit {
                field.unit(*unit);
            }
            if let Some(default) = default {
                field.default(default);
            }
        }
        if let Some(text) = self.text {
            out.field("text", text);
        }
    }
}

/// A tree of [`Node`]s keyed by render id, with each node's bounds and links.
struct Tree {
    root: Option<RenderId>,
    nodes: BTreeMap<RenderId, (Node, Option<Rect>, Links, Vec<RenderId>)>,
}

impl InspectTree for Tree {
    type Node = Node;

    fn root(&self) -> Option<InspectId> {
        self.root.map(InspectId::from)
    }

    fn node(&self, id: InspectId) -> Option<NodeView<'_, Node>> {
        let Link::Render(id) = id.link() else {
            return None;
        };
        self.nodes
            .get(&id)
            .map(|(object, rect, links, _)| NodeView {
                object,
                rect: *rect,
                links: *links,
            })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        let children = match id.link() {
            Link::Render(id) => self
                .nodes
                .get(&id)
                .map(|(.., children)| children.as_slice()),
            _ => None,
        };
        children
            .unwrap_or_default()
            .iter()
            .copied()
            .map(InspectId::from)
    }

    fn facts(&self, _: InspectId, out: &mut Fields<'_>) {
        out.field("attached", &true).level(Level::Debug);
    }
}

fn node(
    kind: &'static str,
    fields: Vec<(&'static str, f64, Option<Unit>, Option<f64>)>,
    text: Option<&'static str>,
) -> Node {
    Node { kind, fields, text }
}

/// `r1` RenderFlex(r2 RenderPadding(r3 RenderParagraph "Save"), r4 RenderParagraph "Cancel",
/// r5 RenderParagraph "Help").
fn sample(padding_left: f64) -> Tree {
    let px = Some(Unit::LogicalPx);
    let mut nodes = BTreeMap::new();
    nodes.insert(
        render_id(1),
        (
            node("RenderFlex", vec![("spacing", 0.0, px, Some(0.0))], None),
            Some(Rect::from_xywh(0.0, 0.0, 360.0, 48.0)),
            Links {
                element: Some(ElementId::new(10)),
                ..Links::default()
            },
            vec![render_id(2), render_id(4), render_id(5)],
        ),
    );
    nodes.insert(
        render_id(2),
        (
            node(
                "RenderPadding",
                vec![("left", padding_left, px, None)],
                None,
            ),
            Some(Rect::from_xywh(padding_left, 8.0, 344.0, 32.0)),
            Links::default(),
            vec![render_id(3)],
        ),
    );
    for (slot, text, x) in [(3, "Save", 0.0), (4, "Cancel", 200.0), (5, "Help", 300.0)] {
        nodes.insert(
            render_id(slot),
            (
                node("RenderParagraph", vec![], Some(text)),
                Some(Rect::from_xywh(x, 0.0, 40.0, 14.0)),
                Links {
                    semantics: Some(SemanticsId::new(slot as usize)),
                    ..Links::default()
                },
                vec![],
            ),
        );
    }
    Tree {
        root: Some(render_id(1)),
        nodes,
    }
}

fn whole(tree: &Tree) -> SnapshotNode {
    snapshot(tree, SnapshotQuery::default()).expect("the sample has a root")
}

#[test]
fn the_outline_shows_kind_id_fields_and_bounds_and_hides_defaults_at_summary() {
    let root = whole(&sample(8.0));
    let mut summary = String::new();
    render(&root, Level::Summary, &Plain, &mut summary).expect("a String accepts writes");
    assert_eq!(
        summary,
        "\
RenderFlex r1  0,0 360x48
  RenderPadding r2  left=8  8,8 344x32
    RenderParagraph r3  text=\"Save\"  0,0 40x14
  RenderParagraph r4  text=\"Cancel\"  200,0 40x14
  RenderParagraph r5  text=\"Help\"  300,0 40x14
"
    );
    assert_eq!(root.to_string(), summary, "`Display` is the plain summary");

    let mut debug = String::new();
    render(&root, Level::Debug, &Plain, &mut debug).expect("a String accepts writes");
    assert_eq!(
        debug.lines().next(),
        Some("RenderFlex r1  attached=true  spacing=0  0,0 360x48"),
        "debug shows the tree's facts and fields at their default"
    );

    let capped = snapshot(
        &sample(8.0),
        SnapshotQuery {
            max_depth: Some(1),
            max_nodes: Some(3),
            level: Level::Summary,
            ..SnapshotQuery::default()
        },
    )
    .expect("the sample has a root");
    assert_eq!(
        capped.to_string(),
        "\
RenderFlex r1  0,0 360x48
  RenderPadding r2  left=8  8,8 344x32
    … 1 more
  RenderParagraph r4  text=\"Cancel\"  200,0 40x14
  … 1 more
"
    );
    assert_eq!(
        capped.fact("attached"),
        None,
        "a summary query keeps no debug fact"
    );
}

#[test]
fn a_snapshot_reads_fields_by_type_and_finds_nodes_by_link_and_kind() {
    let root = whole(&sample(8.0));
    let padding = root
        .find_by_kind("RenderPadding")
        .unique()
        .expect("one padding");
    assert_eq!(padding.get::<f64>("left"), Ok(8.0));
    assert_eq!(
        padding.get::<&str>("left"),
        Err(FieldError::WrongKind {
            kind: "RenderPadding",
            id: render_id(2).into(),
            name: "left".to_owned(),
            expected: "str",
            found: "float",
        })
    );
    assert_eq!(
        padding.get::<f64>("right"),
        Err(FieldError::Missing {
            kind: "RenderPadding",
            id: render_id(2).into(),
            name: "right".to_owned(),
        })
    );
    assert_eq!(padding.get_fact::<bool>("attached"), Ok(true));
    assert_eq!(
        padding.get::<bool>("attached"),
        Err(FieldError::Missing {
            kind: "RenderPadding",
            id: render_id(2).into(),
            name: "attached".to_owned(),
        }),
        "a fact is not one of the object's fields"
    );

    let help = root
        .find_by_link(Link::Semantics(SemanticsId::new(5)))
        .expect("the node linked to s5");
    assert_eq!(help.get::<&str>("text"), Ok("Help"));
    let by_id = root
        .find_by_link(Link::Render(render_id(4)))
        .expect("the node r4");
    assert_eq!(by_id.get::<&str>("text"), Ok("Cancel"));
    assert!(
        root.find_by_link(Link::Layer(SceneLayer {
            scene: NonZeroU64::MIN,
            id: LayerId::new(1),
        }))
        .is_none()
    );
    let flex = root
        .find_by_link(Link::Element(ElementId::new(10)))
        .expect("the element link");
    assert_eq!(flex.kind, "RenderFlex");

    match root.find_by_kind("RenderParagraph").unique() {
        Err(MatchError::Ambiguous { count, ids, .. }) => {
            assert_eq!(count, 3);
            assert_eq!(
                ids,
                vec![
                    InspectId::from(render_id(3)),
                    render_id(4).into(),
                    render_id(5).into()
                ],
                "matches come in tree order"
            );
        }
        other => panic!("three paragraphs are ambiguous, got {other:?}"),
    }
    assert!(matches!(
        root.find_by_kind("RenderImage").unique(),
        Err(MatchError::None { .. })
    ));
    assert_eq!(root.total_debug_fields(), 0);
}

#[test]
fn a_snapshot_counts_untyped_fields_and_keeps_sequences() {
    struct WithDebug;
    impl Inspect for WithDebug {
        fn kind(&self) -> &'static str {
            "WithDebug"
        }

        fn inspect(&self, out: &mut Fields<'_>) {
            out.value("opaque", Value::Debug(&"x"));
            out.field("stops", &vec![0.0_f64, 0.5, 1.0]);
        }
    }
    struct One;
    impl InspectTree for One {
        type Node = WithDebug;

        fn root(&self) -> Option<InspectId> {
            Some(render_id(1).into())
        }

        fn node(&self, _: InspectId) -> Option<NodeView<'_, WithDebug>> {
            Some(NodeView {
                object: &WithDebug,
                rect: None,
                links: Links::default(),
            })
        }

        fn children(&self, _: InspectId) -> impl Iterator<Item = InspectId> + '_ {
            core::iter::empty()
        }
    }
    let root = snapshot(&One, SnapshotQuery::default()).expect("one node");
    assert_eq!(root.debug_fields, 1);
    assert_eq!(root.total_debug_fields(), 1);
    assert_eq!(
        root.get::<&[OwnedValue]>("stops"),
        Ok([
            OwnedValue::Float(0.0),
            OwnedValue::Float(0.5),
            OwnedValue::Float(1.0)
        ]
        .as_slice())
    );
    assert_eq!(
        root.to_string(),
        "WithDebug r1  opaque=\"x\"  stops=[0, 0.5, 1]\n"
    );
}

#[test]
fn a_query_from_a_missing_node_is_gone_and_an_empty_tree_is_empty() {
    let tree = sample(8.0);
    let missing = SnapshotQuery {
        root: Some(render_id(99).into()),
        ..SnapshotQuery::default()
    };
    assert_eq!(
        snapshot(&tree, missing),
        Err(SnapshotError::Gone(render_id(99).into()))
    );
    let empty = Tree {
        root: None,
        nodes: BTreeMap::new(),
    };
    assert_eq!(
        snapshot(&empty, SnapshotQuery::default()),
        Err(SnapshotError::Empty)
    );
}

#[test]
fn the_diff_reports_a_moved_node_with_its_delta_and_ignores_a_sub_tolerance_change() {
    let before = whole(&sample(8.0));
    let after = whole(&sample(12.0));
    let changes = diff(&before, &after, DiffOptions::default());
    assert_eq!(changes.len(), 2, "{changes:#?}");
    let moved = changes
        .iter()
        .find(|change| matches!(change, Change::Moved { .. }))
        .expect("the padding moved");
    let Change::Moved { node, .. } = moved else {
        unreachable!()
    };
    assert_eq!(
        (node.id, node.kind, node.path.as_slice()),
        (render_id(2).into(), "RenderPadding", [0].as_slice())
    );
    assert_eq!(moved.delta(), Some(Offset::new(4.0, 0.0)));
    assert!(changes.iter().any(|change| matches!(
        change,
        Change::Field {
            name: "left",
            from: Some(OwnedValue::Float(8.0)),
            to: Some(OwnedValue::Float(12.0)),
            ..
        }
    )));

    let nudged = whole(&sample(8.004));
    assert_eq!(
        diff(&before, &nudged, DiffOptions::default()),
        vec![],
        "0.004 px is under the default hundredth of a pixel"
    );
    assert_eq!(
        diff(&before, &nudged, DiffOptions::exact()).len(),
        2,
        "an exact diff sees the same nudge"
    );
}

/// The sample with the padding also describing a field named as the tree's fact.
fn with_attached_field(value: f64) -> Tree {
    let mut tree = sample(8.0);
    tree.nodes
        .get_mut(&render_id(2))
        .expect("the padding")
        .0
        .fields
        .push(("attached", value, None, None));
    tree
}

/// A fact and an object's field of the same name read and diff apart: each accessor reads its
/// own list, and the diff pairs facts with facts and fields with fields.
///
/// Fails when the snapshot puts both in one list, where the first of the two answers for both
/// and an unchanged field diffs against the fact.
#[test]
fn a_fact_and_a_field_of_the_same_name_stay_apart() {
    let before = whole(&with_attached_field(1.0));
    let padding = before
        .find_by_kind("RenderPadding")
        .unique()
        .expect("one padding");
    assert_eq!(padding.get::<f64>("attached"), Ok(1.0));
    assert_eq!(padding.get_fact::<bool>("attached"), Ok(true));
    assert_eq!(padding.debug_fields, 0);

    let mut debug = String::new();
    render(padding, Level::Debug, &Plain, &mut debug).expect("a String accepts writes");
    assert!(
        debug.starts_with("RenderPadding r2  attached=true  left=8  attached=1  "),
        "facts, then fields: {debug}"
    );

    assert_eq!(
        diff(
            &before,
            &whole(&with_attached_field(1.0)),
            DiffOptions::exact()
        ),
        vec![],
        "an unchanged field is no change, whatever the fact of its name holds"
    );
    let changes = diff(
        &before,
        &whole(&with_attached_field(2.0)),
        DiffOptions::exact(),
    );
    assert!(
        matches!(
            changes.as_slice(),
            [Change::Field {
                name: "attached",
                from: Some(OwnedValue::Float(1.0)),
                to: Some(OwnedValue::Float(2.0)),
                ..
            }]
        ),
        "the field changed and the fact did not: {changes:#?}"
    );

    // A summary read leaves out the debug-level fact and keeps the field of its name.
    let summary = snapshot(
        &with_attached_field(1.0),
        SnapshotQuery {
            level: Level::Summary,
            ..SnapshotQuery::default()
        },
    )
    .expect("the sample has a root");
    let gone: Vec<_> = diff(&before, &summary, DiffOptions::exact())
        .into_iter()
        .filter(|change| {
            matches!(
                change,
                Change::Field {
                    name: "attached",
                    ..
                } | Change::Fact {
                    name: "attached",
                    ..
                }
            ) && change_path(change) == [0]
        })
        .collect();
    assert!(
        matches!(
            gone.as_slice(),
            [Change::Fact {
                name: "attached",
                from: Some(OwnedValue::Bool(true)),
                to: None,
                ..
            }]
        ),
        "the fact is gone, the field is not: {gone:#?}"
    );
}

/// The path of a field or fact change.
fn change_path(change: &Change) -> &[usize] {
    match change {
        Change::Field { node, .. } | Change::Fact { node, .. } => &node.path,
        _ => &[],
    }
}

#[test]
fn the_diff_matches_children_by_id_and_reports_additions_and_removals() {
    let before = whole(&sample(8.0));
    let mut tree = sample(8.0);
    // r4 leaves; r6 arrives at its position, so r5 keeps its id-matched pairing.
    tree.nodes.remove(&render_id(4));
    tree.nodes.insert(
        render_id(6),
        (
            node("RenderImage", vec![], None),
            Some(Rect::from_xywh(200.0, 0.0, 40.0, 14.0)),
            Links::default(),
            vec![],
        ),
    );
    tree.nodes.get_mut(&render_id(1)).expect("the root").3 =
        vec![render_id(2), render_id(6), render_id(5)];
    let after = whole(&tree);
    let changes = diff(&before, &after, DiffOptions::default());
    assert_eq!(changes.len(), 2, "{changes:#?}");
    assert!(changes.iter().any(|change| matches!(
        change,
        Change::Removed(node) if node.id == render_id(4).into() && node.path == [1]
    )));
    assert!(changes.iter().any(|change| matches!(
        change,
        Change::Added(node) if node.id == render_id(6).into() && node.kind == "RenderImage"
    )));
}

#[test]
fn a_variant_field_reads_as_its_names() {
    struct Holder;
    impl Inspect for Holder {
        fn kind(&self) -> &'static str {
            "Holder"
        }

        fn inspect(&self, out: &mut Fields<'_>) {
            out.field("fit", &Fit::Cover);
        }
    }
    struct One;
    impl InspectTree for One {
        type Node = Holder;

        fn root(&self) -> Option<InspectId> {
            Some(render_id(1).into())
        }

        fn node(&self, _: InspectId) -> Option<NodeView<'_, Holder>> {
            Some(NodeView {
                object: &Holder,
                rect: None,
                links: Links::default(),
            })
        }

        fn children(&self, _: InspectId) -> impl Iterator<Item = InspectId> + '_ {
            core::iter::empty()
        }
    }
    let root = snapshot(&One, SnapshotQuery::default()).expect("one node");
    assert_eq!(
        root.get::<Variant<'_>>("fit"),
        Ok(Variant {
            ty: "Fit",
            name: "Cover"
        })
    );
}

/// Erased nodes name their kind from the type name: the last path segment, with its type
/// arguments reported apart.
#[test]
fn a_type_name_spells_its_kind_and_type_arguments() {
    let cases: [(&'static str, &str, &[&str]); 5] = [
        ("Padding", "Padding", &[]),
        ("flui_widgets::basic::Padding", "Padding", &[]),
        ("a::Builder<b::C>", "Builder", &["b::C"]),
        (
            "a::Map<b::K, c::V<d::W, e::X>>",
            "Map",
            &["b::K", "c::V<d::W, e::X>"],
        ),
        ("a::F<fn(u8) -> u8, b::T>", "F", &["fn(u8) -> u8", "b::T"]),
    ];
    for (type_name, kind, args) in cases {
        assert_eq!(kind_of(type_name), kind, "{type_name}");
        let mut seen = Vec::new();
        kind_args_of(type_name, &mut |arg| seen.push(arg));
        assert_eq!(seen, args, "{type_name}");
    }
}
