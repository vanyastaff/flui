//! A scripted [`LauncherBackend`] for scenarios: the double of the launcher
//! contract (ADR-0154 §10, ADR-0243 D4).

use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};

use crate::capability::Resolver;
use crate::launch::{ExternalUri, LaunchResult, LauncherBackend};

/// One request a [`ScriptedLauncher`] received.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaunchRequest {
    /// [`Launcher::open_uri`](crate::launch::Launcher::open_uri).
    OpenUri(ExternalUri),
    /// [`Launcher::open_path`](crate::launch::Launcher::open_path).
    OpenPath(PathBuf),
    /// [`Launcher::reveal_path`](crate::launch::Launcher::reveal_path).
    RevealPath(PathBuf),
    /// [`Launcher::trash_path`](crate::launch::Launcher::trash_path).
    TrashPath(PathBuf),
}

#[derive(Default)]
struct State {
    requests: Vec<LaunchRequest>,
    /// Answers for the next requests, oldest first; `Ok(())` once empty.
    script: VecDeque<LaunchResult>,
    /// While set, requests wait in `held` instead of being answered.
    holding: bool,
    held: VecDeque<Resolver<LaunchResult>>,
}

/// A launcher whose system is a script: it records every request and
/// answers each with the next scripted result, `Ok(())` when none is
/// scripted, or holds it until the test releases it.
///
/// Cheap to clone; clones share the script and the record, so a test keeps
/// one and hands another to the provider.
#[derive(Clone, Default)]
pub struct ScriptedLauncher {
    state: Rc<RefCell<State>>,
}

impl ScriptedLauncher {
    /// A launcher that answers every request `Ok(())`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer the next unscripted request with `result`.
    pub fn answer_next(&self, result: LaunchResult) {
        self.state.borrow_mut().script.push_back(result);
    }

    /// Hold every later request until [`release`](Self::release).
    pub fn hold(&self) {
        self.state.borrow_mut().holding = true;
    }

    /// Answer the oldest held request with `result`; `false` when none is
    /// held.
    pub fn release(&self, result: LaunchResult) -> bool {
        let held = self.state.borrow_mut().held.pop_front();
        held.is_some_and(|answer| {
            // A retired owner leaves nothing waiting for the answer.
            let _ = answer.resolve(result);
            true
        })
    }

    /// The requests received so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LaunchRequest> {
        self.state.borrow().requests.clone()
    }

    fn receive(&self, request: LaunchRequest, answer: Resolver<LaunchResult>) {
        let scripted = {
            let mut state = self.state.borrow_mut();
            state.requests.push(request);
            if state.holding {
                state.held.push_back(answer);
                return;
            }
            (state.script.pop_front().unwrap_or(Ok(())), answer)
        };
        let (result, answer) = scripted;
        let _ = answer.resolve(result);
    }
}

impl LauncherBackend for ScriptedLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        self.receive(LaunchRequest::OpenUri(uri), answer);
    }

    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.receive(LaunchRequest::OpenPath(path), answer);
    }

    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.receive(LaunchRequest::RevealPath(path), answer);
    }

    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.receive(LaunchRequest::TrashPath(path), answer);
    }
}

impl std::fmt::Debug for ScriptedLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedLauncher")
            .field("requests", &state.requests.len())
            .field("held", &state.held.len())
            .finish_non_exhaustive()
    }
}
