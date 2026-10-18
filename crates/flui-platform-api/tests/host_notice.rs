//! A session end waits for as long as anyone holds a block on its request;
//! dropping the last block releases it, once, and never panics.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform_api::SessionEndRequest;

fn an_unblocked_request_waits_for_nothing() {
    let request = SessionEndRequest::new();
    assert!(!request.is_blocked());
    assert_eq!(request.reasons(), Vec::<String>::new());
}

fn a_block_holds_until_dropped_and_names_its_reason() {
    let request = SessionEndRequest::new();
    let block = request.block("unsaved document");
    assert!(request.is_blocked());
    assert_eq!(request.reasons(), ["unsaved document"]);
    drop(block);
    assert!(!request.is_blocked());
}

fn releasing_a_block_by_name_is_dropping_it() {
    let request = SessionEndRequest::new();
    request.block("x").release();
    assert!(!request.is_blocked());
}

fn blocks_on_clones_of_a_request_add_up() {
    let request = SessionEndRequest::new();
    let seen_by_another_presentation = request.clone();
    let first = request.block("first");
    let second = seen_by_another_presentation.block("second");
    assert_eq!(request.reasons(), ["first", "second"]);
    drop(first);
    assert!(
        seen_by_another_presentation.is_blocked(),
        "one block remains on the shared request"
    );
    drop(second);
    assert!(!request.is_blocked());
}

fn the_last_block_runs_the_release_hook_once() {
    let released = Rc::new(Cell::new(0));
    let counted = Rc::clone(&released);
    let request = SessionEndRequest::new().releasing(move || counted.set(counted.get() + 1));
    let first = request.block("a");
    let second = request.block("b");
    drop(first);
    assert_eq!(released.get(), 0, "not while a block remains");
    drop(second);
    assert_eq!(released.get(), 1);
    drop(request.block("c"));
    assert_eq!(released.get(), 1, "the hook is spent");
}

fn a_panicking_release_hook_does_not_escape_the_drop() {
    let request = SessionEndRequest::new().releasing(|| panic!("the release hook fails"));
    let block = request.block("a");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(block)));
    assert!(outcome.is_ok(), "dropping a block never panics");
    assert!(
        !request.is_blocked(),
        "and the block is released regardless"
    );
}

#[test]
fn session_end_request() {
    crate::run_table(
        "session_end_request",
        &[
            (
                "an_unblocked_request_waits_for_nothing",
                an_unblocked_request_waits_for_nothing as fn(),
            ),
            (
                "a_block_holds_until_dropped_and_names_its_reason",
                a_block_holds_until_dropped_and_names_its_reason as fn(),
            ),
            (
                "releasing_a_block_by_name_is_dropping_it",
                releasing_a_block_by_name_is_dropping_it as fn(),
            ),
            (
                "blocks_on_clones_of_a_request_add_up",
                blocks_on_clones_of_a_request_add_up as fn(),
            ),
            (
                "the_last_block_runs_the_release_hook_once",
                the_last_block_runs_the_release_hook_once as fn(),
            ),
            (
                "a_panicking_release_hook_does_not_escape_the_drop",
                a_panicking_release_hook_does_not_escape_the_drop as fn(),
            ),
        ],
    );
}
