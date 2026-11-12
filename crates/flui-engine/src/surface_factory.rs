//! The owner's factory of window surfaces over the GPU context's one instance
//! (ADR-0305 §2).
//!
//! The instance is made on the host's [`DisplayConnection`] and keeps it for
//! its whole life: wgpu reads the instance's display at every surface it
//! creates, with `expect`, so a display taken from a window would panic on
//! the next window's surface once the first window had closed. The factory
//! holds the same connection and compares a target's display with it before
//! wgpu sees the target.
//!
//! The factory is owner-local: it is built on an [`Rc`], so code that makes a
//! surface on the raster thread does not compile. That is where the Apple and
//! Android targets answer `Unavailable`. The surface it makes is `Send` and
//! travels to the executor in a `RasterCommand::Attach` or `Restore`.

use std::{fmt, rc::Rc, sync::Arc};

use flui_platform::{DisplayConnection, SurfaceTarget};
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle};

use crate::{
    error::{EngineError, EngineResult},
    surface_lease::{SurfaceLease, probe_target},
    window_target::WindowTarget,
};

/// The host's display connection as wgpu's instance holds it: one clone of the
/// host's `Arc`, kept for the instance's life.
pub(crate) struct HeldDisplay(pub(crate) Arc<dyn DisplayConnection>);

impl fmt::Debug for HeldDisplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HeldDisplay")
    }
}

impl HasDisplayHandle for HeldDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

/// The instance descriptor of one backend attempt over `display`: made on the
/// display when it answers one, without a display when it answers none (a
/// headless host), which only offscreen slots and no window can use.
pub(crate) fn instance_descriptor(
    display: &Arc<dyn DisplayConnection>,
    backends: wgpu::Backends,
    backend_options: wgpu::BackendOptions,
) -> wgpu::InstanceDescriptor {
    let base = if display.display_handle().is_ok() {
        wgpu::InstanceDescriptor::new_with_display_handle(Box::new(HeldDisplay(Arc::clone(
            display,
        ))))
    } else {
        wgpu::InstanceDescriptor::new_without_display_handle()
    };
    wgpu::InstanceDescriptor {
        backends,
        backend_options,
        ..base
    }
}

/// `target` as the surface lease holds it, after the checks every surface
/// passes before wgpu sees it: the target answers its handles, and its
/// display is `display`.
///
/// # Errors
/// [`EngineError::SurfaceTargetUnavailable`] for a target whose handles do
/// not answer; [`EngineError::SurfaceOnForeignDisplay`] for one on another
/// display, or any target when `display` answers none.
pub(crate) fn checked_target(
    display: &dyn DisplayConnection,
    target: &Arc<dyn SurfaceTarget>,
) -> EngineResult<Arc<dyn WindowTarget>> {
    // `Arc<dyn SurfaceTarget>` cannot upcast to `Arc<dyn WindowTarget>`: the
    // platform's trait cannot name this crate's as a supertrait.
    let leased: Arc<dyn WindowTarget> = Arc::new(Arc::clone(target));
    probe_target(&leased)?;
    let theirs = target
        .display_handle()
        .map_err(EngineError::surface_target_unavailable)?
        .as_raw();
    match display.display_handle() {
        Ok(ours) if ours.as_raw() == theirs => Ok(leased),
        Ok(_) | Err(_) => Err(EngineError::SurfaceOnForeignDisplay),
    }
}

/// What the factory shares between its clones.
struct FactoryParts {
    instance: wgpu::Instance,
    display: Arc<dyn DisplayConnection>,
}

/// Makes window surfaces over the GPU context's instance, on the owner thread
/// (ADR-0305 §2).
///
/// From [`GpuContext::bootstrap`](crate::GpuContext::bootstrap). Not `Send`
/// and not `Sync`: its parts sit behind an [`Rc`].
#[derive(Clone)]
pub struct SurfaceFactory {
    parts: Rc<FactoryParts>,
}

static_assertions::assert_not_impl_any!(SurfaceFactory: Send, Sync);

impl fmt::Debug for SurfaceFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurfaceFactory")
            .finish_non_exhaustive()
    }
}

impl SurfaceFactory {
    pub(crate) fn new(instance: wgpu::Instance, display: Arc<dyn DisplayConnection>) -> Self {
        Self {
            parts: Rc::new(FactoryParts { instance, display }),
        }
    }

    /// A surface over `target`, not yet configured: it gets its size in the
    /// slot it is attached to.
    ///
    /// # Errors
    /// - [`EngineError::SurfaceTargetUnavailable`]: the target's handles do
    ///   not answer (a closed window, a suspended Android surface).
    /// - [`EngineError::SurfaceOnForeignDisplay`]: the target is on another
    ///   display than the context's, or the context has no display.
    /// - [`EngineError::SurfaceCreation`]: wgpu refused the surface.
    pub fn create(&self, target: Arc<dyn SurfaceTarget>) -> EngineResult<UnconfiguredSurface> {
        let leased = checked_target(&*self.parts.display, &target)?;
        let surface = self
            .parts
            .instance
            .create_surface(Arc::clone(&leased))
            .map_err(EngineError::surface_creation)?;
        Ok(UnconfiguredSurface::new(leased, target, surface))
    }
}

