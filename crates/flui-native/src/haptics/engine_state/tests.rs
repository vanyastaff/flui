//! The Core Haptics engine state, run on every host.

use super::{Engine, EngineState};

#[test]
fn a_restart_reports_the_engine_running() {
    let state = EngineState::new();
    state.set(Engine::Running);
    state.down();
    let ticket = state
        .restart_ticket()
        .expect("a stopped engine is owed a restart");
    assert!(state.restarted(ticket));
    assert_eq!(state.get(), Engine::Running);
    assert_eq!(state.restart_ticket(), None, "nothing is owed once it runs");
}

#[test]
fn a_late_restart_completion_leaves_a_newer_stop_owed() {
    let state = EngineState::new();
    state.set(Engine::Running);
    state.down();
    let ticket = state.restart_ticket().expect("owed");
    // The engine stops again on its own queue while the restart is in flight.
    state.down();
    assert!(
        !state.restarted(ticket),
        "the completion answers the earlier stop only"
    );
    assert_eq!(state.get(), Engine::Restarting);
    let newer = state
        .restart_ticket()
        .expect("the newer stop is still owed");
    assert_ne!(newer, ticket);
    assert!(state.restarted(newer));
    assert_eq!(state.get(), Engine::Running);
}

#[test]
fn creation_keeps_the_epoch_and_the_state_round_trips() {
    let state = EngineState::new();
    assert_eq!(state.get(), Engine::NotCreated);
    assert_eq!(state.restart_ticket(), None);
    for engine in [
        Engine::Running,
        Engine::Restarting,
        Engine::Unsupported,
        Engine::NotCreated,
    ] {
        state.set(engine);
        assert_eq!(state.get(), engine);
    }
    state.down();
    let ticket = state.restart_ticket().expect("owed");
    state.set(Engine::Restarting);
    assert!(state.restarted(ticket), "set keeps the stop's epoch");
}

#[test]
fn a_stop_while_the_engine_first_starts_is_owed_a_restart() {
    let fresh = EngineState::new();
    assert!(fresh.started());
    assert_eq!(fresh.get(), Engine::Running);

    let stopped = EngineState::new();
    // The stopped handler runs on Core Haptics' queue before the start returns.
    stopped.down();
    assert!(!stopped.started(), "the stop is newer than the start");
    assert_eq!(stopped.get(), Engine::Restarting);
    assert!(stopped.restart_ticket().is_some());
}
