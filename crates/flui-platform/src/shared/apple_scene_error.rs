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