/// A window's surface, made by the [`SurfaceFactory`] and not configured yet:
/// it travels to the raster executor in a slot's
/// [`RasterCommand::Attach`](crate::RasterCommand::Attach) or
/// [`RasterCommand::Restore`](crate::RasterCommand::Restore).
pub struct UnconfiguredSurface {
    /// The surface and the target it needs (ADR-0063): the surface drops first.
    pub(crate) lease: SurfaceLease<wgpu::Surface<'static>>,
    /// The same target, which the slot tells before each present.
    pub(crate) target: Arc<dyn SurfaceTarget>,
}

static_assertions::assert_impl_all!(UnconfiguredSurface: Send);

impl fmt::Debug for UnconfiguredSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnconfiguredSurface")
            .finish_non_exhaustive()
    }
}

impl UnconfiguredSurface {
    pub(crate) fn new(
        leased: Arc<dyn WindowTarget>,
        target: Arc<dyn SurfaceTarget>,
        surface: wgpu::Surface<'static>,
    ) -> Self {
        Self {
            lease: SurfaceLease::from_parts(leased, surface),
            target,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_ulong, sync::Arc};

    use flui_platform::{DisplayConnection, SurfaceTarget, SwapChainStatistics};
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, WindowHandle, XcbDisplayHandle, XlibDisplayHandle, XlibWindowHandle,
    };

    use super::{SurfaceFactory, checked_target};
    use crate::error::EngineError;

    /// A display handle without a connection pointer, told apart by kind and
    /// screen, so no handle here has a pointee whose validity matters (and
    /// the doubles stay `Send + Sync`, which a raw handle is not).
    #[derive(Debug, Clone, Copy)]
    enum Handle {
        Xlib(i32),
        Xcb(i32),
    }

    impl Handle {
        fn borrowed(self) -> DisplayHandle<'static> {
            let raw = match self {
                Self::Xlib(screen) => RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, screen)),
                Self::Xcb(screen) => RawDisplayHandle::Xcb(XcbDisplayHandle::new(None, screen)),
            };
            // SAFETY: neither handle carries a pointer, so there is no
            // pointee for `borrow_raw`'s contract to protect.
            #[expect(unsafe_code, reason = "test double: a handle without a pointer")]
            unsafe {
                DisplayHandle::borrow_raw(raw)
            }
        }
    }

    struct Display(Option<Handle>);

    impl HasDisplayHandle for Display {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            self.0
                .map(Handle::borrowed)
                .ok_or(HandleError::NotSupported)
        }
    }

    impl DisplayConnection for Display {}

    /// A window on `display`; `None` for a window whose handle is gone.
    struct Window {
        display: Handle,
        window: Option<c_ulong>,
    }

    impl HasWindowHandle for Window {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let id = self.window.ok_or(HandleError::Unavailable)?;
            let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(id));
            // SAFETY: an Xlib window handle is an integer id, not a pointer.
            #[expect(unsafe_code, reason = "test double: a handle without a pointer")]
            let handle = unsafe { WindowHandle::borrow_raw(raw) };
            Ok(handle)
        }
    }

    impl HasDisplayHandle for Window {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(self.display.borrowed())
        }
    }

    impl SurfaceTarget for Window {
        fn pre_present_notify(&self, _statistics: SwapChainStatistics) {}
    }

    fn window(display: Handle, window: Option<c_ulong>) -> Arc<dyn SurfaceTarget> {
        Arc::new(Window { display, window })
    }

    /// A target on another display than the factory's, or any target of a
    /// factory without a display, is refused with a typed error before wgpu
    /// sees it; a closed window answers that its handle is unavailable. The
    /// factory's instance has no backend, so a target that reached wgpu
    /// would fail with `SurfaceCreation` instead.
    ///
    /// Red-check: drop the display comparison from `checked_target` and the
    /// three foreign rows fail with `SurfaceCreation`.
    #[test]
    fn a_surface_on_a_foreign_display_is_refused_before_wgpu_sees_it() {
        let ours = Handle::Xlib(0);
        let xcb = Handle::Xcb(0);
        let cases = [
            (
                "another screen",
                Some(ours),
                window(Handle::Xlib(1), Some(7)),
                "foreign",
            ),
            (
                "another window system",
                Some(ours),
                window(xcb, Some(7)),
                "foreign",
            ),
            ("a headless host", None, window(ours, Some(7)), "foreign"),
            (
                "a closed window",
                Some(ours),
                window(ours, None),
                "unavailable",
            ),
        ];
        for (name, display, target, expected) in cases {
            let display: Arc<dyn DisplayConnection> = Arc::new(Display(display));
            let factory = SurfaceFactory::new(
                wgpu::Instance::new(wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::empty(),
                    ..wgpu::InstanceDescriptor::new_without_display_handle()
                }),
                display,
            );
            let found = match factory.create(target) {
                Err(EngineError::SurfaceOnForeignDisplay) => "foreign",
                Err(EngineError::SurfaceTargetUnavailable {
                    source: HandleError::Unavailable,
                }) => "unavailable",
                other => panic!("{name}: answered {other:?}"),
            };
            assert_eq!(found, expected, "{name}");
        }
        assert!(
            checked_target(&Display(Some(ours)), &window(ours, Some(7))).is_ok(),
            "a window on the factory's display passes the checks"
        );
    }
}
