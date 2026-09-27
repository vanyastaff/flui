//! flui-cupertino's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).
//!
//! `common` is declared once here and every suite imports it with
//! `use crate::common;` -- a per-suite `mod common;` would load
//! `tests/common/mod.rs` once per suite (`clippy::duplicate_mod`).

mod common;

#[path = "bottom_tab_bar.rs"]
mod bottom_tab_bar;

#[path = "button.rs"]
mod button;

#[path = "colors.rs"]
mod colors;

#[path = "cupertino_app.rs"]
mod cupertino_app;

#[path = "nav_bar.rs"]
mod nav_bar;

#[path = "page_scaffold.rs"]
mod page_scaffold;

#[path = "route.rs"]
mod route;

#[path = "tab_scaffold.rs"]
mod tab_scaffold;

#[path = "theme.rs"]
mod theme;
