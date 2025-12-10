//! Abstract pipeline traits
//!
//! This module provides the abstract traits that define pipeline behavior.
//! Concrete implementations live in `flui_core`.
//!
//! # Architecture
//!
//! ```text
//! flui-pipeline (this crate)       flui_core (concrete impl)
//! ┌─────────────────────────┐      ┌─────────────────────────┐
//! │ BuildPhase              │ ◄─── │ BuildPipeline           │
//! │ LayoutPhase             │ ◄─── │ LayoutPipeline          │
//! │ PaintPhase              │ ◄─── │ PaintPipeline           │
//! │ PipelineCoordinator     │ ◄─── │ FrameCoordinator        │
//! └─────────────────────────┘      └─────────────────────────┘
//!
//! flui_rendering (flags & context-based API)
//! ┌─────────────────────────┐
//! │ AtomicRenderFlags       │  ← Lock-free dirty flags
//! │ LayoutTree trait        │
//! │ PaintTree trait         │
//! │ LayoutContext           │  ← Type-safe layout operations
//! │ PaintContext            │  ← Type-safe paint operations
//! │ HitTestContext          │  ← Type-safe hit testing
//! └─────────────────────────┘
//! ```
//!
//! # Key Traits
//!
//! ## Phase Traits
//!
//! - [`BuildPhase`]: Rebuilds dirty widgets (depth-aware scheduling)
//! - [`LayoutPhase`]: Computes sizes and positions (constraint-based)
//! - [`PaintPhase`]: Generates paint layers
//!
//! ## Extension Traits
//!
//! - [`ParallelExecution`]: For phases supporting parallel processing
//! - [`BatchedExecution`]: For phases supporting batching
//!
//! ## Coordination Traits
//!
//! - [`PipelineCoordinator`]: Orchestrates phase execution
//!
//! ## Tree Access (from flui-tree)
//!
//! - [`TreeRead`], [`TreeNav`]: Basic tree navigation
//!
//! ## Dirty Tracking (from flui_rendering)
//!
//! For dirty flags, use `flui_rendering::AtomicRenderFlags` (per-element) or
//! `flui_rendering::RenderFlags` (bitflags enum).

mod coordinator;
mod phase;

// Phase traits
pub use phase::{
    // Extension traits
    BatchedExecution,
    // Core phase traits
    BuildPhase,
    LayoutPhase,
    PaintPhase,
    ParallelExecution,
    // Common types
    PhaseContext,
    PhaseResult,
};

// Coordinator traits
pub use coordinator::{CoordinatorConfig, FrameResult, PipelineCoordinator};

// Re-export tree traits (from flui-tree)
pub use flui_tree::{TreeNav, TreeRead};
