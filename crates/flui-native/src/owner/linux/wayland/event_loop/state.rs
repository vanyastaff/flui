//! What the owner's event queue dispatches into: the globals the loop binds itself.

use std::sync::Arc;

use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_subcompositor::{self, WlSubcompositor};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::{
    self, WpFractionalScaleManagerV1,
};
use wayland_protocols::wp::presentation_time::client::wp_presentation::WpPresentation;
use wayland_protocols::wp::viewporter::client::wp_viewporter::{self, WpViewporter};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::{self, XdgActivationV1};
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::{
    self, ZxdgDecorationManagerV1,
};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::{self, ZxdgExporterV2};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use super::{
    LiveOutputs, OutputName, PresentationClock, PresentationTime, WaylandClipboard, WaylandInput,
};
use crate::transfer::linux::wayland::DataDevices;

/// The newest `wl_compositor` the loop understands: version 6 gives a
/// surface its preferred buffer scale.
const COMPOSITOR_VERSION: u32 = 6;
/// The newest `xdg_wm_base`: version 5 announces what the compositor lets
/// a toplevel do, version 6 says when it is suspended.
const WM_BASE_VERSION: u32 = 6;
/// The `xdg_wm_base` (and `xdg_toplevel`) version from which the
/// compositor says when it suspends a toplevel: one it does not show,
/// covered by other windows among the reasons xdg-shell names.
pub(crate) const SUSPENSION_VERSION: u32 = 6;
/// The newest `wl_output`; the loop binds every output only so that a
/// surface's `enter` and `leave` name one.
const OUTPUT_VERSION: u32 = 4;
/// The outputs the loop binds. A desk has a few screens and a video wall
/// a few dozen; a compositor announcing outputs without end is not bound
/// past this, and a surface on one of those names no display.
pub(crate) const MAX_OUTPUTS: usize = 64;
/// `wp_viewporter` has one version.
const VIEWPORTER_VERSION: u32 = 1;
/// `zxdg_decoration_manager_v1` has one version.
const DECORATION_VERSION: u32 = 1;
/// `wp_fractional_scale_manager_v1` has one version.
const FRACTIONAL_SCALE_VERSION: u32 = 1;
/// `zxdg_exporter_v2` has one version.
const EXPORTER_VERSION: u32 = 1;
/// `wp_presentation` version 1; version 2 only lets a compositor round
/// its `refresh`.
const PRESENTATION_VERSION: u32 = 1;
/// `xdg_activation_v1` has one version.
const ACTIVATION_VERSION: u32 = 1;

/// What the owner's event queue dispatches into: the globals the loop
/// binds itself. The window feature implements the dispatch of a window's
/// own objects (`wl_surface`, `xdg_surface`, `xdg_toplevel` and frame
/// callbacks) on it, recording into the window's own state.
pub(crate) struct WaylandState {
    /// Every output the compositor announced, bound so that a surface's
    /// `enter` names it by its global.
    pub(super) outputs: Vec<(OutputName, WlOutput)>,
    /// The same outputs, as the windows read them.
    pub(super) live: LiveOutputs,
    /// The seats' input, offered every global the loop does not bind.
    pub(super) input: Arc<dyn WaylandInput>,
    /// The clipboard, offered the same globals.
    pub(super) clipboard: Arc<dyn WaylandClipboard>,
    /// The seats' data devices, which the clipboard and drag share.
    pub(super) devices: DataDevices,
    /// The globals the loop binds for itself, while it starts: each is
    /// bound as the compositor first announces it, and the loop takes
    /// them once the registry was read. `None` from then on, when a
    /// global of one of those kinds is bound no more.
    pub(super) starting: Option<LoopGlobals>,
}

/// The globals the loop binds for itself, each the first of its kind the
/// compositor announces.
#[derive(Default)]
pub(super) struct LoopGlobals {
    pub(super) compositor: Option<WlCompositor>,
    pub(super) wm_base: Option<XdgWmBase>,
    pub(super) viewporter: Option<WpViewporter>,
    pub(super) subcompositor: Option<WlSubcompositor>,
    pub(super) shm: Option<WlShm>,
    pub(super) decoration: Option<ZxdgDecorationManagerV1>,
    pub(super) fractional_scale: Option<WpFractionalScaleManagerV1>,
    pub(super) exporter: Option<ZxdgExporterV2>,
    pub(super) presentation: Option<PresentationTime>,
    pub(super) activation: Option<XdgActivationV1>,
}

