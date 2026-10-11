//! The thread-safe executor can outlive its owner and retire on an async lane.

use flui_platform::executor::BackgroundExecutor;

fn last_executor_owner_retires_inside_another_runtime() {
    let foreign = BackgroundExecutor::new();
    let retiring = BackgroundExecutor::new();
    retiring.block(async {});
    foreign.block(async move { drop(retiring) });
}

fn last_executor_owner_retires_inside_its_own_task() {
    let executor = BackgroundExecutor::new();
    let final_owner = executor.clone();
    let (start, begin) = tokio::sync::oneshot::channel();
    let (finished, done) = std::sync::mpsc::channel();
    executor
        .spawn(async move {
            begin.await.expect("retirement admitted");
            drop(final_owner);
            finished.send(()).expect("retirement witness");
        })
        .detach();
    drop(executor);
    start.send(()).expect("last owner task is alive");
    done.recv_timeout(std::time::Duration::from_secs(5))
        .expect("task completes after retiring its executor");
}

#[test]
fn background_executor_retirement() {
    crate::run_table(
        "background_executor_retirement",
        &[
            (
                "last_executor_owner_retires_inside_another_runtime",
                last_executor_owner_retires_inside_another_runtime,
            ),
            (
                "last_executor_owner_retires_inside_its_own_task",
                last_executor_owner_retires_inside_its_own_task,
            ),
        ],
    );
}
