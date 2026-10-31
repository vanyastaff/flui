//! The loop's terminal failure (ADR-0258 §2, §3): a panic nothing is left to
//! continue after waits in the host's slot while the loop quits and the exit
//! order writes what the application persisted, and is then resumed, so the
//! process exits 101 with the panic's text printed once, where it happened.

use std::any::Any;

use super::host::APP_RUNTIME;

/// Hold `payload` as the loop's terminal failure, unless one is held
/// already: the first stays, a later one is logged and retained. The slot
/// keeps text: an inert payload (`&'static str`, `String`) as it is, an
/// opaque one's description, with the payload itself retained (ADR-0127).
pub(super) fn hold_terminal(payload: Box<dyn Any + Send>, during: &'static str) {
    let held = if payload.is::<&'static str>() || payload.is::<String>() {
        payload
    } else {
        flui_foundation::panic::discard_payload(payload);
        Box::new(String::from("a panic whose payload is not text"))
    };
    let refused = APP_RUNTIME.with(|slot| match slot.try_borrow_mut() {
        Ok(mut state) if state.terminal.is_none() => {
            state.terminal = Some(held);
            None
        }
        Ok(_) | Err(_) => Some(held),
    });
    if let Some(later) = refused {
        contain_diagnostic(|| {
            tracing::error!(
                during,
                panic = flui_foundation::panic::payload_text(later.as_ref()),
                "a later terminal failure; the first one is the one resumed"
            );
        });
        flui_foundation::panic::discard_payload(later);
    }
}

/// Take the loop's terminal failure, to resume once the exit order ran.
#[cfg_attr(
    target_arch = "wasm32",
    expect(
        dead_code,
        reason = "the web loop never returns, so no exit order runs after it"
    )
)]
pub(super) fn take_terminal() -> Option<Box<dyn Any + Send>> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut()
            .ok()
            .and_then(|mut state| state.terminal.take())
    })
}

/// Run `body`, a step of the loop's exit that drops application values: a
/// panic in it is the loop's terminal failure (ADR-0258 §2), held for the
/// exit's end while the rest of the exit runs.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn contain_terminal(during: &'static str, body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        hold_terminal(payload, during);
    }
}

/// Run `body`, a platform handler's whole method (ADR-0258 §4): a panic that
/// leaves it is held as the loop's terminal failure, the loop is asked to
/// quit, and the method answers `failed` instead of unwinding into the
/// platform.
pub(super) fn contained<R>(failed: R, during: &'static str, body: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(answer) => answer,
        Err(payload) => {
            hold_terminal(payload, during);
            request_terminal_quit();
            failed
        }
    }
}

/// Ask the loop to end after a terminal failure, as a program quit does.
fn request_terminal_quit() {
    let quit = std::panic::catch_unwind(|| {
        let _ = super::host::with_owner_platform(flui_native::OwnerPlatform::quit);
    });
    if let Err(payload) = quit {
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Run `report`, a log of a failure already contained: a subscriber that
/// panics while it reports cannot replace the failure, so its payload is
/// retained.
pub(super) fn contain_diagnostic(report: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(report)) {
        flui_foundation::panic::discard_payload(payload);
    }
}
