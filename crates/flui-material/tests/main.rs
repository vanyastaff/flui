//! flui-material's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).
//!
//! `common` is declared once here and every suite imports it with
//! `use crate::common;` -- a per-suite `mod common;` would load
//! `tests/common/mod.rs` once per suite (`clippy::duplicate_mod`).

mod common;

#[path = "app_bar.rs"]
mod app_bar;

#[path = "card.rs"]
mod card;

#[path = "checkbox.rs"]
mod checkbox;

#[path = "chip.rs"]
mod chip;

#[path = "data_table.rs"]
mod data_table;

#[path = "dialog.rs"]
mod dialog;

#[path = "divider.rs"]
mod divider;

#[path = "drawer.rs"]
mod drawer;

#[path = "elevated_button.rs"]
mod elevated_button;

#[path = "flexible_space_bar.rs"]
mod flexible_space_bar;

#[path = "floating_action_button.rs"]
mod floating_action_button;

#[path = "icon_button.rs"]
mod icon_button;

#[path = "inherited_theme.rs"]
mod inherited_theme;

#[path = "ink_well.rs"]
mod ink_well;

#[path = "input_decorator.rs"]
mod input_decorator;

#[path = "list_tile.rs"]
mod list_tile;

#[path = "material.rs"]
mod material;

#[path = "material_app.rs"]
mod material_app;

#[path = "navigation_bar.rs"]
mod navigation_bar;

#[path = "radio.rs"]
mod radio;

#[path = "rebuild_exactness.rs"]
mod rebuild_exactness;

#[path = "scaffold.rs"]
mod scaffold;

#[path = "show_dialog.rs"]
mod show_dialog;

#[path = "sliver_app_bar.rs"]
mod sliver_app_bar;

#[path = "snack_bar.rs"]
mod snack_bar;

#[path = "switch.rs"]
mod switch;

#[path = "tab_bar_view.rs"]
mod tab_bar_view;

#[path = "tabs.rs"]
mod tabs;

#[path = "text_field.rs"]
mod text_field;

#[path = "theme.rs"]
mod theme;
