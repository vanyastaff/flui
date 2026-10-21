//! Set position and size of lazy lists, read as an assistive technology reads
//! them: the position from the item, the size from the nearest container that
//! declares one ("item 12 of 100").

use std::num::{NonZeroU32, NonZeroUsize};
use std::rc::Rc;

use crate::common::{LaidOut, lay_out, tight};
use flui_testing::A11yTree;
use flui_testing::a11y::{A11ySortDirection, Role};
use flui_view::ViewExt;
use flui_view::element::{ItemCount, SemanticSetMapping};
use flui_widgets::prelude::*;
use flui_widgets::{
    CellPosition, CustomScrollView, IndexedSemantics, SemanticsRole, SetSize, SliverList,
    SortDirection, TableShape,
};

/// A row announcing itself as `label`.
fn labelled(label: String) -> flui_view::BoxedView {
    Semantics::new()
        .container(true)
        .label(label)
        .child(SizedBox::new(200.0, 48.0))
        .boxed()
}

/// A builder for `count` rows named `"{prefix}{index}"`.
fn rows(prefix: &'static str, count: usize) -> Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>> {
    Rc::new(move |index| (index < count).then(|| labelled(format!("{prefix}{index}"))))
}

/// A scroll view over the given slivers, tall enough to build every row.
fn laid_out(slivers: Vec<flui_view::BoxedView>) -> LaidOut {
    let mut laid = lay_out(CustomScrollView::new(slivers), tight(200.0, 480.0));
    laid.enable_semantics();
    laid.tick();
    laid.tick();
    laid
}

/// What a screen reader is told about each labelled row: its label, its
/// zero-based position, and the size of the set it reads from its container.
fn announced(tree: &A11yTree) -> Vec<(String, Option<usize>, Option<usize>)> {
    tree.nodes()
        .filter_map(|node| {
            let label = node.label()?;
            Some((
                label.to_owned(),
                node.position_in_set(),
                node.size_of_containing_set(),
            ))
        })
        .collect()
}

fn tree(laid: &LaidOut) -> A11yTree {
    laid.a11y_tree().expect("semantics are enabled")
}

/// "Item 3 of 5": the position is the item's and the size is its container's.
pub(crate) fn a_lazy_list_item_reads_its_position_and_its_containers_size() {
    let mut laid = lay_out(
        ListView::builder(5, 48.0, |i| (i < 5).then(|| labelled(format!("row{i}")))),
        tight(200.0, 480.0),
    );
    laid.enable_semantics();
    laid.tick();
    laid.tick();
    let tree = tree(&laid);

    let expected: Vec<_> = (0..5)
        .map(|i| (format!("row{i}"), Some(i), Some(5)))
        .collect();
    assert_eq!(announced(&tree), expected, "{}", tree.describe());
    let items = tree.find_all(Role::List);
    assert_eq!(items.len(), 1, "one container holds the set");
    assert_eq!(items[0].size_of_set(), Some(5));
    assert!(
        tree.nodes()
            .filter(|node| node.label().is_some())
            .all(|node| node.size_of_set().is_none()),
        "the size is declared by the container alone"
    );
}

/// A virtualized list publishes the whole set's size and the rows' true
/// positions, not the number of rows it happened to build.
pub(crate) fn a_virtualized_list_publishes_its_full_size_and_true_positions() {
    let mut laid = lay_out(
        ListView::builder(100, 48.0, |i| {
            (i < 100).then(|| labelled(format!("row{i}")))
        })
        .offset(48.0 * 40.0),
        tight(200.0, 144.0),
    );
    laid.enable_semantics();
    laid.tick();
    laid.tick();
    laid.tick();
    let tree = tree(&laid);
    let rows = announced(&tree);

    assert!(
        rows.len() < 20,
        "only the window around the offset is built: {rows:?}"
    );
    assert!(
        rows.iter().all(|(_, _, size)| *size == Some(100)),
        "every row reads the whole set's size: {rows:?}"
    );
    // Rows in the scroll cache beyond the viewport are hidden: the consumer
    // drops them, so only the rows on screen are held to a position.
    let mut positioned = 0;
    for (label, position, _) in &rows {
        let index: usize = label
            .trim_start_matches("row")
            .parse()
            .expect("a row label");
        if let Some(position) = position {
            assert_eq!(
                *position, index,
                "{label} is announced at its true place, not its place among the built"
            );
            positioned += 1;
        }
    }
    assert!(
        positioned >= 3,
        "the rows on screen are positioned: {rows:?}"
    );
    assert!(
        rows.iter().any(|(label, _, _)| label == "row40"),
        "the window sits around the offset: {rows:?}"
    );
}

