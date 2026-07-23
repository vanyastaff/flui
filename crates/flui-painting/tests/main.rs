//! Single-binary consolidation of flui-painting's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `painting_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! paths keep working unchanged.
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-painting's integration tests do — the crate's only process-global
//! is the lazily initialized `FONT_SYSTEM` `OnceLock` (benign once-init,
//! never replaced or reset by tests).

#[path = "canvas_composition.rs"]
mod canvas_composition;
#[path = "canvas_scoped.rs"]
mod canvas_scoped;
#[path = "canvas_transform.rs"]
mod canvas_transform;
#[path = "canvas_unit.rs"]
mod canvas_unit;
#[path = "decoration_unit.rs"]
mod decoration_unit;
#[path = "display_list_unit.rs"]
mod display_list_unit;
#[path = "rich_text_example.rs"]
mod rich_text_example;
#[path = "text_layout_pipeline.rs"]
mod text_layout_pipeline;
#[path = "text_layout_unit.rs"]
mod text_layout_unit;
#[path = "text_overflow_unit.rs"]
mod text_overflow_unit;
#[path = "text_painter_unit.rs"]
mod text_painter_unit;
#[path = "thread_safety.rs"]
mod thread_safety;
