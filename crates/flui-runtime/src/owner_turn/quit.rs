//! A quit the application or the user asked for: every presentation is asked
//! as for the program's close, a refusal with a record defers the quit until
//! that record is carried out, and one without cancels it (ADR-0350 §1).

use flui_foundation::panic::FirstPanic;
use flui_foundation::{PresentationAddress, UiRuntimeId};
use flui_view::CloseReason;

use super::{CloseRequestRouter, CloseResponse, SessionEnd};
use crate::ui_runtime::SettledCloses;

/// Where a quit stands, from the ask until the host commits it or it is
/// cancelled. The host keeps one for its application and moves it only
/// through these methods, so every host defers, cancels and commits a quit
/// on the same answers. No method runs code it does not own (a `tracing`
/// event included), so a host may move it under its own lock and act on the
/// verdict once the lock is released.
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

/// What the presentations answered a quit ([`QuitConsultation::ask`]).
#[derive(Debug, PartialEq, Eq)]
enum QuitAnswer {
    /// A refusal recorded nothing to wait on: the quit is cancelled.
    Cancelled,
    /// Code the consultation ran panicked: no answer, and the quit is
    /// cancelled.
    Panicked,
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

    /// Start asking the presentations: the consultation when a quit is asked
    /// and not being asked already. The host asks it with no borrow of its
    /// own state held ([`QuitConsultation::ask`]) and hands the answer back
    /// to [`finish_consult`](Self::finish_consult).
    pub fn begin_consult(&mut self) -> Option<QuitConsultation> {
        if self.0 != QuitState::Asked {
            return None;
        }
        self.0 = QuitState::Consulting;
        Some(QuitConsultation(()))
    }

