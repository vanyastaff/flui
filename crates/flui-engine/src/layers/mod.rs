//! The walk from a scene's layers to the painter: the layer visitor, the
//! command and state traits it dispatches through, the dispatcher that
//! implements them over `WgpuPainter`, and the offscreen and compositing
//! helpers that render a layer to a texture.

mod command_renderer;
pub(crate) mod compositor;
pub(crate) mod dispatch;
pub(crate) mod dispatcher;
pub(crate) mod offscreen;
mod render;
mod state_stack;
pub(crate) mod walk;
