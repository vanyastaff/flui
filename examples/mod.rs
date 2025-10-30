//! # Flui Examples
//!
//! This module contains comprehensive examples demonstrating various Flui widgets and patterns.
//!
//! ## Basic Examples
//! - **widget_hello_world** - Simple hello world using the modern builder pattern
//! - **minimal_app** - Minimal application setup
//!
//! ## Real-World UI Examples
//!
//! ### Profile Card (`profile_card.rs`)
//! A beautiful profile card demonstrating:
//! - Card widget for elevation and styling
//! - Row and Column for layout composition
//! - ClipOval for circular avatar
//! - Divider for visual separation
//! - Stats display and action buttons
//!
//! ### Dashboard (`dashboard.rs`)
//! A complete dashboard interface showing:
//! - Complex layouts with nested Row and Column
//! - Multiple Card widgets for information panels
//! - Flexible widgets for responsive design
//! - Stats cards with percentage changes
//! - Activity feed and quick actions panel
//!
//! ### Photo Gallery (`photo_gallery.rs`)
//! A responsive photo gallery featuring:
//! - Wrap widget for responsive grid layout
//! - AspectRatio to maintain image proportions
//! - ClipRRect for rounded corners
//! - Stack for overlay effects
//! - Filter chips with active states
//!
//! ### Settings Page (`settings_page.rs`)
//! A professional settings interface with:
//! - Sectioned layout using Cards
//! - Dividers for visual organization
//! - GestureDetector for interactive items
//! - Multiple setting categories
//! - Save and reset actions
//!
//! ### Pricing Table (`pricing_table.rs`)
//! A pricing comparison page demonstrating:
//! - Horizontal card layout
//! - Feature lists with checkmarks
//! - Popular badge overlay
//! - Different pricing tiers
//! - Professional design patterns
//!
//! ## Running Examples
//!
//! To run any example:
//! ```bash
//! cargo run --example profile_card --features="flui_app,flui_widgets"
//! cargo run --example dashboard --features="flui_app,flui_widgets"
//! cargo run --example photo_gallery --features="flui_app,flui_widgets"
//! cargo run --example settings_page --features="flui_app,flui_widgets"
//! cargo run --example pricing_table --features="flui_app,flui_widgets"
//! ```

pub mod dashboard;
pub mod minimal_app;
pub mod photo_gallery;
pub mod pricing_table;
pub mod profile_card;
pub mod settings_page;
pub mod widget_hello_world;