    /// End the consultation [`begin_consult`](Self::begin_consult) started
    /// with what the presentations answered. A quit asked again while they
    /// were asked stays asked for the next turn, and this answer is dropped;
    /// its panic is still handed on to resume.
    pub fn finish_consult(&mut self, answered: QuitAnswered) -> QuitFinished {
        let QuitAnswered { answer, panic } = answered;
        let verdict = if self.0 == QuitState::Consulting {
            match answer {
                QuitAnswer::Cancelled | QuitAnswer::Panicked => {
                    self.0 = QuitState::None;
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
        } else {
            QuitVerdict::Unchanged
        };
        QuitFinished { verdict, panic }
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

/// A quit's consultation of its presentations, which only
/// [`QuitRequest::begin_consult`] hands out: [`ask`](Self::ask) consumes it
/// into the [`QuitAnswered`] that [`QuitRequest::finish_consult`] alone
/// takes, so a host cannot ask without finishing or finish without asking.
/// Nothing runs when one is dropped.
#[derive(Debug)]
#[must_use = "a begun consultation is asked, and its answer finished with QuitRequest::finish_consult"]
pub struct QuitConsultation(());

impl QuitConsultation {
    /// Ask each of `presentations`, in order, whether the quit may go ahead,
    /// as a close for [`CloseReason::Program`]: its guard first, then its
    /// handler ([`CloseRequestRouter::consult`]). Every one is asked, not
    /// only up to the first refusal, so each that refuses records its close.
    /// A refusal that recorded nothing (a handler that panicked, a
    /// presentation without a guard) cancels the quit.
    ///
    /// A panic that escapes a presentation's question (the report of its
    /// handler's panic reaching a log subscriber that fails, the drop of a
    /// handler that closed its own window) is kept, not unwound: that
    /// presentation gave no answer, the quit is cancelled as
    /// [`QuitFinished::panicked`], every later presentation is still asked,
    /// and the host resumes the first such panic once it has finished the
    /// consultation ([`QuitFinished::resume`]).
    ///
    /// Runs application code: call it with no borrow of the host's state
    /// held.
    pub fn ask(
        self,
        router: &CloseRequestRouter,
        presentations: impl IntoIterator<Item = PresentationAddress>,
    ) -> QuitAnswered {
        let mut panic = FirstPanic::new();
        let mut waiting = Vec::new();
        let mut refused_unrecorded = false;
        for address in presentations {
            let recorded = panic.contain(|| {
                let response = router.consult(address, CloseReason::Program);
                (response == CloseResponse::KeepOpen)
                    .then(|| router.is_recorded(address, CloseReason::Program))
            });
            // Not asked to stay open, or a panic, which is no answer to wait
            // on (`panic` holds the first), adds nothing to wait for.
            match recorded {
                Some(Some(true)) => waiting.push(address),
                Some(Some(false)) => refused_unrecorded = true,
                Some(None) | None => {}
            }
        }
        let answer = if panic.is_held() {
            QuitAnswer::Panicked
        } else if refused_unrecorded {
            QuitAnswer::Cancelled
        } else {
            QuitAnswer::Waiting(waiting)
        };
        QuitAnswered { answer, panic }
    }
}

/// What a quit's presentations answered ([`QuitConsultation::ask`]), for
/// [`QuitRequest::finish_consult`].
#[must_use = "a quit's answer is finished with QuitRequest::finish_consult"]
pub struct QuitAnswered {
    answer: QuitAnswer,
    panic: FirstPanic,
}

impl std::fmt::Debug for QuitAnswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuitAnswered")
            .field("answer", &self.answer)
            .field("panicked", &self.panic.is_held())
            .finish()
    }
}

/// A finished consultation ([`QuitRequest::finish_consult`]): the verdict the
/// host follows, and the first panic the consultation kept, which the host
/// resumes once it has followed the verdict and released its own state.
#[must_use = "a finished consultation's verdict is followed and its panic resumed"]
pub struct QuitFinished {
    verdict: QuitVerdict,
    panic: FirstPanic,
}

impl QuitFinished {
    /// What the host carries out.
    pub fn verdict(&self) -> QuitVerdict {
        self.verdict
    }

    /// Whether a panic is kept: read before any [`contain`](Self::contain),
    /// whether the quit was cancelled because code the consultation ran
    /// panicked, not because a presentation refused it.
    #[must_use]
    pub fn panicked(&self) -> bool {
        self.panic.is_held()
    }

    /// Run `follow_up`, the host's code after the consultation (following the
    /// verdict, reporting it), keeping a panic it raises behind the first one
    /// the consultation kept.
    pub fn contain(&mut self, follow_up: impl FnOnce()) {
        let _done = self.panic.contain(follow_up);
    }

    /// Resume the first panic the consultation kept, if any.
    pub fn resume(self) {
        self.panic.resume();
    }
}

impl std::fmt::Debug for QuitFinished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuitFinished")
            .field("verdict", &self.verdict)
            .field("panicked", &self.panic.is_held())
            .finish()
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

    /// Finish the consultation `quit` just began with `answer`.
    fn finish(quit: &mut QuitRequest, answer: QuitAnswer) -> QuitVerdict {
        let _consultation = quit.begin_consult().expect("a quit is asked");
        let finished = quit.finish_consult(QuitAnswered {
            answer,
            panic: FirstPanic::new(),
        });
        let verdict = finished.verdict();
        finished.resume();
        verdict
    }

    /// A deferred quit commits when its last waiter consents, a gone waiter
    /// consents, and one that stays open abandons it; an answer that arrives
    /// after the quit was asked again is dropped.
    #[test]
    fn a_quit_moves_on_its_presentations_answers() {
        let mut quit = QuitRequest::new();
        assert!(quit.begin_consult().is_none(), "nothing asked");
        quit.ask();
        assert_eq!(
            finish(&mut quit, QuitAnswer::Waiting(vec![at(1), at(2)])),
            QuitVerdict::Deferred
        );
        assert_eq!(quit.consented(at(1)), QuitVerdict::Unchanged);
        assert_eq!(quit.forget_gone(&[at(1)]), QuitVerdict::Commit);
        assert!(!quit.is_pending());

        quit.ask();
        assert_eq!(
            finish(&mut quit, QuitAnswer::Waiting(vec![at(3)])),
            QuitVerdict::Deferred
        );
        assert_eq!(quit.declined(at(4)), QuitVerdict::Unchanged);
        assert_eq!(quit.declined(at(3)), QuitVerdict::Abandoned);

        quit.ask();
        let _consultation = quit.begin_consult().expect("a quit is asked");
        quit.ask();
        let finished = quit.finish_consult(QuitAnswered {
            answer: QuitAnswer::Waiting(Vec::new()),
            panic: FirstPanic::new(),
        });
        assert_eq!(
            finished.verdict(),
            QuitVerdict::Unchanged,
            "a quit asked again during the consultation is asked again"
        );
        finished.resume();
        assert_eq!(
            finish(&mut quit, QuitAnswer::Cancelled),
            QuitVerdict::Abandoned
        );
        quit.ask();
        assert_eq!(
            finish(&mut quit, QuitAnswer::Panicked),
            QuitVerdict::Abandoned
        );
        assert!(!quit.is_pending());
    }
}
