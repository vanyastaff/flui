//! Renderer subsystem - command execution and backend abstraction
//!
//! This module provides the command rendering infrastructure following Clean Architecture:
//! - CommandRenderer trait (visitor interface for command dispatch)
//! - WgpuRenderer (wgpu backend implementation)
//! - RenderBackend (strategy pattern for backend selection)
//!
//! # Architecture
//!
//! ```text
//! DisplayList (Commands) → CommandRenderer (Trait) → WgpuRenderer (Impl)
//!                              ↓                           ↓
//!                       Visitor Pattern            WgpuPainter (GPU)
//! ```
//!
//! This design follows SOLID principles:
//! - **S**ingle Responsibility: Each renderer handles one backend
//! - **O**pen/Closed: Add new commands/renderers without modifying existing code
//! - **L**iskov Substitution: All CommandRenderer impls are interchangeable
//! - **I**nterface Segregation: CommandRenderer has focused, cohesive interface
//! - **D**ependency Inversion: High-level code depends on abstractions

pub mod command_renderer;
#[cfg(debug_assertions)]
pub mod debug_renderer;
pub mod dispatcher;
pub mod wgpu_renderer;



pub use command_renderer::CommandRenderer;
pub use dispatcher::{dispatch_command, dispatch_commands};
pub use wgpu_renderer::WgpuRenderer;

#[cfg(debug_assertions)]
pub use debug_renderer::DebugRenderer;

/// Rendering backend selection (Strategy pattern)
///
/// Allows swapping rendering backends at runtime for different use cases:
/// - **Wgpu**: Production GPU-accelerated rendering
/// - **Debug**: Logging renderer for development/debugging
pub enum RenderBackend {
    /// GPU-accelerated wgpu backend (production)
    Wgpu(Box<WgpuRenderer>),

    /// Debug renderer (logs commands, validates state)
    #[cfg(debug_assertions)]
    Debug(DebugRenderer),
}

impl RenderBackend {
    /// Get a mutable reference to the underlying renderer
    ///
    /// This enables polymorphic rendering - the same code can work with
    /// any backend through the CommandRenderer trait.
    pub fn as_renderer(&mut self) -> &mut dyn CommandRenderer {
        match self {
            RenderBackend::Wgpu(r) => r.as_mut(),
            #[cfg(debug_assertions)]
            RenderBackend::Debug(r) => r,
        }
    }
}

