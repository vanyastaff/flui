### Changed

- `HeroController::new` and `HeroController::with_rect_tween` return `Rc<HeroController>`, and
  `HeroControllerScope::new` takes one: the navigator keeps its state on the owner thread in `Rc`,
  `RefCell` and `Cell` (ADR-0323 §1). A navigator re-entered while it is busy now panics on the
  second borrow instead of hanging the thread.
