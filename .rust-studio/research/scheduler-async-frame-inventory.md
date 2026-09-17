QUESTION: Does the current flui scheduler/async/frame pipeline still expose a new, nonduplicate architectural problem comparable to Flutter's lifecycle/phase issues?

ANSWER: This pass did not find a new confirmed nonduplicate issue. The scheduler now has explicit begin -> mid-frame async -> persistent -> pipeline -> post-frame ordering, weak scheduler handles for long-lived capabilities, owner-affine local post-frame lanes, and tests for the main frame/async failure modes. Existing issues #1055, #1056, #1057, #1058, #1038, #1062, and #1063 still cover known scheduler/async risks.

VERSIONS: flui workspace current checkout, inspected 2026-09-13.

SOURCES:
- `crates/flui-scheduler/AGENTS.md`: scheduler ownership contract says `UpdateScheduler` owns logical time only; `LocalPostFrameLane` is owner-affine and non-`Send`.
- `crates/flui-scheduler/src/scheduler.rs`: `handle_begin_frame` polls async tasks in `MidFrameMicrotasks`; `drive_frame_impl` orders begin/draw/pipeline/end and aborts panicking pipelines; `set_on_frame_scheduled` documents cross-thread wake hazards already tracked by #949.
- `crates/flui-scheduler/src/async_driver.rs`: wakers coalesce false->true ready transitions, use `Weak<Inner>`, poll outside the task-map lock, cancel on `TaskToken` drop, and include `spawn_local_eager` for FutureBuilder's synchronous completion window.
- `crates/flui-scheduler/src/post_frame.rs`: shared/local post-frame callbacks share one id order; local handles are `!Send`; wrong-scheduler lane drains are typed errors and leave queues intact.
- `crates/flui-view/src/element/future_builder.rs`: subscriptions capture `RebuildHandle`/`AsyncDriver` in `init_state`, use generation guards, cancel tokens on key change/dispose, and test synchronous-ready futures.
- `crates/flui-view/src/element/stream_builder.rs`: streams intentionally avoid eager polling, preserve Flutter-like waiting-before-first-event semantics, use generation guards, and cancel on key change/dispose.
- `crates/flui-app/tests/runner_frame_ordering.rs`: source-scan regression guards pin production runner sites to `drive_frame_with_lane` and `finish_async_pump` + `drive_async_tasks`.

EXECUTED:
- `cargo nextest run -p flui-scheduler --all-targets --no-fail-fast`: 416 tests run, 416 passed, 0 skipped.
- `cargo nextest run -p flui-app --test runner_frame_ordering --no-fail-fast`: 3 tests run, 3 passed, 0 skipped.
- `cargo nextest run -p flui-view --lib -E 'test(element::future_builder::tests::) | test(element::stream_builder::tests::)' --no-fail-fast`: 33 tests run, 33 passed, 418 skipped.
- `gh issue list --repo vanyastaff/flui --state all --limit 120 --search 'scheduler frame async task wake cancellation post_frame end_of_frame PumpAsync drive_async_tasks'`: no additional matching issue beyond known ledger items was returned in this pass.
- Similar issue searches for FutureBuilder/StreamBuilder async cancellation and ticker/scheduler wake returned no new matches.

OPEN:
- Existing known issues remain live: #1055 end_of_frame demand/cancel, #1056 dormant async O(N), #1057 scheduler-owned panic recovery, #1058 legacy frame callback lock reentry, #1038 destructor-under-lock cancellation, #1062 orphan image work, #1063 CPU decode on I/O executor.
- `set_on_frame_scheduled` still documents a cross-thread platform wake hazard tracked elsewhere (#949); not filed here because this pass did not trace/execute the macOS backend.
- Broader platform task/service shutdown and reduced-motion/animation integration remain outside this narrow L/M pass.
