//! Abstract pipeline traits
//!
//! This module provides the abstract traits that define pipeline behavior.
//! Concrete implementations live in `flui_core`.
//!
//! # Architecture
//!
//! ```text
//! flui-tree (tree traits)         flui-pipeline (pipeline traits)    flui_core (impl)
//! ┌─────────────────────┐         ┌─────────────────────────┐        ┌─────────────────┐
//! │ TreeRead/Nav/Write  │ ◄────── │ BuildPhase              │ ◄───── │ BuildPipeline   │
//! │ RenderTreeAccess    │         │ LayoutPhase             │ ◄───── │ LayoutPipeline  │
//! │ DirtyTracking       │         │ PaintPhase              │ ◄───── │ PaintPipeline   │
//! └─────────────────────┘         │ PipelineCoordinator     │ ◄───── │ FrameCoordinator│
//!                                 │ LayoutVisitable         │        └─────────────────┘
//! flui-scheduler (types)          │ PaintVisitable          │
//! ┌─────────────────────┐         │ HitTestVisitable        │
//! │ Priority            │ ◄────── │ SchedulerIntegration    │
//! │ FrameTiming         │         └─────────────────────────┘
//! └─────────────────────┘
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
//! ## Visitor Traits (from flui-tree)
//!
//! - [`LayoutVisitable`]: Abstract layout operations on tree nodes
//! - [`PaintVisitable`]: Abstract paint operations on tree nodes
//! - [`HitTestVisitable`]: Abstract hit-test operations on tree nodes
//!
//! ## Extension Traits
//!
//! - [`ParallelExecution`]: For phases supporting parallel processing
//! - [`BatchedExecution`]: For phases supporting batching
//!
//! ## Coordination Traits
//!
//! - [`PipelineCoordinator`]: Orchestrates phase execution
//! - [`SchedulerIntegration`]: Bridge to flui-scheduler
//!
//! ## Tree Access Traits (from flui-tree)
//!
//! - [`TreeRead`], [`TreeNav`]: Basic tree access
//! - [`RenderTreeAccess`]: Access to render objects and state
//! - [`DirtyTracking`]: Layout/paint dirty flag management
//!
//! # Re-exported Types
//!
//! Types from `flui-scheduler` are re-exported for convenience:
//! - [`Priority`]: Task priority levels (UserInput > Animation > Build > Idle)
//! - [`FrameTiming`]: Frame timing information

mod coordinator;
mod phase;
mod scheduler_integration;
mod visitor;

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

// Scheduler integration
pub use scheduler_integration::{NoopScheduler, RecordingScheduler, SchedulerIntegration};

// Re-export scheduler types (single source of truth from flui-scheduler)
pub use scheduler_integration::{FrameTiming, Priority};

// Visitor traits (re-exported from flui-tree)
pub use visitor::{
    // Callback-based operations
    hit_test_with_callback,
    layout_with_callback,
    paint_with_callback,
    // Hit test visitor
    HitTestVisitable,
    HitTestVisitableExt,
    // Layout visitor
    LayoutVisitable,
    LayoutVisitableExt,
    // Paint visitor
    PaintVisitable,
    PaintVisitableExt,
    // Generic visitors
    PipelineVisitor,
    SimpleTreeVisitor,
    TreeOperation,
};

// Tree access traits (re-exported from flui-tree)
pub use visitor::{DirtyTracking, DirtyTrackingExt, RenderTreeAccess, TreeNav, TreeRead};