impl LoopGlobals {
    /// Bind global `name` if it is of a kind the loop binds and the
    /// first of it announced.
    fn offer(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        /// Bind `name` into `slot` at the version announced, no newer
        /// than `newest`, unless one of its kind is bound.
        fn first<I>(
            slot: &mut Option<I>,
            registry: &WlRegistry,
            (name, version, newest): (u32, u32, u32),
            queue: &QueueHandle<WaylandState>,
        ) where
            I: Proxy + 'static,
            WaylandState: Dispatch<I, ()>,
        {
            if slot.is_none() {
                let bound =
                    registry.bind::<I, (), WaylandState>(name, version.min(newest), queue, ());
                *slot = Some(bound);
            }
        }
        let global = |newest: u32| (name, version, newest);
        if interface == WlCompositor::interface().name {
            first(
                &mut self.compositor,
                registry,
                global(COMPOSITOR_VERSION),
                queue,
            );
        } else if interface == XdgWmBase::interface().name {
            first(&mut self.wm_base, registry, global(WM_BASE_VERSION), queue);
        } else if interface == WpViewporter::interface().name {
            first(
                &mut self.viewporter,
                registry,
                global(VIEWPORTER_VERSION),
                queue,
            );
        } else if interface == WlSubcompositor::interface().name {
            first(&mut self.subcompositor, registry, global(1), queue);
        } else if interface == WlShm::interface().name {
            first(&mut self.shm, registry, global(1), queue);
        } else if interface == ZxdgDecorationManagerV1::interface().name {
            first(
                &mut self.decoration,
                registry,
                global(DECORATION_VERSION),
                queue,
            );
        } else if interface == WpFractionalScaleManagerV1::interface().name {
            first(
                &mut self.fractional_scale,
                registry,
                global(FRACTIONAL_SCALE_VERSION),
                queue,
            );
        } else if interface == ZxdgExporterV2::interface().name {
            first(
                &mut self.exporter,
                registry,
                global(EXPORTER_VERSION),
                queue,
            );
        } else if interface == XdgActivationV1::interface().name {
            first(
                &mut self.activation,
                registry,
                global(ACTIVATION_VERSION),
                queue,
            );
        } else if interface == WpPresentation::interface().name && self.presentation.is_none() {
            let clock = PresentationClock::default();
            let manager = registry.bind::<WpPresentation, PresentationClock, WaylandState>(
                name,
                version.min(PRESENTATION_VERSION),
                queue,
                clock.clone(),
            );
            self.presentation = Some(PresentationTime { manager, clock });
        }
    }
}

impl WaylandState {
    /// Offer global `name` to the features that bind theirs.
    fn offer_global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<Self>,
    ) {
        self.input.global(registry, name, interface, version, queue);
        self.clipboard
            .global(registry, name, interface, version, queue);
        self.devices
            .global(registry, name, interface, version, queue);
    }

    fn bind_output(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        version: u32,
        queue: &QueueHandle<Self>,
    ) {
        let name = OutputName::new(name);
        if self.outputs.iter().any(|(bound, _)| *bound == name) {
            return;
        }
        if self.outputs.len() >= MAX_OUTPUTS {
            tracing::debug!(?name, "a Wayland output past {MAX_OUTPUTS} was not bound");
            return;
        }
        let output = registry.bind::<WlOutput, OutputName, Self>(
            name.get(),
            version.min(OUTPUT_VERSION),
            queue,
            name,
        );
        self.live.add(name);
        self.outputs.push((name, output));
    }

    fn remove_output(&mut self, name: u32) {
        let name = OutputName::new(name);
        self.live.remove(name);
        if let Some(index) = self.outputs.iter().position(|(bound, _)| *bound == name) {
            let (_, output) = self.outputs.remove(index);
            // `release` exists from version 3; below it the proxy is just
            // dropped.
            if output.version() >= 3 {
                output.release();
            }
        }
    }
}

/// The registry keeps no list of what the compositor announces: a global
/// is bound as it is announced, by the loop or by a feature, or not at
/// all, and a removal is told to those that bind. A compositor announcing
/// globals without end grows nothing here.
impl Dispatch<WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match bindable(event) {
            Some(wl_registry::Event::Global {
                name,
                interface,
                version,
            }) if interface == WlOutput::interface().name => {
                state.bind_output(registry, name, version, queue);
            }
            Some(wl_registry::Event::Global {
                name,
                interface,
                version,
            }) => {
                if let Some(starting) = &mut state.starting {
                    starting.offer(registry, name, &interface, version, queue);
                }
                state.offer_global(registry, name, &interface, version, queue);
            }
            Some(wl_registry::Event::GlobalRemove { name }) => {
                state.remove_output(name);
                state.input.global_removed(name);
                state.clipboard.global_removed(name);
                state.devices.global_removed(name);
            }
            _ => {}
        }
    }
}

/// `event` as a client acts on it: `None` for a global of version zero,
/// which no client binds, so the loop, its outputs, every feature it
/// offers globals to and the display watch bind from version one.
pub(crate) fn bindable(event: wl_registry::Event) -> Option<wl_registry::Event> {
    match event {
        wl_registry::Event::Global { version: 0, .. } => None,
        event => Some(event),
    }
}

impl Dispatch<WlOutput, OutputName> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: wl_output::Event,
        _: &OutputName,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The display source reads the outputs on its own connection; here
        // an output is only a name a surface enters.
    }
}

impl Dispatch<WlCompositor, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The compositor has no events.
    }
}

impl Dispatch<WpViewporter, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The viewporter has no events.
    }
}

impl Dispatch<WlShm, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: wl_shm::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The formats it announces: ARGB8888, which a window's own frame
        // is drawn in, is every compositor's.
    }
}

impl Dispatch<WlSubcompositor, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlSubcompositor,
        _: wl_subcompositor::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The subcompositor has no events.
    }
}

impl Dispatch<ZxdgDecorationManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZxdgDecorationManagerV1,
        _: zxdg_decoration_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZxdgExporterV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZxdgExporterV2,
        _: zxdg_exporter_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The exporter has no events.
    }
}

impl Dispatch<XdgActivationV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: xdg_activation_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The activation global has no events; its tokens are the
        // windows'.
    }
}

impl Dispatch<XdgWmBase, ()> for WaylandState {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A client that does not answer is considered hung.
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}
