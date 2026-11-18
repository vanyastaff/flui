//! A quit the application or the user asked for: every presentation is asked
//! as for the program's close, a refusal with a record defers the quit until
//! that record is carried out, and one without cancels it (ADR-0350 §1).

use flui_foundation::{PresentationAddress, UiRuntimeId};
use flui_view::CloseReason;

use super::{CloseRequestRouter, CloseResponse, SessionEnd};
use crate::ui_runtime::SettledCloses;

/// Where a quit stands, from the ask until the host commits it or it is
/// cancelled. The host keeps one for its application and moves it only
/// through these methods, so every host defers, cancels and commits a quit
/// on the same answers.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct QuitRequest(QuitState);

#[derive(Debug, Default, PartialEq, Eq)]
enum QuitState {
    /// No quit is asked.
    #[default]
    None,
    /// The host asks every presentation on its next turn.
    Asked,
    /// The host is asking now.
    Consulting,
    /// Presentations refused, each recording the close on its guard; the
    /// quit waits for each record to be carried out.
    Deferred(Vec<PresentationAddress>),
}

/// What the presentations answered a quit ([`ask_quit`]).
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuitAnswer {
    /// A refusal recorded nothing to wait on: the quit is cancelled.
    Cancelled,
    /// The presentations whose recorded close the quit waits on; none, and
    /// the quit goes ahead.
    Waiting(Vec<PresentationAddress>),
}

/// What a change of a [`QuitRequest`] asks of its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "a committed quit is carried out by the host, an abandoned one shows what waited"]
#[non_exhaustive]
pub enum QuitVerdict {
    /// Nothing for the host to do.
    Unchanged,
    /// The quit waits for the presentations that refused with a record.
    Deferred,
    /// The quit was abandoned: a presentation refused without a record or
    /// stayed open.
    Abandoned,
    /// No presentation holds the quit: the host carries it out now.
    Commit,
}

impl QuitRequest {
    /// No quit asked.
    #[must_use]
    pub const fn new() -> Self {
        Self(QuitState::None)
    }

    /// Ask for a quit: the host's next turn asks every presentation. A quit
    /// asked while one is deferred asks them all again.
    pub fn ask(&mut self) {
        self.0 = QuitState::Asked;
    }

    /// Whether a quit is asked, being asked or deferred.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.0 != QuitState::None
    }

    /// Whether a quit is asked and waits for the host's turn to ask the
    /// presentations.
    #[must_use]
    pub fn is_asked(&self) -> bool {
        self.0 == QuitState::Asked
    }

    /// Start asking the presentations: `true` when a quit is asked and not
    /// being asked already.
    pub fn begin_consult(&mut self) -> bool {
        if self.0 != QuitState::Asked {
            return false;
        }
        self.0 = QuitState::Consulting;
        true
    }

    /// End the consultation [`begin_consult`](Self::begin_consult) started
    /// with what the presentations answered. A quit asked again while they
    /// were asked stays asked for the next turn, and this answer is dropped.
    pub fn finish_consult(&mut self, answer: QuitAnswer) -> QuitVerdict {
        if self.0 != QuitState::Consulting {
            return QuitVerdict::Unchanged;
        }
        match answer {
            QuitAnswer::Cancelled => {
                self.0 = QuitState::None;
                tracing::info!(
                    "a presentation refused the quit without recording it; quit cancelled"
                );
                QuitVerdict::Abandoned
            }
            QuitAnswer::Waiting(waiting) if !waiting.is_empty() => {
                self.0 = QuitState::Deferred(waiting);
                QuitVerdict::Deferred
            }
            QuitAnswer::Waiting(_) => {
                self.0 = QuitState::None;
                QuitVerdict::Commit
            }
        }
    }

    /// The presentation at `address` gave the deferred quit its consent: its
    /// recorded close was carried out, or it is gone. The quit commits once
    /// no presentation is left to wait on.
    pub fn consented(&mut self, address: PresentationAddress) -> QuitVerdict {
        let QuitState::Deferred(waiting) = &mut self.0 else {
            return QuitVerdict::Unchanged;
        };
        waiting.retain(|waited| *waited != address);
        if waiting.is_empty() {
            self.0 = QuitState::None;
            QuitVerdict::Commit
        } else {
            QuitVerdict::Unchanged
        }
    }

    /// Every presentation the deferred quit waits on and that is no longer in
    /// `registered` consents: a window that is gone holds no quit.
    pub fn forget_gone(&mut self, registered: &[PresentationAddress]) -> QuitVerdict {
        let gone: Vec<_> = self
            .waiting()
            .iter()
            .copied()
            .filter(|address| !registered.contains(address))
            .collect();
        let mut verdict = QuitVerdict::Unchanged;
        for address in gone {
            if self.consented(address) == QuitVerdict::Commit {
                verdict = QuitVerdict::Commit;
            }
        }
        verdict
    }

    /// The presentation at `address` kept itself open: the deferred quit that
    /// waited on it is abandoned.
    pub fn declined(&mut self, address: PresentationAddress) -> QuitVerdict {
        if matches!(&self.0, QuitState::Deferred(waiting) if waiting.contains(&address)) {
            self.0 = QuitState::None;
            tracing::info!(
                ?address,
                "a presentation stayed open; deferred quit cancelled"
            );
            QuitVerdict::Abandoned
        } else {
            QuitVerdict::Unchanged
        }
    }

    /// The presentations a deferred quit still waits on.
    #[must_use]
    pub fn waiting(&self) -> &[PresentationAddress] {
        match &self.0 {
            QuitState::Deferred(waiting) => waiting,
            QuitState::None | QuitState::Asked | QuitState::Consulting => &[],
        }
    }

    /// Forget the quit: the host committed to shutting down past it.
    pub fn clear(&mut self) {
        self.0 = QuitState::None;
    }
}

