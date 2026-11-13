//! A delivery bound to an application handler from the crate's test kit
//! ([`crate::testing::app`]), for the loops' and features' own tests.

use std::rc::Rc;
use std::sync::Arc;

pub(crate) use crate::testing::app::{AppCall, AppLog, on_ready};

use super::AppDelivery;
use crate::owner::signal::OwnerSignal;
use crate::owner::{DirectOwnerHooks, OwnerHooks, OwnerPlatform};
use crate::platform::{AppHandler, Platform};

/// A delivery bound to a handler on this thread over a headless platform, and
/// the platform and owner hooks the delivery holds weakly.
pub(crate) struct Bound {
    pub(crate) delivery: Arc<AppDelivery>,
    pub(crate) platform: Arc<dyn Platform>,
    pub(crate) hooks: Rc<dyn OwnerHooks>,
}

impl Bound {
    /// The owner capability `ready` is handed.
    pub(crate) fn owner(&self) -> OwnerPlatform {
        OwnerPlatform::new(Arc::clone(&self.platform), Rc::clone(&self.hooks))
    }
}

/// `delivery` bound to `handler` on this thread, over a headless platform.
pub(crate) fn bind(delivery: Arc<AppDelivery>, handler: Rc<dyn AppHandler>) -> Bound {
    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "ADR-0327 §1: a platform is shared as `Rc` once every host is owner-local (§5)"
    )]
    let platform: Arc<dyn Platform> = Arc::new(crate::HeadlessPlatform::new());
    let signal = OwnerSignal::new(Arc::new(|| Ok(())));
    let hooks: Rc<dyn OwnerHooks> = Rc::new(DirectOwnerHooks::with_signal(
        Arc::clone(&platform),
        signal,
        Rc::default(),
    ));
    delivery.bind(handler, &platform, &hooks);
    Bound {
        delivery,
        platform,
        hooks,
    }
}

/// A new delivery bound to `handler`, whose `ready` has run.
pub(crate) fn bound(handler: Rc<dyn AppHandler>) -> Bound {
    let run = bind(Arc::new(AppDelivery::new()), handler);
    run.delivery
        .ready(run.owner())
        .expect("the recording handler's bootstrap succeeds");
    run
}
