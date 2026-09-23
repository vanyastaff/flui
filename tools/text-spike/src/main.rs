//! B7 research spike: parley vs cosmic-text shaping/layout, standalone
//! from the flui workspace (see `Cargo.toml`'s `[workspace]` table and
//! `.rust-studio/specs/b7-text-stack-spike/plan.md`).
//!
//! Prints one JSON measurement per invocation; `docs/research/text-stack-2026.md`
//! records the exact commands run to produce its measurement table.
//!
//! `init_rss_bytes` and `peak_rss_bytes` are sampled separately (right
//! after backend construction, and again after shaping) so construction
//! cost and shaping-attributable growth can at least be told apart --
//! `getrusage`'s `ru_maxrss` is a process-lifetime high-water mark, so
//! `peak_rss_bytes - init_rss_bytes` is the growth in that mark between
//! the two samples, not a precise "bytes retained by shaping" figure: if
//! construction itself has an internal transient spike above its own
//! settled footprint, `init_rss_bytes` already reflects that spike (the
//! mark can't fall back down), which can understate the later delta by
//! an unknown amount. Treat the delta as an upper-bound-shaped proxy, not
//! an exact attribution (see `docs/research/text-stack-2026.md`'s
//! "Memory" section for the caveat this motivates).
//!
//! Both backends retain their shaped state until after both samples are
//! taken, on both axes this file measures:
//! - **Memory**: `--backend parley --parley-mode per-paragraph` keeps
//!   every built `Layout` alive (`ParleyBackend::shape_per_paragraph_retained`)
//!   until after `peak_rss_bytes()` is sampled, matching cosmic-text's
//!   single `Buffer`, which already holds every paragraph's shaped state
//!   at once by construction -- otherwise a build-and-drop-per-paragraph
//!   loop would only ever show ~1 paragraph's footprint.
//! - **Timing**: both backends' `shape_retained` variants return their
//!   shaped state (a `Buffer` or `Layout`/`Vec<Layout>`) instead of
//!   dropping it internally, so destruction happens AFTER `elapsed_ms` is
//!   sampled on both sides. Without this, `CosmicBackend::shape`'s local
//!   `Buffer` drops before returning -- inside the timed window -- while
//!   parley's returned, still-alive state doesn't, so cosmic-text's
//!   `elapsed_ms` would silently include deallocating hundreds of MB that
//!   parley's measurement excluded (a Codex review finding).

mod corpora;
mod cosmic_backend;
mod metrics;
mod parley_backend;

use clap::{Parser, ValueEnum};
use serde::Serialize;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Backend {
    Parley,
    #[value(name = "cosmic-text")]
    CosmicText,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Size {
    Paragraph,
    Large,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Cache {
    Cold,
    Warm,
}

/// Only meaningful for `--backend parley` -- see `parley_backend`'s module
/// docs for why these two are not equivalent. `PerParagraph` is the
/// realistic UI usage and the default; `Single` is kept only as an
/// explicitly-labeled "not a UI scenario" data point.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ParleyMode {
    PerParagraph,
    Single,
}

/// Shared measurement output every backend produces, so the two shaping
/// paths stay comparable on the same axes.
pub struct ShapeResult {
    pub line_count: usize,
    pub glyph_count: usize,
}

#[derive(Parser, Debug)]
#[command(
    name = "text-spike",
    about = "B7 spike: parley vs cosmic-text shaping/layout"
)]
struct Args {
    #[arg(long, value_enum)]
    backend: Backend,
    /// One of: latin, arabic, arabic_mixed, cjk, emoji_zwj, devanagari.
    #[arg(long)]
    corpus: String,
    #[arg(long, value_enum, default_value = "paragraph")]
    size: Size,
    /// "warm" shapes once to populate the backend's own caches, discards
    /// that timing, then shapes again and reports the second timing.
    #[arg(long, value_enum, default_value = "cold")]
    cache: Cache,
    #[arg(long, default_value_t = 10_000)]
    large_lines: usize,
    #[arg(long)]
    max_width: Option<f32>,
    /// Ignored for `--backend cosmic-text` (its `Buffer` already shapes
    /// each line independently).
    #[arg(long, value_enum, default_value = "per-paragraph")]
    parley_mode: ParleyMode,
}