/// A list whose end has not been found has no size to give: the container
/// publishes none, rather than the number of rows built, and the rows keep
/// their positions ("item 12").
pub(crate) fn an_unknown_length_publishes_no_size() {
    let laid = laid_out(vec![
        SliverList::new(ItemCount::Unknown, 48.0, rows("row", 5)).boxed(),
    ]);
    let tree = tree(&laid);

    let expected: Vec<_> = (0..5).map(|i| (format!("row{i}"), Some(i), None)).collect();
    assert_eq!(announced(&tree), expected, "{}", tree.describe());
}

/// Separators occupy logical indices without being members: the set is the
/// items alone, in a size that counts them alone, and a separator announces no
/// position of its own.
pub(crate) fn separators_are_not_members_of_the_set() {
    let laid = laid_out(vec![
        SliverList::separated(3, 48.0, rows("item", 3), rows("sep", 2)).boxed(),
    ]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![
            ("item0".to_owned(), Some(0), Some(3)),
            ("sep0".to_owned(), None, Some(3)),
            ("item1".to_owned(), Some(1), Some(3)),
            ("sep1".to_owned(), None, Some(3)),
            ("item2".to_owned(), Some(2), Some(3)),
        ],
        "{}",
        tree.describe()
    );
}

/// An unknown set inside a known one: the consumer would pair an inner
/// position with the nearest size above it, "item 12 of 2", so the outer
/// total is withheld and every row keeps its position ("item 12").
pub(crate) fn an_unknown_set_inside_a_known_one_withholds_the_outer_size() {
    let outer = ListView::builder(2, 192.0, |outer| {
        (outer < 2).then(|| {
            Semantics::new()
                .container(true)
                .label(format!("outer{outer}"))
                .child(
                    SizedBox::new(200.0, 192.0).child(CustomScrollView::new(vec![
                        SliverList::new(ItemCount::Unknown, 48.0, rows("inner", 3)).boxed(),
                    ])),
                )
                .boxed()
        })
    });
    let mut laid = lay_out(outer, tight(200.0, 400.0));
    laid.enable_semantics();
    laid.tick();
    laid.tick();
    laid.tick();
    let tree = tree(&laid);
    let rows = announced(&tree);

    let outer_rows: Vec<_> = rows.iter().filter(|r| r.0.starts_with("outer")).collect();
    assert_eq!(
        outer_rows.iter().map(|r| (r.1, r.2)).collect::<Vec<_>>(),
        vec![(Some(0), None), (Some(1), None)],
        "{}",
        tree.describe()
    );
    let inner_rows: Vec<_> = rows.iter().filter(|r| r.0.starts_with("inner")).collect();
    assert!(!inner_rows.is_empty(), "the inner rows are built: {rows:?}");
    assert!(
        inner_rows.iter().all(|r| r.1.is_some() && r.2.is_none()),
        "an inner row of unknown size keeps its position and reads no size: {inner_rows:?}"
    );
}

