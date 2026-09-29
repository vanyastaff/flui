//! Unit test of the `Navigator`'s named-route registry, which needs no mounted
//! element tree. The mounted suite lives in `crates/flui-widgets/tests/navigator.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::prelude::*;

use super::named_route::RouteRequest;
use super::navigator::NavigatorHandle;
use super::overlay_route::SimpleRoute;
use crate::SizedBox;

// ============================================================================
// Named-route registry
// ============================================================================

/// Displacing a registration drops a **user closure**, and that closure's `Drop`
/// may reach back into the registry. It must not deadlock.
///
/// `parking_lot::Mutex` is not reentrant and there is no compile-time oracle for
/// a self-locking call — it compiles clean and hangs at run time. The path is
/// not exotic here: the closure being dropped by `clear_routes` is, by design,
/// the one that captured a `NavigatorHandle`, so its captured state is exactly
/// what a `Drop` impl would hang off.
///
/// Both directions are covered: re-registering a name (which drops the entry it
/// displaces) and `clear_routes` (which drops every entry at once).
///
/// Red-check: drop the displaced entry *inside* the guard — in
/// `RouteRegistry::register_named`, bind `previous` inside the locked block
/// instead of outside it; in `clear`, call `table.clear()` under the guard
/// rather than moving the map out. This test then hangs rather than failing,
/// which is why it is written to make progress observable (`re_registrations`
/// rises) instead of only asserting at the end.
pub(super) fn a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry() {
    /// Registers another route from its own `Drop` — the self-locking shape.
    struct ReRegisterOnDrop {
        navigator: NavigatorHandle,
        ran: Arc<AtomicUsize>,
    }

    impl Drop for ReRegisterOnDrop {
        fn drop(&mut self) {
            self.ran.fetch_add(1, Ordering::Relaxed);
            self.navigator
                .route("/dropped-into", |_request: &RouteRequest<'_>| {
                    Some(SimpleRoute::<u32>::new(|_ctx| {
                        SizedBox::new(1.0, 1.0).into_view().boxed()
                    }))
                });
        }
    }

    let re_registrations = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();

    // A factory whose captured state re-enters the registry when dropped.
    let hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.route("/replaced", move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &hook;
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });

    // Displacing it drops the hook. Under a guard held across the drop this
    // deadlocks the owner thread.
    handle.route("/replaced", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        1,
        "the displaced closure's Drop ran, and re-entered the registry without \
         deadlocking"
    );
    assert!(
        handle.push_named("/dropped-into").is_ok(),
        "and its re-entrant registration actually landed"
    );

    // The same, through `clear_routes`, which drops every entry at once.
    let hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.route("/cleared", move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &hook;
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });
    handle.clear_routes();
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        2,
        "clear_routes dropped the closure outside its guard too"
    );

    // And the two generator hooks, which displace by assignment rather than by
    // `HashMap::insert` — the same hazard, reached a different way. Written out
    // twice rather than looped: the two methods take `impl Fn`, so there is no
    // one value that installs either.
    let generator_hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.on_generate_route(move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &generator_hook;
        None
    });
    handle.on_generate_route(|_request: &RouteRequest<'_>| None);
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        3,
        "a replaced on_generate_route hook is dropped outside the guard too"
    );

    let fallback_hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.on_unknown_route(move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &fallback_hook;
        None
    });
    handle.on_unknown_route(|_request: &RouteRequest<'_>| None);
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        4,
        "and so is a replaced on_unknown_route hook"
    );
}
