//! What text costs before the first frame and while it is held
//! (ADR-0092 gate 6).
//!
//! Three timings, then two heap figures:
//!
//! - building the bundled-only collection, scanning the host's fonts, and
//!   feeding a collection from that scan (criterion, 10 samples each);
//! - the app's start-up path: what the owner thread pays to build the
//!   collection and its host feed, and what the feed costs on its own
//!   thread once the host is scanned;
//! - the heap a host-fed collection keeps;
//! - the heap one laid-out `TextPainter` keeps, per paragraph shape: 1,000
//!   painters laid out and held, the live-bytes delta divided by 1,000.
//!
//! Heap figures come from the counting global allocator, on the bench's own
//! thread: live bytes are allocations minus deallocations. Run with
//! `cargo bench -p flui-painting --bench text_startup`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion};
use flui_foundation::TextDirection;
use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};
use flui_painting::testing::feed_with_host;
use flui_painting::typography::TextSpan;
use flui_painting::{FontCollection, HostFonts, TextContext, TextPainter};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// This thread's live heap bytes, wrapping: only the difference of two readings means anything.
fn live() -> i64 {
    HeapCounts::now().live_bytes.cast_signed()
}

/// Bytes as mebibytes, for the report.
#[expect(clippy::cast_precision_loss, reason = "a report figure")]
fn mib(bytes: i64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

const LABEL: &str = "Save changes";
const SENTENCE: &str = "The quick brown fox jumps over the lazy dog, twice a day.";
/// The removed text spike's `arabic_mixed` corpus.
const ARABIC_MIXED: &str = "مرحبا! This sentence mixes Arabic نص عربي with English text, the way a \
bidirectional paragraph often does in real UI content. A field like \"الاسم: John Smith\" or a \
price like \"المبلغ: 42.50 USD\" forces a text stack to resolve visual order (UAX #9) correctly, \
not just shape each run in isolation. FLUI's own hit-testing and caret placement have to agree \
with this visual order, run boundary by run boundary.";
/// The removed text spike's `cjk` corpus.
const CJK: &str = "快速的棕色狐狸跳过了懒狗。文字排版是任何用户界面的基础。こんにちは、これは日本語のテストです。\
文字の整形とレイアウトは、ユーザーインターフェースの基盤です。한국어 텍스트도 마찬가지로 중요합니다. \
텍스트 셰이핑과 레이아웃은 모든 사용자 인터페이스의 기초입니다.";

const PAINTERS: usize = 1_000;
const WIDTH: f64 = 400.0;

fn painter(text: &str) -> TextPainter {
    TextPainter::new()
        .with_text(TextSpan::new(text))
        .with_text_direction(TextDirection::Ltr)
}

/// Live bytes one laid-out painter of `text` keeps, averaged over
/// [`PAINTERS`] held at once. One painter is laid out first, so the
/// context's own caches are warm and not counted.
fn per_painter(context: &mut TextContext, text: &str) -> i64 {
    painter(text)
        .layout(context, 0.0, WIDTH)
        .expect("benchmark text has valid layout inputs");
    let before = live();
    let mut held = Vec::with_capacity(PAINTERS);
    let vec_bytes = live() - before;
    for _ in 0..PAINTERS {
        let mut painter = painter(text);
        painter
            .layout(context, 0.0, WIDTH)
            .expect("benchmark text has valid layout inputs");
        held.push(painter);
    }
    let after = live();
    drop(black_box(held));
    (after - before - vec_bytes) / i64::try_from(PAINTERS).expect("small")
}

fn main() {
    let mut criterion = Criterion::default().sample_size(10).configure_from_args();

    criterion.bench_function("FontCollection::new", |b| {
        b.iter(|| black_box(FontCollection::new()));
    });
    criterion.bench_function("HostFonts::scan", |b| {
        b.iter(|| black_box(HostFonts::scan()));
    });
    let host = HostFonts::scan();
    criterion.bench_function("FontCollection::with_host_fonts", |b| {
        b.iter(|| black_box(FontCollection::with_host_fonts(&host)));
    });
    criterion.bench_function("FontCollection::with_host_feed", |b| {
        b.iter(|| black_box(FontCollection::with_host_feed()));
    });
    criterion.bench_function("HostFontFeed::run", |b| {
        b.iter_batched(
            || {
                let (fonts, feed) = FontCollection::with_host_feed();
                (fonts, feed_with_host(feed, HostFonts::scan()))
            },
            |(fonts, feed)| {
                feed.run();
                fonts
            },
            BatchSize::PerIteration,
        );
    });
    criterion.final_summary();

    let before = live();
    let fonts = FontCollection::with_host_fonts(&host);
    let collection = live() - before;
    let mut context = TextContext::new(&fonts);
    println!();
    println!(
        "heap kept by a host-fed collection: {:.2} MiB",
        mib(collection)
    );
    for (name, text) in [
        ("12-char label", LABEL),
        ("57-char sentence", SENTENCE),
        ("570-char paragraph", &SENTENCE.repeat(10)),
        ("arabic_mixed", ARABIC_MIXED),
        ("cjk", CJK),
    ] {
        let bytes = per_painter(&mut context, text);
        println!(
            "heap kept per laid-out painter, {name} ({} chars): {bytes} B",
            text.chars().count()
        );
    }
}
