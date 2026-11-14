### Added

- `flui_engine::RasterExecutor::spawn` starts the executor on its own raster thread and answers
  an `ExecutorHandle` with the sender: a send wakes the thread, a slot holds two settlements
  undrained, and `ExecutorHandle::detach` and `shut_down` wait at most
  `DETACH_DEADLINE`. A thread that does not answer in time is `ExecutorHealth::Died`, and
  `RasterSender::take_executor_failure` answers why (`ExecutorFailure::Wedged`), as does a
  contained panic of a turn (`ExecutorFailure::TurnPanicked`). No runner uses the thread yet.
- `flui_engine::testing::ScriptedSlots` can be handed to a raster thread, and
  `ScriptedSlots::hold_next_render` stands for a render that does not answer.