/// A known set keeps its positions under any outer set: the inner size is the
/// nearest the consumer finds.
pub(crate) fn a_known_set_inside_a_known_one_reads_its_own_size() {
    let outer = ListView::builder(2, 192.0, |outer| {
        (outer < 2).then(|| {
            Semantics::new()
                .container(true)
                .label(format!("outer{outer}"))
                .child(
                    SizedBox::new(200.0, 192.0).child(CustomScrollView::new(vec![
                        SliverList::new(3, 48.0, rows("inner", 3)).boxed(),
                    ])),
                )
                .boxed()
        })
    });
    let mut laid = lay_out(outer, tight(200.0, 400.0));
    laid.enable_semantics();
    laid.tick();
    laid.tick();
    laid.tick();
    let tree = tree(&laid);

    for (label, position, size) in announced(&tree)
        .into_iter()
        .filter(|r| r.0.starts_with("inner"))
    {
        let index: usize = label.trim_start_matches("inner").parse().expect("a row");
        assert_eq!(
            (position, size),
            (Some(index), Some(3)),
            "{label}: {}",
            tree.describe()
        );
    }
}

/// Two slivers one caller composes into a single set continue its positions
/// and read the whole set's size. They are two containers: equal sizes do not
/// make two lists one.
pub(crate) fn slivers_composed_into_one_set_continue_its_positions() {
    let first = SliverList::new(2, 48.0, rows("a", 2)).semantics(
        SemanticSetMapping::one_to_one(ItemCount::Exact(2)).composed_at(0, SetSize::Known(5)),
    );
    let second = SliverList::new(3, 48.0, rows("b", 3)).semantics(
        SemanticSetMapping::one_to_one(ItemCount::Exact(3)).composed_at(2, SetSize::Known(5)),
    );
    let laid = laid_out(vec![first.boxed(), second.boxed()]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![
            ("a0".to_owned(), Some(0), Some(5)),
            ("a1".to_owned(), Some(1), Some(5)),
            ("b0".to_owned(), Some(2), Some(5)),
            ("b1".to_owned(), Some(3), Some(5)),
            ("b2".to_owned(), Some(4), Some(5)),
        ],
        "{}",
        tree.describe()
    );
    assert_eq!(tree.find_all(Role::List).len(), 2, "{}", tree.describe());
}

/// Two slivers that are two sets are two containers, even when their sizes
/// agree: two lists of three rows read positions 0 to 2 twice, which one
/// container could not say.
pub(crate) fn two_sets_in_one_scroll_view_do_not_share_a_container() {
    let laid = laid_out(vec![
        SliverList::new(3, 48.0, rows("a", 3)).boxed(),
        SliverList::new(3, 48.0, rows("b", 3)).boxed(),
    ]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![
            ("a0".to_owned(), Some(0), Some(3)),
            ("a1".to_owned(), Some(1), Some(3)),
            ("a2".to_owned(), Some(2), Some(3)),
            ("b0".to_owned(), Some(0), Some(3)),
            ("b1".to_owned(), Some(1), Some(3)),
            ("b2".to_owned(), Some(2), Some(3)),
        ],
        "{}",
        tree.describe()
    );
    assert_eq!(
        tree.find_all(Role::List).len(),
        2,
        "two sets, two containers: {}",
        tree.describe()
    );
}

/// A row a caller numbers itself keeps its number: the stamped position is a
/// fallback for rows that declare none. Its numbering is another set's than the
/// delegate's, so the delegate's size is withheld rather than paired with it.
pub(crate) fn an_explicitly_indexed_row_keeps_its_own_position() {
    let builder: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>> = Rc::new(|index| {
        (index < 3).then(|| {
            // Inside the row's container: a non-boundary configuration is
            // absorbed by the nearest boundary above it.
            Semantics::new()
                .container(true)
                .label(format!("row{index}"))
                .child(IndexedSemantics::new(2 - index).child(SizedBox::new(200.0, 48.0)))
                .boxed()
        })
    });
    let laid = laid_out(vec![SliverList::new(3, 48.0, builder).boxed()]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![
            ("row0".to_owned(), Some(2), None),
            ("row1".to_owned(), Some(1), None),
            ("row2".to_owned(), Some(0), None),
        ],
        "{}",
        tree.describe()
    );
}

