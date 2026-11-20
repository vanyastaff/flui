//! Recovery of the public first-frame presentation gate.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_runtime::renderer_binding::RenderingBinding;

pub(crate) fn unmatched_first_frame_release_preserves_the_next_deferral() {
    let binding = RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
    for _ in 0..2 {
        assert!(catch_unwind(AssertUnwindSafe(|| binding.allow_first_frame())).is_err());
        assert!(
            binding.send_frames_to_engine(),
            "a refused release defers nothing"
        );
    }
    binding.defer_first_frame();
    assert!(
        !binding.send_frames_to_engine(),
        "the new deferral withholds the frame"
    );
    binding.allow_first_frame();
    assert!(
        binding.send_frames_to_engine(),
        "the matched release permits it"
    );
    // The refusal remains harmless after the gate reopened too.
    assert!(catch_unwind(AssertUnwindSafe(|| binding.allow_first_frame())).is_err());
    assert!(binding.send_frames_to_engine());
}
