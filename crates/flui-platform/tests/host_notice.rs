//! A session end waits for as long as anyone holds a block on its request;
//! the host closes the request's book when it answers, and dropping the last
//! block after that releases it, once, and never panics.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::{SessionEndAnswer, SessionEndBlockError, SessionEndRequest};

fn an_unblocked_request_waits_for_nothing() {
    let request = SessionEndRequest::new();
    assert!(!request.is_blocked());
    assert_eq!(request.reasons(), Vec::<String>::new());
    assert_eq!(request.answer(), SessionEndAnswer::Proceed);
}

fn a_block_holds_until_dropped_and_names_its_reason() {
    let request = SessionEndRequest::new();
    let block = request.block("unsaved document").expect("open book");
    assert!(request.is_blocked());
    assert_eq!(request.reasons(), ["unsaved document"]);
    drop(block);
    assert!(!request.is_blocked());
}

fn releasing_a_block_by_name_is_dropping_it() {
    let request = SessionEndRequest::new();
    request.block("x").expect("open book").release();
    assert!(!request.is_blocked());
}

fn blocks_on_clones_of_a_request_add_up() {
    let request = SessionEndRequest::new();
    let seen_by_another_presentation = request.clone();
    let first = request.block("first").expect("open book");
    let second = seen_by_another_presentation
        .block("second")
        .expect("open book");
    assert_eq!(request.reasons(), ["first", "second"]);
    drop(first);
    assert!(
        seen_by_another_presentation.is_blocked(),
        "one block remains on the shared request"
    );
    drop(second);
    assert!(!request.is_blocked());
}

fn answering_reports_the_blocks_and_closes_the_book() {
    let request = SessionEndRequest::new();
    let block = request.block("save").expect("open book");
    assert_eq!(request.answer(), SessionEndAnswer::Block);
    assert_eq!(
        request.block("too late").map(|_| ()),
        Err(SessionEndBlockError::Answered),
        "a block after the answer could not have changed it"
    );
    assert_eq!(
        request.reasons(),
        ["save"],
        "the refused block holds nothing"
    );
    drop(block);

    let unblocked = SessionEndRequest::new();
    assert_eq!(unblocked.answer(), SessionEndAnswer::Proceed);
    assert_eq!(
        unblocked.block("late").map(|_| ()),
        Err(SessionEndBlockError::Answered),
        "a Proceed answer closes the book as well"
    );
}

fn the_last_block_runs_the_release_hook_once_after_the_answer() {
    let released = Rc::new(Cell::new(0));
    let counted = Rc::clone(&released);
    let request = SessionEndRequest::new().on_release(move || counted.set(counted.get() + 1));
    let first = request.block("a").expect("open book");
    let second = request.block("b").expect("open book");
    assert_eq!(request.answer(), SessionEndAnswer::Block);
    drop(first);
    assert_eq!(released.get(), 0, "not while a block remains");
    drop(second);
    assert_eq!(released.get(), 1);
}

fn a_block_dropped_before_the_answer_releases_nothing_the_platform_was_told() {
    let released = Rc::new(Cell::new(0));
    let counted = Rc::clone(&released);
    let request = SessionEndRequest::new().on_release(move || counted.set(counted.get() + 1));
    drop(request.block("changed its mind").expect("open book"));
    assert_eq!(
        released.get(),
        0,
        "the platform has been told nothing yet, so there is nothing to undo"
    );
    assert_eq!(request.answer(), SessionEndAnswer::Proceed);
}

fn a_panicking_release_hook_does_not_escape_the_drop() {
    let request = SessionEndRequest::new().on_release(|| panic!("the release hook fails"));
    let block = request.block("a").expect("open book");
    request.answer();
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
                "answering_reports_the_blocks_and_closes_the_book",
                answering_reports_the_blocks_and_closes_the_book as fn(),
            ),
            (
                "the_last_block_runs_the_release_hook_once_after_the_answer",
                the_last_block_runs_the_release_hook_once_after_the_answer as fn(),
            ),
            (
                "a_block_dropped_before_the_answer_releases_nothing_the_platform_was_told",
                a_block_dropped_before_the_answer_releases_nothing_the_platform_was_told as fn(),
            ),
            (
                "a_panicking_release_hook_does_not_escape_the_drop",
                a_panicking_release_hook_does_not_escape_the_drop as fn(),
            ),
        ],
    );
}
