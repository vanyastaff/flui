//! The asynchronous UIKit destruction-error ABI boundary.
use block2::RcBlock;
use objc2_foundation::NSError;
use std::ptr::NonNull;

pub(crate) fn destruction_error_handler() -> RcBlock<dyn Fn(NonNull<NSError>)> {
    RcBlock::new(|error: NonNull<NSError>| {
        crate::owner::panic_boundary::os_entry(
            "the scene destruction's error handler",
            (),
            crate::owner::panic_boundary::AfterPanic::Answer(|()| ()),
            || {
                tracing::error!(
                    ?error,
                    "UIKit rejected scene destruction; logical window remains closed"
                );
            },
        );
    })
}
