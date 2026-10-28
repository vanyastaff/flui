//! Writes opened through a captured `WriterSource` (ADR-0086): a nested
//! `write` inside a live `EventCx` opens its own writer, applies at once and is
//! not a transaction with the outer one, and a panic inside it reaches the
//! outer callback with the writes before it still applied.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_view::{BuildOwner, SignalWriteExt};

pub(crate) fn nested_writer_source_inside_an_event_cx() {
    let owner = BuildOwner::new();
    let graph = owner.signal_graph();
    let source = owner.writer_source();
    let a = graph.signal(0u32);
    let b = graph.signal(0u32);

    let seen = source.write(|outer| {
        a.set(outer, 1).expect("outer write");
        let inner_saw = source.write(|inner| {
            let saw = a.peek(inner, |n| *n);
            b.set(inner, 2).expect("nested write");
            saw
        });
        (inner_saw, b.peek(outer, |n| *n))
    });

    assert_eq!(
        seen,
        (Ok(1), Ok(2)),
        "each write is visible to the next read, across the nesting"
    );
    assert_eq!(a.peek(graph, |n| *n), Ok(1));
    assert_eq!(b.peek(graph, |n| *n), Ok(2));
}

pub(crate) fn nested_write_panics_inside_an_event_cx() {
    let owner = BuildOwner::new();
    let graph = owner.signal_graph();
    let source = owner.writer_source();
    let a = graph.signal(0u32);
    let b = graph.signal(0u32);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        source.write(|outer| {
            a.set(outer, 1).expect("outer write");
            source.write(|inner| {
                b.set(inner, 2).expect("nested write");
                panic!("nested callback failure");
            });
        });
    }))
    .expect_err("the nested panic reaches the outer callback");

    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("nested callback failure"),
        "the nested panic is the outer callback's first panic"
    );
    assert_eq!(
        a.peek(graph, |n| *n),
        Ok(1),
        "the outer write stays applied"
    );
    assert_eq!(
        b.peek(graph, |n| *n),
        Ok(2),
        "the nested write stays applied"
    );
    assert_eq!(
        source.write(|cx| a.set(cx, 3)),
        Ok(()),
        "the source opens the next write"
    );
}
