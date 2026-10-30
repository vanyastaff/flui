//! The assistive technology's cursor, as the subtrees it is in: every handler
//! told `true` is told `false` once the cursor leaves its subtree, whatever
//! owner the tree is in by then.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_semantics::{AccessibilityNodeId, AssistiveFocus, AssistiveFocusHandler};

use crate::fixture::{Fixture, render_id};

type Log = Rc<RefCell<Vec<(&'static str, bool)>>>;

fn handler(log: &Log, name: &'static str) -> AssistiveFocusHandler {
    let log = Rc::clone(log);
    Rc::new(move |within| log.borrow_mut().push((name, within)))
}

fn node(index: u32) -> AccessibilityNodeId {
    AccessibilityNodeId::from(render_id(index, 5))
}

/// Root 1 (`root`) holds 2 (`list`), whose child 3 has no handler, and 4
/// (`aside`).
fn tree(handlers: &[AssistiveFocusHandler; 3]) -> Fixture {
    let [root, list, aside] = handlers.clone();
    let mut fixture = Fixture::new();
    fixture
        .add(None, 1, (0.0, 0.0, 100.0, 100.0), |c| {
            c.add_assistive_focus_handler(root);
        })
        .add(Some(1), 2, (0.0, 0.0, 100.0, 50.0), |c| {
            c.set_label("list");
            c.add_assistive_focus_handler(list);
        })
        .add(Some(2), 3, (0.0, 0.0, 100.0, 20.0), |c| c.set_label("item"))
        .add(Some(1), 4, (0.0, 50.0, 100.0, 50.0), |c| {
            c.set_label("aside");
            c.add_assistive_focus_handler(aside);
        });
    fixture.flush();
    fixture
}

/// Entering tells the subtrees entered, nearest first; moving tells the
/// subtrees left before those entered; leaving the tree tells every one.
#[test]
fn the_cursor_enters_and_leaves_subtrees() {
    let log: Log = Rc::default();
    let handlers = [
        handler(&log, "root"),
        handler(&log, "list"),
        handler(&log, "aside"),
    ];
    let fixture = tree(&handlers);
    let mut focus = AssistiveFocus::default();
    focus.move_to(Some(node(3)), Some(&fixture.owner)).invoke();
    assert_eq!(*log.borrow(), [("list", true), ("root", true)]);
    log.borrow_mut().clear();
    focus.move_to(Some(node(4)), Some(&fixture.owner)).invoke();
    assert_eq!(*log.borrow(), [("list", false), ("aside", true)]);
    log.borrow_mut().clear();
    let unchanged = focus.move_to(Some(node(4)), Some(&fixture.owner));
    assert!(unchanged.is_empty(), "the same place tells nobody");
    unchanged.invoke();
    focus.move_to(None, Some(&fixture.owner)).invoke();
    assert_eq!(*log.borrow(), [("aside", false), ("root", false)]);
    assert_eq!(focus.node(), None);
}

/// A cursor on a node the tree no longer holds, or with no tree at all, is
/// in no subtree: each handler told `true` hears `false`. A tree rebuilt in a
/// new owner with the same handlers tells nobody anything.
#[test]
fn every_entered_subtree_hears_it_is_left() {
    let log: Log = Rc::default();
    let handlers = [
        handler(&log, "root"),
        handler(&log, "list"),
        handler(&log, "aside"),
    ];
    let mut focus = AssistiveFocus::default();
    {
        let first = tree(&handlers);
        focus.move_to(Some(node(3)), Some(&first.owner)).invoke();
    }
    log.borrow_mut().clear();
    let rebuilt = tree(&handlers);
    let same = focus.move_to(Some(node(3)), Some(&rebuilt.owner));
    assert!(same.is_empty(), "the same handlers are already told");
    same.invoke();
    focus.move_to(Some(node(9)), Some(&rebuilt.owner)).invoke();
    assert_eq!(*log.borrow(), [("list", false), ("root", false)]);
    log.borrow_mut().clear();
    focus.move_to(Some(node(4)), Some(&rebuilt.owner)).invoke();
    focus.move_to(Some(node(4)), None).invoke();
    assert_eq!(
        *log.borrow(),
        [
            ("aside", true),
            ("root", true),
            ("aside", false),
            ("root", false)
        ]
    );
}

/// A handler that panics keeps no other from hearing the move, and the panic
/// reaches the caller once all have.
#[test]
fn a_panicking_handler_does_not_silence_the_others() {
    let log: Log = Rc::default();
    let failing: AssistiveFocusHandler = Rc::new(|_| panic!("the list failed"));
    let handlers = [handler(&log, "root"), failing, handler(&log, "aside")];
    let fixture = tree(&handlers);
    let mut focus = AssistiveFocus::default();
    let change = focus.move_to(Some(node(3)), Some(&fixture.owner));
    let outcome = catch_unwind(AssertUnwindSafe(|| change.invoke()));
    assert!(outcome.is_err(), "the panic is resumed");
    assert_eq!(*log.borrow(), [("root", true)], "the root still heard");
}
