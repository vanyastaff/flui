//! The asynchronous UIKit destruction-error ABI boundary.
use block2::RcBlock;
use objc2_foundation::NSError;
use std::ptr::NonNull;

pub(crate) fn destruction_error_handler() -> RcBlock<dyn Fn(NonNull<NSError>)> {
    RcBlock::new(|error: NonNull<NSError>| {
        super::panic_boundary::contain_owner_callback(|| {
            tracing::error!(
                ?error,
                "UIKit rejected scene destruction; logical window remains closed"
            );
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Subscriber(Arc<AtomicUsize>, bool);
    impl tracing::Subscriber for Subscriber {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
            if self.1 {
                struct Hostile;
                impl Drop for Hostile {
                    fn drop(&mut self) {
                        panic!("hostile payload drop");
                    }
                }
                std::panic::panic_any(Hostile);
            }
            panic!("late subscriber panic");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    #[test]
    fn delayed_native_error_block_contains_hostile_subscriber() {
        let calls = Arc::new(AtomicUsize::new(0));
        let handler = destruction_error_handler();
        // SAFETY: valid owned NSString domain; no userInfo objects or borrowed payloads.
        #[expect(
            unsafe_code,
            reason = "construct native NSError for the actual error block"
        )]
        let error = unsafe {
            NSError::errorWithDomain_code_userInfo(
                &objc2_foundation::NSString::from_str("FLUI probe"),
                1,
                None,
            )
        };
        // Invoke after construction returned, without any scene-action catch.
        tracing::subscriber::with_default(Subscriber(calls.clone(), true), || {
            handler.call((NonNull::from(&*error),));
        });
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }
}
