//! The handle source a surface lease holds.
//!
//! A window's surface is made from handles the lease owns, never from raw
//! handles of a *borrowed* window kept for a device-loss recovery: a
//! `#![forbid(unsafe_code)]` consumer could otherwise hand over a borrow, drop
//! the window and keep presenting with bytes that outlived what they pointed
//! at, and the soundness of the surface-creation call would rest on a comment,
//! not on a type. The public entry points that take a window
//! ([`GpuContext::bootstrap`](crate::GpuContext::bootstrap) and
//! [`SurfaceFactory::create`](crate::SurfaceFactory::create)) take its
//! `Arc<dyn SurfaceTarget>`, owned and `'static`, so a caller cannot hand a
//! window over and then outlive it (the compile-fail case
//! `surface_factory_rejects_borrowed_window` pins it); inside the crate the
//! lease holds it as a `WindowTarget`, the bound wgpu's surface creation
//! reads.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// What a window's surface needs from the thing it draws into: a handle
/// source the surface lease can **own** for its whole life, **share** with
/// wgpu's safe surface-creation path, and **re-query** before each surface
/// is made (a first attach, a restore after a suspend or a lost surface).
///
/// - `HasWindowHandle + HasDisplayHandle` — the two safe accessors wgpu's own
///   `create_surface` calls internally; nothing here calls unsafe
///   window-handle extraction any more.
/// - `'static` — the lease retains this value (in the crate-private
///   `SurfaceLease`) for as long as its surface lives, which outlives the
///   stack frame that made the surface and crosses to the raster executor.
///   A borrowed target was considered and rejected (see the issue's ADR): it
///   forbids `drop(window)` but not a `WindowCommand::Close`, and is
///   incompatible with a `thread_local!` runtime registry and `spawn_local`
///   futures on web, both of which need `'static`.
/// - `Send + Sync` — wgpu's own `WindowHandle` marker trait (the bound its
///   safe `SurfaceTarget` requires) demands both, and the surface the lease
///   holds travels to the executor's thread in a `RasterCommand::Attach` or
///   `Restore`.
///
/// Blanket-implemented over every type that already satisfies the bounds —
/// effectively sealed, since there is nothing to implement by hand. This
/// mirrors the market shape rather than inventing one: iced's `Window` trait
/// takes the identical `HasWindowHandle + HasDisplayHandle + Send + Sync`
/// combination for its owned-target renderer, and wgpu's own `WindowHandle`
/// (`wgpu::WindowHandle` internal to `create_surface`) is `HasWindowHandle +
/// Send + Sync` for exactly the same "the surface owns this for its whole
/// life" reason.
pub(crate) trait WindowTarget:
    HasWindowHandle + HasDisplayHandle + Send + Sync + 'static
{
}

impl<T> WindowTarget for T where
    T: ?Sized + HasWindowHandle + HasDisplayHandle + Send + Sync + 'static
{
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };

    use super::WindowTarget;

    /// Never constructed as a real window — this only needs to type-check as
    /// a `WindowTarget`, never to hand out a working handle.
    struct FakeWindow;

    impl HasWindowHandle for FakeWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }

    impl HasDisplayHandle for FakeWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }

    // Compile-time-only counterpart to the `surface_factory_rejects_borrowed_window`
    // compile-fail fixture: an `Arc<T>` — the shape every real call site
    // hands over (`Arc::clone(&window)`) — satisfies `WindowTarget`. A
    // `#[test]` fn wrapping this would run at test time and prove nothing at
    // runtime (the whole check is that it TYPE-CHECKS); `assert_impl_all!`
    // states that directly, with no GPU involved.
    static_assertions::assert_impl_all!(Arc<FakeWindow>: WindowTarget);
}
