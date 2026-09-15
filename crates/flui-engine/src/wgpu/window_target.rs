//! The abstraction a windowed [`Renderer`](crate::wgpu::Renderer) draws into.
//!
//! Issue #1043: the renderer used to extract raw handles from a *borrowed*
//! window at construction time and keep the bytes for later reuse in
//! [`Renderer::recover`](crate::wgpu::Renderer::recover). A `#![forbid(unsafe_code)]`
//! consumer could write `Renderer::new(&window).await?; drop(window);` and it
//! type-checked — the saved bytes then outlived the thing they pointed at, and
//! the SAFETY argument that made the surface-creation `unsafe` block sound
//! lived in a comment, not in the type system. `WindowTarget` closes that hole
//! by requiring an *owned*, `'static` handle source instead of a borrowed one:
//! a caller can no longer hand the renderer a window and then outlive it.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// What a windowed renderer needs from the thing it draws into: a handle
/// source it can **own** for its whole life, **share** with wgpu's safe
/// surface-creation path, and **re-query** on recovery after a device loss
/// or a suspend/resume cycle.
///
/// - `HasWindowHandle + HasDisplayHandle` — the two safe accessors wgpu's own
///   `create_surface` calls internally; nothing here calls unsafe
///   window-handle extraction any more.
/// - `'static` — the renderer retains this value (in the crate-private
///   `SurfaceLease`) for as long as it needs a surface, which may outlive the
///   stack frame that constructed the renderer. A borrowed target
///   (`Renderer<'w>`) was considered and rejected (see the issue's ADR): it
///   forbids `drop(window)` but not `window.close()`, and is incompatible
///   with a `thread_local!` runtime registry and `spawn_local` futures on
///   web, both of which need `'static`.
/// - `Send + Sync` — wgpu's own `WindowHandle` marker trait (the bound its
///   safe `SurfaceTarget` requires) demands both, and the lease is rebuilt
///   from the raster owner thread on recovery, which must be able to hold
///   the `Arc<dyn WindowTarget>` across an `.await` without becoming `!Send`.
///
/// Blanket-implemented over every type that already satisfies the bounds —
/// effectively sealed, since there is nothing to implement by hand. This
/// mirrors the market shape rather than inventing one: iced's `Window` trait
/// takes the identical `HasWindowHandle + HasDisplayHandle + Send + Sync`
/// combination for its owned-target renderer, and wgpu's own `WindowHandle`
/// (`wgpu::WindowHandle` internal to `create_surface`) is `HasWindowHandle +
/// Send + Sync` for exactly the same "the surface owns this for its whole
/// life" reason.
pub trait WindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {}

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

    // Compile-time-only counterpart to the `renderer_new_rejects_borrowed_window`
    // compile-fail fixture: an `Arc<T>` — the shape every real call site
    // passes to `Renderer::new` (`Arc::clone(&window)`) — satisfies
    // `WindowTarget`. A `#[test]` fn wrapping this would run at test time
    // and prove nothing at runtime (the whole check is that it TYPE-CHECKS);
    // `assert_impl_all!` states that directly, with no GPU and no
    // `Renderer::new` call involved.
    static_assertions::assert_impl_all!(Arc<FakeWindow>: WindowTarget);
}