#[derive(Serialize)]
struct Report {
    backend: String,
    corpus: String,
    size: String,
    cache: String,
    parley_mode: Option<String>,
    elapsed_ms: f64,
    /// RSS immediately after constructing the backend, before any
    /// shaping -- see module docs for why this isn't a precise
    /// "construction cost" figure on its own.
    init_rss_bytes: u64,
    /// RSS after shaping, with all shaped state still retained -- the
    /// process-lifetime high-water mark, so this is >= `init_rss_bytes`.
    peak_rss_bytes: u64,
    line_count: usize,
    glyph_count: usize,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    if !corpora::CORPUS_NAMES.contains(&args.corpus.as_str()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "unknown corpus {:?}, expected one of {:?}",
                args.corpus,
                corpora::CORPUS_NAMES
            ),
        ));
    }

    let raw = corpora::load_corpus(&args.corpus)?;
    let text = match args.size {
        Size::Paragraph => raw.trim().to_string(),
        Size::Large => corpora::expand_to_lines(&raw, args.large_lines),
    };

    // Both arms below follow the same shape: construct, sample init RSS,
    // optionally warm up (discarding that call's shaped state -- it's not
    // being measured), then the timed call returns its shaped state
    // instead of dropping it internally, so `elapsed_ms` and
    // `peak_rss_bytes` are both sampled BEFORE that state is dropped, on
    // both backends -- see this file's module docs for why that symmetry
    // matters on both axes.
    let (elapsed_ms, result, parley_mode, init_rss_bytes, peak_rss_bytes) = match args.backend {
        Backend::Parley => {
            let mut backend = parley_backend::ParleyBackend::new();
            let init_rss_bytes = metrics::peak_rss_bytes();
            if args.cache == Cache::Warm {
                match args.parley_mode {
                    ParleyMode::PerParagraph => {
                        let _ = backend.shape_per_paragraph(&text, args.max_width);
                    }
                    ParleyMode::Single => {
                        let _ = backend.shape(&text, args.max_width);
                    }
                }
            }
            let (elapsed_ms, result, peak_rss_bytes) = match args.parley_mode {
                ParleyMode::PerParagraph => {
                    let start = std::time::Instant::now();
                    let (result, layouts) =
                        backend.shape_per_paragraph_retained(&text, args.max_width);
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let peak_rss_bytes = metrics::peak_rss_bytes();
                    drop(layouts);
                    (elapsed_ms, result, peak_rss_bytes)
                }
                ParleyMode::Single => {
                    let start = std::time::Instant::now();
                    let (result, layout) = backend.shape_retained(&text, args.max_width);
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let peak_rss_bytes = metrics::peak_rss_bytes();
                    drop(layout);
                    (elapsed_ms, result, peak_rss_bytes)
                }
            };
            (
                elapsed_ms,
                result,
                Some(format!("{:?}", args.parley_mode)),
                init_rss_bytes,
                peak_rss_bytes,
            )
        }
        Backend::CosmicText => {
            let mut backend = cosmic_backend::CosmicBackend::new();
            let init_rss_bytes = metrics::peak_rss_bytes();
            if args.cache == Cache::Warm {
                let _ = backend.shape(&text, args.max_width);
            }
            let start = std::time::Instant::now();
            let (result, buffer) = backend.shape_retained(&text, args.max_width);
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let peak_rss_bytes = metrics::peak_rss_bytes();
            drop(buffer);
            (elapsed_ms, result, None, init_rss_bytes, peak_rss_bytes)
        }
    };

    let report = Report {
        backend: format!("{:?}", args.backend),
        corpus: args.corpus,
        size: format!("{:?}", args.size),
        cache: format!("{:?}", args.cache),
        parley_mode,
        elapsed_ms,
        init_rss_bytes,
        peak_rss_bytes,
        line_count: result.line_count,
        glyph_count: result.glyph_count,
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
