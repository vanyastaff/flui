//! The order of one turn of the page's `requestAnimationFrame` loop, which
//! runs on every host so the order is tested off the browser.

/// Run one animation frame: `ask_next` asks the browser for the next frame
/// before `work` runs, so work that unwinds (a `panic = "unwind"` build)
/// cannot end the loop.
pub(crate) fn turn(ask_next: impl FnOnce(), work: impl FnOnce()) {
    ask_next();
    work();
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::turn;

    #[test]
    fn a_frame_whose_work_unwinds_has_already_asked_for_the_next() {
        let asked = Cell::new(false);
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            turn(|| asked.set(true), || panic!("the frame's work unwinds"));
        }));
        assert!(unwound.is_err());
        assert!(
            asked.get(),
            "the next frame is asked for before the work, or an unwinding frame ends the loop"
        );
    }
}