/// Ask each of `presentations`, in order, whether the quit may go ahead, as a
/// close for [`CloseReason::Program`]: its guard first, then its handler
/// ([`CloseRequestRouter::consult`]). Every one is asked, not only up to the
/// first refusal, so each that refuses records its close. A refusal that
/// recorded nothing (a handler that panicked, a presentation without a
/// guard) cancels the quit.
///
/// Runs application code: call it with no borrow of the host's state held.
pub fn ask_quit(
    router: &CloseRequestRouter,
    presentations: impl IntoIterator<Item = PresentationAddress>,
) -> QuitAnswer {
    let mut waiting = Vec::new();
    let mut cancelled = false;
    for address in presentations {
        if router.consult(address, CloseReason::Program) == CloseResponse::KeepOpen {
            if router.is_recorded(address, CloseReason::Program) {
                waiting.push(address);
            } else {
                cancelled = true;
            }
        }
    }
    if cancelled {
        QuitAnswer::Cancelled
    } else {
        QuitAnswer::Waiting(waiting)
    }
}

/// One thing the owner turn does with a close its guards settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettledStep {
    /// Close the presentation's window, without asking again.
    Close(PresentationAddress),
    /// The presentation consents to a deferred quit
    /// ([`QuitRequest::consented`]).
    QuitConsented(PresentationAddress),
    /// The presentation kept itself open against a deferred quit
    /// ([`QuitRequest::declined`]).
    QuitDeclined(PresentationAddress),
}

/// What the owner turn does with the closes runtime `runtime`'s guards
/// settled, in order: a due close for the user or the session's end closes
/// its window; a due close for the program closes nothing alone, since it is
/// the presentation's consent to the quit the application deferred, and a
/// window close carried out consents too, since the window is gone either
/// way; a program close the application declined abandons that quit.
///
/// A window that closes no longer holds the session end it refused,
/// whichever record its guard carried out, so its refusal is released from
/// `session_end` here.
#[must_use]
pub fn settled_steps(
    settled: SettledCloses,
    runtime: UiRuntimeId,
    session_end: &SessionEnd,
) -> Vec<SettledStep> {
    let address = |presentation_id| PresentationAddress {
        ui_runtime_id: runtime,
        presentation_id,
    };
    let mut steps = Vec::with_capacity(settled.due.len() * 2 + settled.declined.len());
    for (presentation, reason) in settled.due {
        let at = address(presentation);
        if reason != CloseReason::Program {
            session_end.release_refusal(at);
            steps.push(SettledStep::Close(at));
        }
        steps.push(SettledStep::QuitConsented(at));
    }
    for (presentation, reason) in settled.declined {
        if reason == CloseReason::Program {
            steps.push(SettledStep::QuitDeclined(address(presentation)));
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use flui_foundation::PresentationId;

    use super::*;

    fn at(presentation: usize) -> PresentationAddress {
        PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(presentation),
        }
    }

    /// A deferred quit commits when its last waiter consents, a gone waiter
    /// consents, and one that stays open abandons it; an answer that arrives
    /// after the quit was asked again is dropped.
    #[test]
    fn a_quit_moves_on_its_presentations_answers() {
        let mut quit = QuitRequest::new();
        assert!(!quit.begin_consult(), "nothing asked");
        quit.ask();
        assert!(quit.begin_consult());
        assert_eq!(
            quit.finish_consult(QuitAnswer::Waiting(vec![at(1), at(2)])),
            QuitVerdict::Deferred
        );
        assert_eq!(quit.consented(at(1)), QuitVerdict::Unchanged);
        assert_eq!(quit.forget_gone(&[at(1)]), QuitVerdict::Commit);
        assert!(!quit.is_pending());

        quit.ask();
        assert!(quit.begin_consult());
        assert_eq!(
            quit.finish_consult(QuitAnswer::Waiting(vec![at(3)])),
            QuitVerdict::Deferred
        );
        assert_eq!(quit.declined(at(4)), QuitVerdict::Unchanged);
        assert_eq!(quit.declined(at(3)), QuitVerdict::Abandoned);

        quit.ask();
        assert!(quit.begin_consult());
        quit.ask();
        assert_eq!(
            quit.finish_consult(QuitAnswer::Waiting(Vec::new())),
            QuitVerdict::Unchanged,
            "a quit asked again during the consultation is asked again"
        );
        assert!(quit.begin_consult());
        assert_eq!(
            quit.finish_consult(QuitAnswer::Cancelled),
            QuitVerdict::Abandoned
        );
    }
}