/// A one-based row, column or span.
fn nz(number: usize) -> NonZeroUsize {
    NonZeroUsize::new(number).expect("BUG: a fixture number is not zero")
}

/// A node in a table: a role, a name and a place.
fn table_node(role: SemanticsRole, label: &str, place: Semantics) -> flui_view::BoxedView {
    place
        .container(true)
        .role(role)
        .label(label)
        .child(SizedBox::new(60.0, 24.0))
        .boxed()
}

fn find<'a>(tree: &'a A11yTree, label: &str) -> flui_testing::a11y::A11yNode<'a> {
    tree.find_by_label(label)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// A table publishes its shape, its cells their places and spans, and a header
/// its sort. A cell's row is its true place in the table, whatever rows are
/// built: a virtualized table that built its 4000th row says so (AccessKit holds
/// it zero-based, as 3999).
pub(crate) fn a_table_publishes_its_shape_cells_and_sort() {
    use SemanticsRole as R;
    let table = Semantics::new()
        .container(true)
        .role(R::Table)
        .label("Orders")
        .table_shape(TableShape::new(SetSize::Known(4000), SetSize::Known(3)))
        .child(Column::new(vec![
            table_node(
                R::ColumnHeader,
                "Total",
                Semantics::new()
                    .cell_position(CellPosition::new(nz(1), nz(3)))
                    .sort_direction(SortDirection::Descending),
            ),
            table_node(
                R::Cell,
                "7",
                Semantics::new().cell_position(CellPosition::new(nz(4000), nz(3))),
            ),
            table_node(
                R::Cell,
                "total of two columns",
                Semantics::new().cell_position(
                    CellPosition::new(nz(4000), nz(1)).spanning(NonZeroUsize::MIN, nz(2)),
                ),
            ),
        ]));
    let mut laid = lay_out(table, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let table = find(&tree, "Orders");
    assert_eq!(
        (table.row_count(), table.column_count()),
        (Some(4000), Some(3)),
        "{}",
        tree.describe()
    );
    let header = find(&tree, "Total");
    assert_eq!(
        (
            header.row_index(),
            header.column_index(),
            header.sort_direction()
        ),
        (Some(0), Some(2), Some(A11ySortDirection::Descending))
    );
    let cell = find(&tree, "7");
    assert_eq!(
        (cell.row_index(), cell.column_index()),
        (Some(3999), Some(2))
    );
    assert_eq!((cell.row_span(), cell.column_span()), (None, None));
    let wide = find(&tree, "total of two columns");
    assert_eq!((wide.row_span(), wide.column_span()), (None, Some(2)));
}

/// A table that does not know its row count publishes none, rather than the
/// number of rows built.
pub(crate) fn a_table_of_unknown_length_publishes_no_row_count() {
    let table = Semantics::new()
        .container(true)
        .role(SemanticsRole::Grid)
        .label("Feed")
        .table_shape(TableShape::new(SetSize::Unknown, SetSize::Known(2)))
        .child(SizedBox::new(60.0, 24.0));
    let mut laid = lay_out(table, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let grid = find(&tree, "Feed");
    assert_eq!(
        (grid.row_count(), grid.column_count()),
        (None, Some(2)),
        "{}",
        tree.describe()
    );
}

/// A tree item publishes its depth, one-based as ARIA gives it, and a tree
/// whether it selects many.
pub(crate) fn a_tree_publishes_its_levels_and_selection_rule() {
    use SemanticsRole as R;
    let tree_view = Semantics::new()
        .container(true)
        .role(R::Tree)
        .label("Files")
        .multiselectable(true)
        .child(Column::new(vec![
            table_node(
                R::TreeItem,
                "src",
                Semantics::new()
                    .level(NonZeroU32::MIN)
                    .expanded(true)
                    .position_in_set(0),
            ),
            table_node(
                R::TreeItem,
                "main.rs",
                Semantics::new()
                    .level(NonZeroU32::new(2).unwrap())
                    .position_in_set(0),
            ),
        ]));
    let mut laid = lay_out(tree_view, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    assert!(find(&tree, "Files").is_multiselectable());
    assert_eq!(find(&tree, "src").outline_level(), Some(1));
    assert_eq!(find(&tree, "main.rs").outline_level(), Some(2));
}

/// A caller that owns a set declares its size on the container and each
/// member's place itself; the rows read "of 3" from the container.
pub(crate) fn a_hand_built_list_declares_its_size_and_positions() {
    use SemanticsRole as R;
    let list = Semantics::new()
        .container(true)
        .role(R::List)
        .label("Steps")
        .size_of_set(SetSize::Known(3))
        .child(Column::new(
            (0..3)
                .map(|i| {
                    table_node(
                        R::ListItem,
                        &format!("step{i}"),
                        Semantics::new().position_in_set(i),
                    )
                })
                .collect::<Vec<_>>(),
        ));
    let mut laid = lay_out(list, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    for i in 0..3 {
        let step = find(&tree, &format!("step{i}"));
        assert_eq!(
            (step.position_in_set(), step.size_of_containing_set()),
            (Some(i), Some(3)),
            "{}",
            tree.describe()
        );
    }
}

/// A position beyond the set it sits in would be announced as "item 8 of 3":
/// plausible and false. It is not published; a member that fits keeps its own.
pub(crate) fn a_position_beyond_its_set_is_not_announced() {
    let list = Semantics::new()
        .container(true)
        .role(SemanticsRole::List)
        .label("Steps")
        .size_of_set(SetSize::Known(3))
        .child(Column::new(vec![
            table_node(
                SemanticsRole::ListItem,
                "inside",
                Semantics::new().position_in_set(1),
            ),
            table_node(
                SemanticsRole::ListItem,
                "outside",
                Semantics::new().position_in_set(7),
            ),
        ]));
    let mut laid = lay_out(list, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    assert_eq!(find(&tree, "inside").position_in_set(), Some(1));
    assert_eq!(
        find(&tree, "outside").position_in_set(),
        None,
        "{}",
        tree.describe()
    );
}

/// A set a caller declares unknown inside one declared known is held to the
/// same rule as a lazy one: the consumer would walk past it to the outer size,
/// so that size is withheld and the member keeps its position.
pub(crate) fn a_declared_unknown_set_inside_a_known_one_withholds_the_outer_size() {
    let inner = Semantics::new()
        .container(true)
        .role(SemanticsRole::List)
        .label("Inner")
        .size_of_set(SetSize::Unknown)
        .child(Column::new(vec![table_node(
            SemanticsRole::ListItem,
            "unsized member",
            Semantics::new().position_in_set(0),
        )]));
    let outer = Semantics::new()
        .container(true)
        .role(SemanticsRole::List)
        .label("Outer")
        .size_of_set(SetSize::Known(2))
        .child(inner);
    let mut laid = lay_out(outer, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let member = find(&tree, "unsized member");
    assert_eq!(
        (member.position_in_set(), member.size_of_containing_set()),
        (Some(0), None),
        "{}",
        tree.describe()
    );
}

/// A window of a virtualized list can hold no member at all, a long run of
/// headers say. The set is still the size its mapping declares, and its
/// container still exists.
pub(crate) fn a_window_with_no_member_resident_keeps_the_sets_size() {
    let sliver = SliverList::new(10, 48.0, rows("header", 10))
        .semantics(SemanticSetMapping::interleaved(Rc::new(|_| None), 40));
    let laid = laid_out(vec![sliver.boxed()]);
    let tree = tree(&laid);

    let lists = tree.find_all(Role::List);
    assert_eq!(lists.len(), 1, "{}", tree.describe());
    assert_eq!(lists[0].size_of_set(), Some(40), "{}", tree.describe());
    assert!(
        announced(&tree)
            .iter()
            .all(|(_, position, _)| position.is_none()),
        "no header is a member: {}",
        tree.describe()
    );
}

/// A row publishes its own index, one-based for the author and zero-based in
/// AccessKit, without a column or a span.
pub(crate) fn a_row_publishes_its_own_index() {
    let row = table_node(
        SemanticsRole::Row,
        "Order",
        Semantics::new().row_index(nz(4000)),
    );
    let mut laid = lay_out(row, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let order = find(&tree, "Order");
    assert_eq!(
        (order.row_index(), order.column_index()),
        (Some(3999), None),
        "{}",
        tree.describe()
    );
}

/// A row that forms several nodes is still one member: its position goes to
/// the first, not to every control in it, which would announce two buttons as
/// "item 1".
pub(crate) fn a_row_of_several_controls_is_one_member() {
    let laid = {
        let mut laid = lay_out(
            ListView::builder(2, 96.0, |index| {
                (index < 2).then(|| {
                    SizedBox::new(200.0, 96.0)
                        .child(Column::new(vec![
                            table_node(
                                SemanticsRole::Button,
                                &format!("first{index}"),
                                Semantics::new(),
                            ),
                            table_node(
                                SemanticsRole::Button,
                                &format!("second{index}"),
                                Semantics::new(),
                            ),
                        ]))
                        .boxed()
                })
            }),
            tight(200.0, 400.0),
        );
        laid.enable_semantics();
        laid.tick();
        laid.tick();
        laid
    };
    let tree = tree(&laid);

    for index in 0..2 {
        assert_eq!(
            (
                find(&tree, &format!("first{index}")).position_in_set(),
                find(&tree, &format!("second{index}")).position_in_set(),
            ),
            (Some(index), None),
            "{}",
            tree.describe()
        );
    }
}

/// A list's only member is a node of its own below the container, not folded
/// into it: the consumer reads the size from the member's ancestors, so a
/// member merged into its container would read no "of 1".
pub(crate) fn a_lone_member_sits_below_its_container() {
    // A label and no boundary of its own, a plain `Text` say: only the
    // container's directive keeps it from being absorbed by the list.
    let only: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>> = Rc::new(|index| {
        (index < 1).then(|| {
            Semantics::new()
                .label("only0")
                .child(SizedBox::new(200.0, 48.0))
                .boxed()
        })
    });
    let laid = laid_out(vec![SliverList::new(1, 48.0, only).boxed()]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![("only0".to_owned(), Some(0), Some(1))],
        "{}",
        tree.describe()
    );
    let lists = tree.find_all(Role::List);
    assert_eq!(lists.len(), 1, "{}", tree.describe());
    assert_eq!(
        lists[0].label(),
        None,
        "the member is not merged into its container"
    );
}

/// An explicit position anywhere in an item is that item's member, a
/// descendant that forms its own node included: the row above it is not
/// stamped as a second member of the same item.
pub(crate) fn an_explicit_position_in_a_descendant_suppresses_the_stamp() {
    let builder: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>> = Rc::new(|index| {
        (index < 2).then(|| {
            Semantics::new()
                .container(true)
                .label(format!("row{index}"))
                .child(
                    Semantics::new()
                        .container(true)
                        .label(format!("inner{index}"))
                        .child(IndexedSemantics::new(5 + index).child(SizedBox::new(200.0, 48.0))),
                )
                .boxed()
        })
    });
    let laid = laid_out(vec![SliverList::new(2, 48.0, builder).boxed()]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        vec![
            ("row0".to_owned(), None, None),
            ("inner0".to_owned(), Some(5), None),
            ("row1".to_owned(), None, None),
            ("inner1".to_owned(), Some(6), None),
        ],
        "{}",
        tree.describe()
    );
}

/// A cell, a span or a row outside the table it sits in would read "row 5 of
/// 3": it is not published, and the cells inside keep theirs.
pub(crate) fn a_cell_beyond_its_table_is_not_announced() {
    use SemanticsRole as R;
    let table = Semantics::new()
        .container(true)
        .role(R::Table)
        .label("Grid")
        .table_shape(TableShape::new(SetSize::Known(3), SetSize::Known(3)))
        .child(Column::new(vec![
            table_node(
                R::Cell,
                "inside",
                Semantics::new().cell_position(CellPosition::new(nz(2), nz(2))),
            ),
            table_node(
                R::Cell,
                "below",
                Semantics::new().cell_position(CellPosition::new(nz(5), nz(1))),
            ),
            table_node(
                R::Cell,
                "overhang",
                Semantics::new()
                    .cell_position(CellPosition::new(nz(1), nz(3)).spanning(nz(1), nz(2))),
            ),
            table_node(R::Row, "fourth row", Semantics::new().row_index(nz(4))),
            table_node(R::Row, "third row", Semantics::new().row_index(nz(3))),
        ]));
    let mut laid = lay_out(table, tight(200.0, 300.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    assert_eq!(find(&tree, "inside").row_index(), Some(1));
    assert_eq!(
        find(&tree, "below").row_index(),
        None,
        "{}",
        tree.describe()
    );
    assert_eq!(find(&tree, "overhang").column_index(), None);
    assert_eq!(find(&tree, "fourth row").row_index(), None);
    assert_eq!(find(&tree, "third row").row_index(), Some(2));
}

/// A size the adapters cannot convert is not published, so the set it sizes
/// is, to the consumer, one of no size: it is held to the rule of an unknown
/// set, or its members would read the outer total.
pub(crate) fn a_size_the_adapters_cannot_convert_counts_as_unknown() {
    let inner = Semantics::new()
        .container(true)
        .role(SemanticsRole::List)
        .label("Huge")
        .size_of_set(SetSize::Known(usize::MAX))
        .child(Column::new(vec![table_node(
            SemanticsRole::ListItem,
            "huge member",
            Semantics::new().position_in_set(10),
        )]));
    let outer = Semantics::new()
        .container(true)
        .role(SemanticsRole::List)
        .label("Small")
        .size_of_set(SetSize::Known(5))
        .child(inner);
    let mut laid = lay_out(outer, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let member = find(&tree, "huge member");
    assert_eq!(
        (member.position_in_set(), member.size_of_containing_set()),
        (Some(10), None),
        "{}",
        tree.describe()
    );
}

/// A position the adapters cannot convert is never stored, so it neither
/// reaches AccessKit nor counts as a member of another set: the list keeps
/// its size, and the row falls back to the position its host stamped.
pub(crate) fn an_unpublishable_position_withholds_no_total() {
    let builder: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>> = Rc::new(|index| {
        (index < 3).then(|| {
            let body = SizedBox::new(200.0, 48.0);
            let row = Semantics::new()
                .container(true)
                .label(format!("row{index}"));
            if index == 1 {
                row.child(IndexedSemantics::new(i32::MAX as usize).child(body))
                    .boxed()
            } else {
                row.child(body).boxed()
            }
        })
    });
    let laid = laid_out(vec![SliverList::new(3, 48.0, builder).boxed()]);
    let tree = tree(&laid);

    assert_eq!(
        announced(&tree),
        (0..3)
            .map(|i| (format!("row{i}"), Some(i), Some(3)))
            .collect::<Vec<_>>(),
        "{}",
        tree.describe()
    );
}

/// A set declared by size alone, with no role and no `container`, is still a
/// node of its own above its members: a lone member folded into it would read
/// no "of 1".
pub(crate) fn a_set_declared_by_size_keeps_its_lone_member() {
    let set = Semantics::new().size_of_set(SetSize::Known(1)).child(
        Semantics::new()
            .role(SemanticsRole::ListItem)
            .label("lone")
            .position_in_set(0)
            .child(SizedBox::new(200.0, 48.0)),
    );
    let mut laid = lay_out(set, tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = tree(&laid);

    let member = find(&tree, "lone");
    assert_eq!(
        (member.position_in_set(), member.size_of_containing_set()),
        (Some(0), Some(1)),
        "{}",
        tree.describe()
    );
}
