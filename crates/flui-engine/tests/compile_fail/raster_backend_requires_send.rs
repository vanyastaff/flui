//! Pins ADR-0045 decision 1's `RasterBackend: Send` supertrait: a backend
//! whose fields make it `!Send` must fail to implement `RasterBackend` at
//! the `impl` site itself, not merely fail later at a
//! `dyn RasterBackend + Send` use site.

use std::marker::PhantomData;

struct NotSend {
    _marker: PhantomData<*const ()>,
}

struct BadBackend {
    _not_send: NotSend,
}

impl flui_engine::RasterBackend for BadBackend {
    fn render_scene(
        &mut self,
        _scene: &flui_layer::Scene,
    ) -> Result<bool, flui_engine::EngineError> {
        Ok(false)
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn is_device_lost(&self) -> bool {
        false
    }

    fn mark_dirty(&mut self, _rect: flui_types::geometry::Rect<flui_types::geometry::Pixels>) {}

    fn mark_full_repaint(&mut self) {}

    fn has_damage(&self) -> bool {
        false
    }

    fn size(&self) -> (u32, u32) {
        (0, 0)
    }

    fn reconfigure_surface(&mut self) -> Result<(), flui_engine::EngineError> {
        Ok(())
    }
}

fn main() {}
