//! What a frame is rendered into and how much of it: the damage that bounds a
//! partial frame, the cost model that picks how it is redrawn, the retained
//! target that holds the last frame, the render target of one pass, and the
//! protocol that turns a frame plan into GPU work.

pub(crate) mod damage;
pub(crate) mod protocol;
mod region_cost;
pub(crate) mod render_target;
pub(crate) mod retained_target;
