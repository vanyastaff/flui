//! Per-realm text contexts over one shared font collection (ADR-0092 §2–§3).
//!
//! Two contexts built from one [`FontCollection`] shape on two threads at
//! once, see a face registered after they were built, and never build the
//! cosmic-text path's process font system. Its own binary: nothing else in it
//! touches that font system, so the check holds under nextest and
//! `cargo test` alike.

use std::sync::Barrier;
use std::thread;
use std::time::Instant;

use flui_painting::parley_text::{ParagraphLayout, ParagraphSpec};
use flui_painting::testing::font_collection_holders;
use flui_painting::typography::{FontWeight, TextDirection, TextStyle};
use flui_painting::{FontCollection, TextContext, TextLayoutResult};

#[path = "support/cases.rs"]
mod cases;

const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
/// Every word is narrower than the widths the tests break at, so no line
/// overflows its width.
const LATIN: &str = "The quick brown fox jumps over the lazy dog and runs back home again.";

fn spec(spans: &[(String, Option<TextStyle>)], max_width: Option<f32>) -> ParagraphSpec<'_> {
    ParagraphSpec {
        spans,
        default_style: None,
        font_size: 16.0,
        max_width,
        line_height: None,
        direction: TextDirection::Ltr,
        max_lines: None,
    }
}

fn plain(text: &str) -> Vec<(String, Option<TextStyle>)> {
    vec![(text.to_owned(), None)]
}

fn shape(context: &mut TextContext, text: &str, max_width: Option<f32>) -> ParagraphLayout {
    context.shape(&spec(&plain(text), max_width))
}

/// The fields a shaped paragraph reports, as one comparable tuple.
fn key(metrics: &TextLayoutResult) -> (f32, f32, usize, f32, f32) {
    (
        ((metrics.width) as f32),
        ((metrics.height) as f32),
        metrics.line_count,
        ((metrics.alphabetic_baseline) as f32),
        ((metrics.ideographic_baseline) as f32),
    )
}

const fn assert_send<T: Send>() {}
const _: () = assert_send::<TextContext>();

/// Two realms' contexts, each moved to its own thread, shape at the same
/// time and agree with each other. On the cosmic-text path every shape takes
/// one process-wide lock; here each context is used through `&mut` and shares
/// no FLUI lock with the other. The overlap of the two threads' intervals
/// shows they really ran at once; that neither waited on the other rests on
/// the structure (no lock in the API, the crate's `disallowed_types` lint),
/// not on a timing measurement.
fn two_realms_shape_in_parallel() {
    const SHAPES: usize = 200;
    let fonts = FontCollection::new();
    let mut a = TextContext::new(&fonts);
    let b = TextContext::new(&fonts);
    let reference = key(&shape(&mut a, LATIN, Some(120.0)).metrics());
    assert!(
        reference.2 > 1,
        "the reference paragraph wraps: {reference:?}"
    );

    let barrier = Barrier::new(2);
    let intervals = thread::scope(|scope| {
        let workers: Vec<_> = [a, b]
            .into_iter()
            .map(|mut context| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let start = Instant::now();
                    for _ in 0..SHAPES {
                        let metrics = shape(&mut context, LATIN, Some(120.0)).metrics();
                        assert_eq!(key(&metrics), reference);
                    }
                    (start, Instant::now())
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a shaping thread panicked"))
            .collect::<Vec<_>>()
    });
    let [(start_a, end_a), (start_b, end_b)] = intervals[..] else {
        unreachable!("two workers")
    };
    assert!(
        start_a < end_b && start_b < end_a,
        "the two threads' shaping intervals overlap"
    );
}

/// A face registered on the collection after two contexts were built shapes
/// in both. The probe face maps only the space and `A`, each one em wide, so
/// four `A`s in it are exactly four em; Roboto, the fallback a context that
/// never saw the face shapes with, draws a narrower `A`.
fn a_face_registered_after_the_fork_shapes_in_every_realm() {
    const SIZE: f32 = 20.0;
    let fonts = FontCollection::new();
    let mut a = TextContext::new(&fonts);
    let mut b = TextContext::new(&fonts);
    let style = TextStyle {
        font_family: Some("FLUI Probe Mono".to_owned()),
        font_weight: Some(FontWeight::W100),
        font_size: Some(f64::from(SIZE)),
        ..TextStyle::default()
    };
    let spans = vec![("AAAA".to_owned(), Some(style))];
    let width = |context: &mut TextContext| {
        context
            .shape(&ParagraphSpec {
                font_size: SIZE,
                ..spec(&spans, None)
            })
            .metrics()
            .width
    };

    let before = width(&mut b);
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    for (name, context) in [("a", &mut a), ("b", &mut b)] {
        let after = width(context);
        assert!(
            (after - f64::from(4.0 * SIZE)).abs() < 0.01,
            "realm {name}: four one-em `A`s are {} px wide, got {after}",
            4.0 * SIZE
        );
    }
    assert!(
        (before - f64::from(4.0 * SIZE)).abs() > 1.0,
        "before registration the probe family falls back to Roboto, got {before}"
    );
}

/// A clone is the same collection and a new one is not; every clone and
/// every context built from it counts as a holder until it drops; bytes with
/// no face are refused.
fn collection_handles_are_shared_and_counted() {
    let fonts = FontCollection::new();
    assert!(FontCollection::ptr_eq(&fonts, &fonts.clone()));
    assert!(!FontCollection::ptr_eq(&fonts, &FontCollection::new()));

    assert_eq!(font_collection_holders(&fonts), 1);
    let context = TextContext::new(&fonts);
    assert!(FontCollection::ptr_eq(context.fonts(), &fonts));
    assert_eq!(font_collection_holders(&fonts), 2);
    drop(context);
    assert_eq!(font_collection_holders(&fonts), 1);

    assert!(fonts.register_font(b"not a font").is_err());
    assert!(fonts.register_font(&[]).is_err());
}

#[test]
fn text_context_contract() {
    cases::run_cases(
        "text_context",
        &[
            (
                "collection_handles_are_shared_and_counted",
                collection_handles_are_shared_and_counted,
            ),
            ("two_realms_shape_in_parallel", two_realms_shape_in_parallel),
            (
                "a_face_registered_after_the_fork_shapes_in_every_realm",
                a_face_registered_after_the_fork_shapes_in_every_realm,
            ),
        ],
    );
}
