//! The requests one Activity takes for results, and how each answer finds
//! its request (ADR-0154 §9, ADR-0340 §2).
//!
//! A provider's request is minted a [`ResultToken`] here and handed to
//! `FluiHost.launch` on the Java main thread with this Activity's epoch, a
//! number `FluiHost.nextEpoch` gives each platform of the process. Java picks
//! the request code Android sees from the range it keeps for the host, keeps
//! the epoch, the token and the key's two parts beside it, saves the started
//! requests with the Activity's state, and queues the answer
//! `onActivityResult` brings with all of them. The owner drains the queue
//! ([`Answer::from_drained`]) and [`ResultRouter::answer`] routes it:
//!
//! - an answer of this epoch settles the open request with its token, once,
//!   whatever its key reads as; an answer for a token that is not open
//!   (cancelled, already answered) is stale and dropped;
//! - an answer of another epoch belongs to an earlier Activity, whose runtime
//!   ended with it: whatever it says, it ends the oldest reattachment
//!   ([`ResultRouter::open_reattach`]) still waiting under its key (oldest
//!   by token, the [`ResultTokenMint`] sequence), or waits for
//!   one, the oldest first by its arrival sequence, at most [`MAX_EARLIER`] at
//!   once. Epoch 0 names a request Java restored from the saved state of a
//!   process that died. How many answers are still to come under a key is
//!   Java's to say, from one definition (`FluiHost.stillToCome`: waiting to
//!   start, started, answered and not drained), and reattachments wait only
//!   while it counts one for each ([`ResultRouter::reconcile`]).
//!   `FluiActivity` restores the saved requests before
//!   `NativeActivity.onCreate` starts `android_main`, so no provider asks
//!   before they are known.
//!
//! A request lasts while its provider holds its ticket; a dropped ticket
//! wakes the owner, whose turn ends the request as cancelled and cancels a
//! started activity on the Java side, which frees its place there. The
//! `Intent` an answer carries is held here, under its request's token and
//! capability, until that capability's provider hands its [`ResultIntent`] to
//! `Activity::take_data` or drops it, which [`ResultRouter::sweep`] sees. A
//! launch the Java main thread never
//! takes within [`HAND_OFF_LIMIT`] ends as `HandOffFailed`
//! ([`ResultRouter::overdue`]). The router is owner-local and runs no user
//! code: what it settles comes back as a [`Delivery`], which the caller
//! resolves once its own borrow has ended.

use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::IdMint;
use flui_platform::capability::{
    CapabilityId, CompletionSource, DataRefused, MAX_OPEN_REQUESTS,
    MAX_OPEN_REQUESTS_PER_CAPABILITY, RequestKey, Resolver, ResultCode, ResultIntent,
    ResultIntentHold, ResultKey, ResultMint, ResultOutcome, ResultRefused, ResultRequest,
    ResultTicketHold, ResultToken, ResultTokenMint,
};
use web_time::Instant;

/// `FluiHost.ANSWER_RESULT`: `onActivityResult` answered the request.
pub(crate) const ANSWER_RESULT: i64 = 1;
/// `FluiHost.ANSWER_NOT_LAUNCHED`: Android refused to start its activity.
pub(crate) const ANSWER_NOT_LAUNCHED: i64 = 2;
/// `FluiHost.ANSWER_TOO_MANY`: the host held as many requests across the
/// process as it holds.
pub(crate) const ANSWER_TOO_MANY: i64 = 3;
/// The numbers of one answer in `FluiHost.peekAnswers`: epoch, token, kind,
/// result code.
pub(crate) const ANSWER_NUMBERS: usize = 4;
/// The most answers of earlier Activities that wait for a reattachment.
pub(crate) const MAX_EARLIER: usize = MAX_OPEN_REQUESTS;
/// How long a launch may wait for the Java main thread to take it before it
/// ends as `HandOffFailed`: longer than Android lets the main thread stall
/// before it reports the application not responding.
pub(crate) const HAND_OFF_LIMIT: Duration = Duration::from_secs(10);

/// The `Intent` an answer carried, `D` as the host holds it.
#[derive(Debug)]
pub(crate) enum Carried<D> {
    /// None.
    Nothing,
    /// One, held.
    Held(D),
    /// One the host could not keep.
    Lost,
}

/// What one request was answered with; `D` is the `Intent` as the host holds
/// it.
#[derive(Debug)]
pub(crate) enum Reply<D> {
    /// The started activity's answer.
    Result {
        /// The code it finished with.
        code: ResultCode,
        /// The `Intent` it answered with.
        data: Carried<D>,
    },
    /// Android refused to start it.
    NotLaunched,
    /// The host's Java side held as many requests as it holds.
    TooMany,
    /// An answer this library cannot read: a kind or a code it does not
    /// know.
    Unreadable,
}

/// One answer `FluiHost` queued.
#[derive(Debug)]
pub(crate) struct Answer<D> {
    /// The epoch of the Activity that took the request; 0 for a request
    /// restored from an earlier process.
    pub(crate) epoch: u64,
    /// The request's token, as the Java side carried it.
    pub(crate) token: u64,
    /// The request's capability and key, each as a [`ResultKey`]; `None`
    /// when either could not be read or is not one.
    pub(crate) key: Option<(Box<str>, Box<str>)>,
    /// What it was answered with.
    pub(crate) reply: Reply<D>,
}

impl<D> Answer<D> {
    /// The answers `FluiHost.peekAnswers` returned: [`ANSWER_NUMBERS`]
    /// numbers each, oldest first, with each one's capability and key (each
    /// `None` where it could not be read) and `Intent`. Each answer is read
    /// on its own and none is dropped: one whose key could not be read still
    /// settles this Activity's request by its token, and one of a kind or
    /// with a code this library does not know is logged and settles its
    /// request as unreadable. Where the lengths disagree (a defect of one
    /// side), the answers all of them hold are read.
    pub(crate) fn from_drained(
        numbers: &[i64],
        capabilities: Vec<Option<String>>,
        keys: Vec<Option<String>>,
        data: Vec<Carried<D>>,
    ) -> Vec<Self> {
        let (rows, _) = numbers.as_chunks::<ANSWER_NUMBERS>();
        rows.iter()
            .zip(capabilities)
            .zip(keys)
            .zip(data)
            .map(|(((&[epoch, token, kind, code], capability), key), data)| {
                let reply = match (kind, i32::try_from(code)) {
                    (ANSWER_RESULT, Ok(code)) => Reply::Result {
                        code: ResultCode::from_raw(code),
                        data,
                    },
                    (ANSWER_NOT_LAUNCHED, _) => Reply::NotLaunched,
                    (ANSWER_TOO_MANY, _) => Reply::TooMany,
                    _ => {
                        tracing::warn!(
                            target: "flui_native::android",
                            kind,
                            code,
                            "FluiHost queued an answer this library cannot read; \
                             its request ends as unreadable"
                        );
                        Reply::Unreadable
                    }
                };
                let key = capability
                    .zip(key)
                    .filter(|(capability, key)| {
                        ResultKey::check(capability).is_ok() && ResultKey::check(key).is_ok()
                    })
                    .map(|(capability, key)| (capability.into_boxed_str(), key.into_boxed_str()));
                Self {
                    epoch: epoch.cast_unsigned(),
                    token: token.cast_unsigned(),
                    key,
                    reply,
                }
            })
            .collect()
    }
}

/// What a request is open for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// An activity this Activity started.
    Launched,
    /// The next answer to an earlier Activity's request under its key.
    Reattached,
}

/// One open request. Its token is its age: tokens come from one
/// [`ResultTokenMint`], increasing, so the oldest of several is the one with
/// the least token, wherever it sits.
struct Open {
    token: ResultToken,
    capability: CapabilityId,
    key: RequestKey,
    purpose: Purpose,
    /// When the launch was posted to the Java main thread, until it took it;
    /// `None` once taken, and for a reattachment.
    posted: Option<Instant>,
    resolver: Resolver<ResultOutcome>,
    /// Whether the provider still holds the request's ticket.
    ticket: ResultTicketHold,
}

impl Open {
    fn waits_under(&self, capability: &str, key: &str) -> bool {
        self.purpose == Purpose::Reattached
            && !self.ticket.is_released()
            && self.key.matches(capability, key)
    }
}

/// An `Intent` held for its provider, under its request's token and the
/// capability whose request received it.
struct Held<D> {
    token: ResultToken,
    capability: CapabilityId,
    data: D,
    hold: ResultIntentHold,
}

/// What became of an answer, for the owner's log and the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Routed {
    /// It settled this Activity's request.
    Settled,
    /// It settled a reattachment waiting under its key.
    Reattached,
    /// It waits for a reattachment under its key.
    Waiting,
    /// This Activity's, for a token that is not open: cancelled or already
    /// answered.
    Stale,
    /// An earlier Activity's that nothing can receive: its key could not be
    /// read, [`MAX_EARLIER`] answers already wait, or this Activity has
    /// ended. A reattachment it would have settled ends at
    /// [`ResultRouter::reconcile`].
    LetGo,
}

/// Outcomes to resolve once the caller's borrow has ended.
#[must_use = "a delivery left unresolved ends its requests as ProviderDropped"]
#[derive(Default)]
pub(crate) struct Delivery(Vec<(Resolver<ResultOutcome>, ResultOutcome)>);

impl Delivery {
    fn push(&mut self, resolver: Resolver<ResultOutcome>, outcome: ResultOutcome) {
        self.0.push((resolver, outcome));
    }

    /// Resolve each outcome. A completion nobody holds any more drops what
    /// it would have received, which releases an `Intent` in it.
    pub(crate) fn deliver(self) {
        for (resolver, outcome) in self.0 {
            let _ = resolver.resolve(outcome);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// An earlier Activity's answer waiting for its key, with the sequence it
/// arrived in: the oldest is the one with the least.
struct Earlier<D> {
    arrived: NonZeroU64,
    capability: Box<str>,
    key: Box<str>,
    reply: Result<(ResultCode, Carried<D>), ResultRefused>,
}

/// What the router makes the values it hands providers with, which the
/// composition root handed the bridge ([`ResultMint`]), and what a dropped
/// ticket or `ResultIntent` wakes.
pub(crate) struct Issuer {
    pub(crate) mint: ResultMint,
    pub(crate) wake: Rc<dyn Fn()>,
}

/// The requests of one Activity, the answers of earlier ones that wait for
/// their key, and the answered `Intent`s, of type `D`, held for their
/// providers.
///
/// What it counts is only what the Java side no longer holds: `Intent`s
/// drained and held here, earlier answers waiting here, and launches posted
/// to the Java main thread that it has not taken yet. Requests the Java side
/// holds (waiting to start, started, answered and not drained) are its to
/// count: `FluiHost` holds the process-wide and per-capability bounds, and a
/// reattachment waits only while `FluiHost.stillToCome` counts an answer to
/// come for it.
pub(crate) struct ResultRouter<D> {
    /// This Activity's epoch, which `FluiHost.nextEpoch` minted.
    epoch: NonZeroU64,
    tokens: ResultTokenMint,
    arrivals: IdMint<NonZeroU64>,
    issuer: Option<Issuer>,
    open: Vec<Open>,
    /// Earlier Activities' answers; at most [`MAX_EARLIER`].
    earlier: Vec<Earlier<D>>,
    /// The `Intent` each answered request received, until its provider takes
    /// it or lets its `ResultIntent` go; at most [`MAX_OPEN_REQUESTS`], and per
    /// capability [`MAX_OPEN_REQUESTS_PER_CAPABILITY`]. Each came from an
    /// admitted request, so none is ever let go to make room.
    held: Vec<Held<D>>,
    /// The requests whose tickets were dropped, ended as cancelled, and the
    /// launches among them for the caller to cancel on the Java side
    /// ([`take_released`](Self::take_released)).
    released: Delivery,
    finish: Vec<ResultToken>,
    ended: bool,
}

impl<D> ResultRouter<D> {
    /// The router of the Activity whose epoch is `epoch`.
    pub(crate) fn new(epoch: NonZeroU64) -> Self {
        Self {
            epoch,
            tokens: ResultTokenMint::new(),
            arrivals: IdMint::new(),
            issuer: None,
            open: Vec::new(),
            earlier: Vec::new(),
            held: Vec::new(),
            released: Delivery::default(),
            finish: Vec::new(),
            ended: false,
        }
    }

    /// This Activity's epoch, which every request it hands Java carries.
    #[cfg_attr(
        not(target_os = "android"),
        expect(dead_code, reason = "the Android bridge hands Java the epoch")
    )]
    pub(crate) fn epoch(&self) -> NonZeroU64 {
        self.epoch
    }

    /// What the router makes provider values with, from the composition
    /// root's mint; before it, no request can open, since the Activity is
    /// offered only after.
    pub(crate) fn bind(&mut self, issuer: Issuer) {
        self.issuer = Some(issuer);
    }

    fn issuer(&self) -> &Issuer {
        self.issuer
            .as_ref()
            .expect("BUG: a request opens only through an Activity, offered after bind")
    }

    /// Release every held `Intent` whose provider let its `ResultIntent` go,
    /// and end every open request whose provider let its ticket go, as
    /// cancelled.
    pub(crate) fn sweep(&mut self) {
        self.held.retain(|held| !held.hold.is_released());
        let (gone, kept): (Vec<Open>, Vec<Open>) = std::mem::take(&mut self.open)
            .into_iter()
            .partition(|open| open.ticket.is_released());
        self.open = kept;
        for open in gone {
            if open.purpose == Purpose::Launched {
                self.finish.push(open.token);
            }
            self.released
                .push(open.resolver, Err(ResultRefused::Cancelled));
        }
    }

    /// What [`sweep`](Self::sweep) ended since the last call: the launches
    /// to cancel on the Java side, which frees their places there at once,
    /// and the outcomes to resolve.
    pub(crate) fn take_released(&mut self) -> (Vec<ResultToken>, Delivery) {
        (
            std::mem::take(&mut self.finish),
            std::mem::take(&mut self.released),
        )
    }

    /// Whether `capability` may launch another request now: the `Intent`s
    /// held here and the launches the Java main thread has not taken are
    /// this side's to bound; the requests Java holds are bounded there.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::ActivityEnded`] once [`end`](Self::end) ran;
    /// [`ResultRefused::TooManyOpen`] while [`MAX_OPEN_REQUESTS`] `Intent`s,
    /// or [`MAX_OPEN_REQUESTS_PER_CAPABILITY`] of `capability`'s, wait to be
    /// taken, or [`MAX_OPEN_REQUESTS`] launches wait for the Java main
    /// thread.
    pub(crate) fn admits(&mut self, capability: &CapabilityId) -> Result<(), ResultRefused> {
        if self.ended {
            return Err(ResultRefused::ActivityEnded);
        }
        self.sweep();
        let mine = self
            .held
            .iter()
            .filter(|held| held.capability == *capability)
            .count();
        let posted = self
            .open
            .iter()
            .filter(|open| open.posted.is_some())
            .count();
        if self.held.len() >= MAX_OPEN_REQUESTS
            || mine >= MAX_OPEN_REQUESTS_PER_CAPABILITY
            || posted >= MAX_OPEN_REQUESTS
        {
            return Err(ResultRefused::TooManyOpen);
        }
        Ok(())
    }

    /// A token, a pending completion and the ticket's host side for one more
    /// request.
    fn take_token(
        &mut self,
        completions: &CompletionSource,
    ) -> Result<
        (
            ResultToken,
            ResultRequest,
            Resolver<ResultOutcome>,
            ResultTicketHold,
        ),
        ResultRefused,
    > {
        let token = self
            .tokens
            .next()
            .map_err(|_| ResultRefused::TokensExhausted)?;
        let (completion, resolver) = completions.pending::<ResultOutcome>();
        let issuer = self.issuer();
        let (ticket, hold) = issuer.mint.ticket(token, Rc::clone(&issuer.wake));
        let request = issuer.mint.request(ticket, completion);
        Ok((token, request, resolver, hold))
    }

    /// The outcome of the answer `reply` for the request `token` of
    /// `capability`, holding the `Intent` it carried under it.
    fn settle(
        &mut self,
        token: ResultToken,
        capability: &CapabilityId,
        reply: Result<(ResultCode, Carried<D>), ResultRefused>,
    ) -> ResultOutcome {
        let (code, data) = reply?;
        let data = match data {
            Carried::Nothing => None,
            Carried::Lost => return Err(ResultRefused::DataLost { code }),
            Carried::Held(data) => {
                let issuer = self.issuer();
                let (handed, hold) = issuer.mint.data(token, Rc::clone(&issuer.wake));
                self.held.push(Held {
                    token,
                    capability: capability.clone(),
                    data,
                    hold,
                });
                Some(handed)
            }
        };
        Ok(self.issuer().mint.result(code, data))
    }

    /// Open a request of `capability` for an activity this Activity posts to
    /// the Java main thread at `now` under `key`; the caller hands Java its
    /// token with [`epoch`](Self::epoch).
    ///
    /// # Errors
    ///
    /// As [`admits`](Self::admits), and [`ResultRefused::TokensExhausted`].
    pub(crate) fn open_launch(
        &mut self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
        now: Instant,
    ) -> Result<ResultRequest, ResultRefused> {
        self.admits(capability)?;
        let (token, request, resolver, ticket) = self.take_token(completions)?;
        self.open.push(Open {
            token,
            capability: capability.clone(),
            key,
            purpose: Purpose::Launched,
            posted: Some(now),
            resolver,
            ticket,
        });
        Ok(request)
    }

    /// The Java main thread took the launch `token`: it no longer has a
    /// deadline.
    pub(crate) fn handed(&mut self, token: ResultToken) {
        if let Some(open) = self.open.iter_mut().find(|open| open.token == token) {
            open.posted = None;
        }
    }

    /// When the oldest launch not yet taken by the Java main thread is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.open
            .iter()
            .filter_map(|open| open.posted)
            .min()
            .map(|posted| posted + HAND_OFF_LIMIT)
    }

    /// The launches the Java main thread has not taken within
    /// [`HAND_OFF_LIMIT`] of `now`: each ends as
    /// [`ResultRefused::HandOffFailed`], and their tokens are returned for
    /// the caller to cancel on the Java side, should the work run late.
    pub(crate) fn overdue(&mut self, now: Instant) -> (Vec<ResultToken>, Delivery) {
        let mut delivery = Delivery::default();
        let (due, kept): (Vec<Open>, Vec<Open>) = std::mem::take(&mut self.open)
            .into_iter()
            .partition(|open| {
                open.posted
                    .is_some_and(|posted| now.saturating_duration_since(posted) >= HAND_OFF_LIMIT)
            });
        self.open = kept;
        let tokens = due.iter().map(|open| open.token).collect();
        for open in due {
            delivery.push(open.resolver, Err(ResultRefused::HandOffFailed));
        }
        (tokens, delivery)
    }

    /// The oldest earlier answer waiting under `key`, taken.
    fn take_earlier(&mut self, key: RequestKey) -> Option<Earlier<D>> {
        let index = self
            .earlier
            .iter()
            .enumerate()
            .filter(|(_, earlier)| key.matches(&earlier.capability, &earlier.key))
            .min_by_key(|(_, earlier)| earlier.arrived)
            .map(|(index, _)| index)?;
        Some(self.earlier.swap_remove(index))
    }

    /// How many reattachments still wait under `key`.
    fn waiting_under(&self, key: RequestKey) -> usize {
        self.open
            .iter()
            .filter(|open| open.waits_under(key.capability().as_str(), key.key().as_str()))
            .count()
    }

    /// Open a reattachment of `capability` under `key`: settled with the
    /// oldest earlier answer under `key` when one waits, else by the next one
    /// to arrive, while the Java side counts more answers still to come under
    /// `key` (`still_to_come`) than reattachments already wait for.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::NothingPending`] when nothing waits and nothing more
    /// is to come, which opens nothing; [`ResultRefused::ActivityEnded`] and
    /// [`ResultRefused::TokensExhausted`].
    pub(crate) fn open_reattach(
        &mut self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
        still_to_come: usize,
    ) -> Result<(ResultRequest, Delivery), ResultRefused> {
        if self.ended {
            return Err(ResultRefused::ActivityEnded);
        }
        self.sweep();
        let waiting = self
            .earlier
            .iter()
            .any(|earlier| key.matches(&earlier.capability, &earlier.key));
        if !waiting && self.waiting_under(key) >= still_to_come {
            return Err(ResultRefused::NothingPending);
        }
        let (token, request, resolver, ticket) = self.take_token(completions)?;
        let mut delivery = Delivery::default();
        match self.take_earlier(key) {
            Some(earlier) => {
                let outcome = self.settle(token, capability, earlier.reply);
                delivery.push(resolver, outcome);
            }
            None => self.open.push(Open {
                token,
                capability: capability.clone(),
                key,
                purpose: Purpose::Reattached,
                posted: None,
                resolver,
                ticket,
            }),
        }
        Ok((request, delivery))
    }

    /// Route `answer` to the request it answers. Requests whose tickets
    /// were dropped are swept first, so an earlier Activity's answer reaches
    /// the oldest reattachment whose provider still waits; every answer
    /// under a readable key ends a waiting reattachment, whatever it says.
    pub(crate) fn answer(&mut self, answer: Answer<D>) -> (Routed, Delivery) {
        self.sweep();
        let mut delivery = Delivery::default();
        let reply = match answer.reply {
            Reply::Result { code, data } => Ok((code, data)),
            Reply::NotLaunched => Err(ResultRefused::NotLaunched),
            Reply::TooMany => Err(ResultRefused::TooManyOpen),
            Reply::Unreadable => Err(ResultRefused::AnswerUnreadable),
        };
        if answer.epoch == self.epoch.get() {
            let index = self.open.iter().position(|open| {
                open.purpose == Purpose::Launched && open.token.get() == answer.token
            });
            let Some(index) = index else {
                return (Routed::Stale, delivery);
            };
            let open = self.open.swap_remove(index);
            let outcome = self.settle(open.token, &open.capability, reply);
            delivery.push(open.resolver, outcome);
            return (Routed::Settled, delivery);
        }
        let Some((capability, key)) = answer.key else {
            // An earlier Activity's answer whose key could not be read: its
            // runtime ended with it, and nothing can claim it.
            return (Routed::LetGo, delivery);
        };
        let oldest = self
            .open
            .iter()
            .enumerate()
            .filter(|(_, open)| open.waits_under(&capability, &key))
            .min_by_key(|(_, open)| open.token)
            .map(|(index, _)| index);
        if let Some(index) = oldest {
            let open = self.open.swap_remove(index);
            let outcome = self.settle(open.token, &open.capability, reply);
            delivery.push(open.resolver, outcome);
            return (Routed::Reattached, delivery);
        }
        if self.ended || self.earlier.len() >= MAX_EARLIER {
            return (Routed::LetGo, delivery);
        }
        let Ok(arrived) = self.arrivals.next() else {
            return (Routed::LetGo, delivery);
        };
        self.earlier.push(Earlier {
            arrived,
            capability,
            key,
            reply,
        });
        (Routed::Waiting, delivery)
    }

    /// The keys reattachments wait under, for the caller to ask the Java
    /// side how many answers are still to come under each.
    pub(crate) fn reattached_keys(&self) -> Vec<RequestKey> {
        let mut keys: Vec<RequestKey> = Vec::new();
        for open in &self.open {
            if open.purpose == Purpose::Reattached
                && !open.ticket.is_released()
                && !keys.contains(&open.key)
            {
                keys.push(open.key);
            }
        }
        keys
    }

    /// Hold the reattachments under `key` to what the Java side says is
    /// still to come there (`still_to_come`): the youngest beyond it end as
    /// [`ResultRefused::NothingPending`], since no answer will reach them
    /// (one was drained whose key could not be read, or a request was
    /// forgotten with its Activity).
    pub(crate) fn reconcile(&mut self, key: RequestKey, still_to_come: usize) -> Delivery {
        let mut delivery = Delivery::default();
        let mut waiting: Vec<ResultToken> = self
            .open
            .iter()
            .filter(|open| open.waits_under(key.capability().as_str(), key.key().as_str()))
            .map(|open| open.token)
            .collect();
        waiting.sort_unstable();
        for token in waiting.into_iter().skip(still_to_come) {
            if let Some(index) = self.open.iter().position(|open| open.token == token) {
                let open = self.open.swap_remove(index);
                delivery.push(open.resolver, Err(ResultRefused::NothingPending));
            }
        }
        delivery
    }

    /// The host could not hand the launched request `token` to Android: it
    /// ends as [`ResultRefused::HandOffFailed`]. `None` when it is not open
    /// (answered, cancelled or overdue meanwhile).
    pub(crate) fn hand_off_failed(&mut self, token: ResultToken) -> Option<Delivery> {
        let index = self
            .open
            .iter()
            .position(|open| open.purpose == Purpose::Launched && open.token == token)?;
        let open = self.open.swap_remove(index);
        let mut delivery = Delivery::default();
        delivery.push(open.resolver, Err(ResultRefused::HandOffFailed));
        Some(delivery)
    }

    /// The `Intent` `data` names, once, when `capability`'s request received
    /// it.
    ///
    /// # Errors
    ///
    /// [`DataRefused::NotHeld`] when another Activity's host handed `data`
    /// out, it was taken, or the Activity ended; [`DataRefused::OtherCapability`]
    /// when another capability's request received it.
    pub(crate) fn take_data(
        &mut self,
        capability: &CapabilityId,
        data: ResultIntent,
    ) -> Result<D, DataRefused> {
        let ours = self
            .issuer
            .as_ref()
            .is_some_and(|issuer| issuer.mint.made(&data));
        let token = data.token();
        drop(data);
        if !ours {
            return Err(DataRefused::NotHeld);
        }
        let index = self
            .held
            .iter()
            .position(|held| held.token == token)
            .ok_or(DataRefused::NotHeld)?;
        if self.held[index].capability != *capability {
            return Err(DataRefused::OtherCapability);
        }
        Ok(self.held.swap_remove(index).data)
    }

    /// Whether [`end`](Self::end) ran.
    #[cfg_attr(
        not(target_os = "android"),
        expect(
            dead_code,
            reason = "the Android bridge lends no Activity once it ended"
        )
    )]
    pub(crate) fn has_ended(&self) -> bool {
        self.ended
    }

    /// The Activity ended: every open request ends as
    /// [`ResultRefused::ActivityEnded`], later ones are refused, and the
    /// earlier answers nobody reattached to and the `Intent`s nobody took
    /// are let go; how many earlier answers were.
    pub(crate) fn end(&mut self) -> (Delivery, usize) {
        self.ended = true;
        // What the last sweep ended is delivered with the rest; the Activity
        // whose launches it would cancel has ended.
        let mut delivery = std::mem::take(&mut self.released);
        self.finish.clear();
        for open in self.open.drain(..) {
            delivery.push(open.resolver, Err(ResultRefused::ActivityEnded));
        }
        let let_go = self.earlier.len();
        self.earlier.clear();
        self.held.clear();
        (delivery, let_go)
    }

    #[cfg(test)]
    fn open_len(&self) -> usize {
        self.open.len()
    }

    #[cfg(test)]
    fn held_len(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::num::NonZeroU64;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use flui_platform::capability::{
        Abandoned, ActivityBridge, ActivityResult, Capability, CapabilityId, CapabilityRegistrar,
        Completion, CompletionSource, DataRefused, HostBridge, IntentFrame, JniFrame,
        MAX_OPEN_REQUESTS, MAX_OPEN_REQUESTS_PER_CAPABILITY, OwnerJob, OwnerPoster, OwnerRetired,
        OwnerScope, PendingCompletions, ProviderContext, RequestKey, ResultCode, ResultIntent,
        ResultKey, ResultMint, ResultOutcome, ResultRefused, ResultRequest, ResultTicket,
        ResultToken,
    };
    use web_time::Instant;

    use super::{
        ANSWER_NOT_LAUNCHED, ANSWER_NUMBERS, ANSWER_RESULT, ANSWER_TOO_MANY, Answer, Carried,
        Delivery, HAND_OFF_LIMIT, Issuer, MAX_EARLIER, Reply, ResultRouter, Routed,
    };

    const fn key(name: &'static str) -> ResultKey {
        match ResultKey::try_new(name) {
            Ok(key) => key,
            Err(_) => panic!("BUG: a test key is printable and short"),
        }
    }

    /// One capability's keys, and the same key under another capability.
    const PICK: RequestKey = RequestKey::new(key("files"), key("pick"));
    const SHARE: RequestKey = RequestKey::new(key("files"), key("share"));
    const OTHER_PICK: RequestKey = RequestKey::new(key("camera"), key("pick"));

    /// More answers still to come than any test opens reattachments for.
    const PLENTY: usize = MAX_OPEN_REQUESTS;

    fn epoch(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("BUG: test epochs are not zero")
    }

    /// A bridge that keeps the mint the composition root hands it: the only
    /// way a host gets one.
    #[derive(Default)]
    struct Minted(RefCell<Option<ResultMint>>);

    impl ActivityBridge for Minted {
        fn bind(&self, mint: ResultMint) {
            self.0.replace(Some(mint));
        }

        fn launch_for_result(
            &self,
            _completions: &CompletionSource,
            _capability: &CapabilityId,
            _key: RequestKey,
            _intent: &mut dyn FnMut(IntentFrame) -> bool,
        ) -> Result<ResultRequest, ResultRefused> {
            Err(ResultRefused::ActivityEnded)
        }

        fn reattach(
            &self,
            _completions: &CompletionSource,
            _capability: &CapabilityId,
            _key: RequestKey,
        ) -> Result<ResultRequest, ResultRefused> {
            Err(ResultRefused::ActivityEnded)
        }

        fn take_data(
            &self,
            _capability: &CapabilityId,
            _data: ResultIntent,
            _f: &mut dyn FnMut(JniFrame),
        ) -> Result<(), DataRefused> {
            Err(DataRefused::NotHeld)
        }

        fn lend(&self, _f: &mut dyn FnMut(JniFrame)) -> bool {
            false
        }
    }

    /// A mint, as the composition root hands one to an installed bridge.
    fn mint() -> ResultMint {
        let bridge = Rc::new(Minted::default());
        let host = HostBridge::new();
        let installed: Rc<dyn ActivityBridge> = Rc::clone(&bridge) as Rc<dyn ActivityBridge>;
        drop(host.install_activity(Some(installed)));
        bridge.0.borrow().clone().expect("installing binds")
    }

    /// The router of the Activity `value`, bound to its own mint.
    fn bound_router(value: u64) -> Router {
        let mut router = Router::new(epoch(value));
        router.bind(Issuer {
            mint: mint(),
            wake: Rc::new(|| {}),
        });
        router
    }

    enum Files {}
    impl Capability for Files {
        type Handle = ();
        const NAME: &'static str = "files";
    }

    enum Camera {}
    impl Capability for Camera {
        type Handle = ();
        const NAME: &'static str = "camera";
    }

    enum Other {}
    impl Capability for Other {
        type Handle = ();
        const NAME: &'static str = "a";
    }

    enum OtherSlash {}
    impl Capability for OtherSlash {
        type Handle = ();
        const NAME: &'static str = "a/b";
    }

    thread_local! {
        /// One registration per capability name, as one registry mints.
        static IDS: RefCell<HashMap<&'static str, CapabilityId>> = RefCell::default();
    }

    /// The identity the registrar minted for the capability named `name`.
    fn id(name: &'static str) -> CapabilityId {
        IDS.with(|ids| {
            ids.borrow_mut()
                .entry(name)
                .or_insert_with(|| {
                    let mut registrar = CapabilityRegistrar::new();
                    let provider = |_: &ProviderContext<'_>| Ok(());
                    match name {
                        "files" => registrar.provide::<Files>(provider),
                        "camera" => registrar.provide::<Camera>(provider),
                        "a" => registrar.provide::<Other>(provider),
                        "a/b" => registrar.provide::<OtherSlash>(provider),
                        _ => panic!("BUG: a capability the tests name"),
                    }
                    registrar.into_entries()[0].id().clone()
                })
                .clone()
        })
    }

    fn id_of(key: RequestKey) -> CapabilityId {
        id(key.capability().as_str())
    }

    /// An owner run by hand: posted jobs wait for [`Owner::turn`].
    struct Owner {
        queue: Arc<Mutex<Option<Vec<OwnerJob>>>>,
        pending: PendingCompletions,
        source: CompletionSource,
    }

    impl Owner {
        fn new() -> Self {
            let queue = Arc::new(Mutex::new(Some(Vec::new())));
            let sink = Arc::clone(&queue);
            let poster = OwnerPoster::new(move |job| match sink.lock().expect("queue").as_mut() {
                Some(jobs) => {
                    jobs.push(job);
                    Ok(())
                }
                None => Err(OwnerRetired),
            });
            let pending = PendingCompletions::new();
            let source = CompletionSource::new(poster, pending.clone());
            Self {
                queue,
                pending,
                source,
            }
        }

        fn turn(&self) {
            let jobs = std::mem::take(self.queue.lock().expect("queue").as_mut().expect("live"));
            let scope = OwnerScope::new(&self.pending);
            for job in jobs {
                job.run(&scope);
            }
        }

        fn retire(&self) {
            drop(self.queue.lock().expect("queue").take());
            assert!(self.pending.retire().is_none(), "nothing panics retiring");
        }
    }

    /// A request a test took: its token, its ticket while the test keeps
    /// it, and its completion.
    struct Asked {
        token: ResultToken,
        ticket: Option<ResultTicket>,
        completion: Completion<ResultOutcome>,
    }

    impl Asked {
        fn of(request: ResultRequest) -> Self {
            let (ticket, completion) = request.into_parts();
            Self {
                token: ticket.token(),
                ticket: Some(ticket),
                completion,
            }
        }

        fn token(&self) -> ResultToken {
            self.token
        }

        /// Let the ticket go, as a provider that went, or cancels, does.
        fn let_go(&mut self) {
            self.ticket = None;
        }

        /// The outcome once settled, `None` while it waits and once taken.
        fn outcome(&mut self) -> Option<Result<ResultOutcome, Abandoned>> {
            self.completion.poll()
        }

        /// The answer it settled with; panics when it settled otherwise.
        fn answered(&mut self) -> ActivityResult {
            match self.outcome() {
                Some(Ok(Ok(result))) => result,
                other => panic!("an answer: {other:?}"),
            }
        }
    }

    /// The `Intent` a test's answer carries: a name the router holds as it
    /// would hold a JNI global reference.
    type Held = &'static str;

    /// The router a test drives.
    type Router = ResultRouter<Held>;

    fn launch(router: &mut Router, owner: &Owner, key: RequestKey) -> Asked {
        Asked::of(
            router
                .open_launch(&owner.source, &id_of(key), key, Instant::now())
                .expect("admitted"),
        )
    }

    /// A reattachment under `key` while earlier requests are still to come.
    fn reattach(router: &mut Router, owner: &Owner, key: RequestKey) -> (Asked, Delivery) {
        let (request, delivery) = router
            .open_reattach(&owner.source, &id_of(key), key, PLENTY)
            .expect("admitted");
        (Asked::of(request), delivery)
    }

    fn parts(key: RequestKey) -> (Box<str>, Box<str>) {
        (key.capability().as_str().into(), key.key().as_str().into())
    }

    /// An answer of `code` for the request `token` of `epoch` under `key`,
    /// carrying the `Intent` `"intent"`.
    fn result(epoch: u64, token: u64, key: RequestKey, code: ResultCode) -> Answer<Held> {
        answer(
            epoch,
            token,
            Some(parts(key)),
            Reply::Result {
                code,
                data: Carried::Held("intent"),
            },
        )
    }

    fn answer(
        epoch: u64,
        token: u64,
        key: Option<(Box<str>, Box<str>)>,
        reply: Reply<Held>,
    ) -> Answer<Held> {
        Answer {
            epoch,
            token,
            key,
            reply,
        }
    }

    fn refusal_of(settled: Option<Result<ResultOutcome, Abandoned>>) -> Option<ResultRefused> {
        match settled {
            Some(Ok(Err(refused))) => Some(refused),
            _ => None,
        }
    }

    /// Each answer settles the request whose token it carries, once, at the
    /// next owner turn and not inside the drain, even when its key could not
    /// be read; a second answer for that token and one for a token never
    /// minted are stale. Two requests in flight settle in the order their
    /// answers arrive.
    #[test]
    fn an_answer_settles_the_request_its_token_names_once() {
        let owner = Owner::new();
        let mut router = bound_router(7);
        let mut first = launch(&mut router, &owner, PICK);
        let mut second = launch(&mut router, &owner, PICK);
        assert_ne!(first.token(), second.token(), "one token per call");

        let (routed, delivery) =
            router.answer(result(7, second.token().get(), PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        assert!(second.outcome().is_none(), "settles at an owner turn");
        owner.turn();
        assert_eq!(second.answered().code(), ResultCode::Ok);
        assert!(first.outcome().is_none(), "the other request still waits");

        let (routed, delivery) =
            router.answer(result(7, second.token().get(), PICK, ResultCode::Canceled));
        assert_eq!(routed, Routed::Stale, "an answered token is not open");
        assert_eq!(delivery.len(), 0);
        let (routed, _) = router.answer(result(7, u64::MAX, PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Stale, "a token never minted");

        let (routed, delivery) = router.answer(answer(
            7,
            first.token().get(),
            None,
            Reply::Result {
                code: ResultCode::Other(5),
                data: Carried::Nothing,
            },
        ));
        assert_eq!(routed, Routed::Settled, "its own key unread, by token");
        delivery.deliver();
        owner.turn();
        assert_eq!(first.answered().code(), ResultCode::Other(5));
        assert_eq!(router.open_len(), 0);
    }

    /// Every way a launched request ends without an answer ends it with its
    /// own reason: Android refused (`NotLaunched`), the host's Java side was
    /// full (`TooManyOpen`), the host could not hand it over
    /// (`HandOffFailed`, for a failed call, a dropped closure or a panic
    /// alike, and for a launch the Java main thread never took within the
    /// limit, whose token comes back to cancel), or the provider dropped its
    /// ticket (`Cancelled`, and Java cancels the activity, whose later answer
    /// is stale). A token handled twice does nothing the second time.
    #[test]
    fn each_launch_that_is_not_answered_ends_with_its_reason() {
        let owner = Owner::new();
        let mut router = bound_router(1);
        let start = Instant::now();
        let mut refused = launch(&mut router, &owner, PICK);
        let mut full = launch(&mut router, &owner, PICK);
        let mut lost = launch(&mut router, &owner, PICK);
        let mut cancelled = launch(&mut router, &owner, SHARE);
        let mut stuck = Asked::of(
            router
                .open_launch(&owner.source, &id_of(SHARE), SHARE, start)
                .expect("admitted"),
        );
        for taken in [&refused, &full, &lost, &cancelled] {
            router.handed(taken.token());
        }

        router
            .answer(answer(
                1,
                refused.token().get(),
                Some(parts(PICK)),
                Reply::NotLaunched,
            ))
            .1
            .deliver();
        router
            .answer(answer(
                1,
                full.token().get(),
                Some(parts(PICK)),
                Reply::TooMany,
            ))
            .1
            .deliver();
        router
            .hand_off_failed(lost.token())
            .expect("open")
            .deliver();
        assert!(router.hand_off_failed(lost.token()).is_none(), "ended once");
        cancelled.let_go();
        router.sweep();
        let (finish, delivery) = router.take_released();
        assert_eq!(finish, [cancelled.token()], "Java cancels the activity");
        delivery.deliver();
        router.sweep();
        assert_eq!(router.take_released().0, [], "ended once");
        let (routed, _) = router.answer(result(
            1,
            cancelled.token().get(),
            SHARE,
            ResultCode::Canceled,
        ));
        assert_eq!(routed, Routed::Stale);

        assert_eq!(router.next_deadline(), Some(start + HAND_OFF_LIMIT));
        let (tokens, delivery) = router.overdue(start + HAND_OFF_LIMIT / 2);
        assert!(tokens.is_empty() && delivery.len() == 0, "not yet due");
        let (tokens, delivery) = router.overdue(start + HAND_OFF_LIMIT);
        assert_eq!(tokens, [stuck.token()], "only the launch never taken");
        delivery.deliver();
        assert_eq!(router.next_deadline(), None);

        owner.turn();
        assert_eq!(
            refusal_of(refused.outcome()),
            Some(ResultRefused::NotLaunched)
        );
        assert_eq!(refusal_of(full.outcome()), Some(ResultRefused::TooManyOpen));
        assert_eq!(
            refusal_of(lost.outcome()),
            Some(ResultRefused::HandOffFailed)
        );
        assert_eq!(
            refusal_of(cancelled.outcome()),
            Some(ResultRefused::Cancelled)
        );
        assert_eq!(
            refusal_of(stuck.outcome()),
            Some(ResultRefused::HandOffFailed)
        );
        assert_eq!(router.open_len(), 0);
    }

    /// What this side bounds is only what the Java side no longer holds: at
    /// most `MAX_OPEN_REQUESTS_PER_CAPABILITY` untaken `Intent`s of one
    /// capability, so it is refused while another still launches, and an
    /// `Intent` is never let go to make room (taking it, or dropping its
    /// `ResultIntent`, does); and at most `MAX_OPEN_REQUESTS` launches the Java
    /// main thread has not taken. Launches Java took are Java's to count.
    #[test]
    fn the_untaken_intents_and_unhanded_launches_are_bounded() {
        let owner = Owner::new();
        let mut router = bound_router(1);
        let mut answered: Vec<ActivityResult> = Vec::new();
        for _ in 0..MAX_OPEN_REQUESTS_PER_CAPABILITY {
            let mut asked = launch(&mut router, &owner, PICK);
            router.handed(asked.token());
            router
                .answer(result(1, asked.token().get(), PICK, ResultCode::Ok))
                .1
                .deliver();
            owner.turn();
            answered.push(asked.answered());
        }
        assert_eq!(router.admits(&id("files")), Err(ResultRefused::TooManyOpen));
        assert_eq!(router.admits(&id("camera")), Ok(()), "another still asks");
        let data = answered[0].take_data().expect("it carried an Intent");
        assert_eq!(router.take_data(&id("files"), data), Ok("intent"));
        assert_eq!(router.admits(&id("files")), Ok(()), "taking it made room");
        drop(answered);
        assert_eq!(router.held_len(), MAX_OPEN_REQUESTS_PER_CAPABILITY - 1);
        router.sweep();
        assert_eq!(router.held_len(), 0, "dropped unread, released");

        let taken: Vec<Asked> = (0..MAX_OPEN_REQUESTS)
            .map(|_| {
                let asked = launch(&mut router, &owner, OTHER_PICK);
                router.handed(asked.token());
                asked
            })
            .collect();
        assert_eq!(
            router.admits(&id("camera")),
            Ok(()),
            "launches Java took are not counted here"
        );
        let posted: Vec<Asked> = (0..MAX_OPEN_REQUESTS)
            .map(|_| launch(&mut router, &owner, OTHER_PICK))
            .collect();
        assert_eq!(
            router.admits(&id("camera")),
            Err(ResultRefused::TooManyOpen),
            "launches the Java main thread has not taken are"
        );
        drop((taken, posted));
    }

    /// A reattachment with no earlier answer waiting is admitted only while
    /// the Java side counts an answer still to come for it: one more is
    /// refused as `NothingPending` and holds no place.
    #[test]
    fn a_reattachment_with_nothing_pending_holds_nothing() {
        let owner = Owner::new();
        let mut router = bound_router(3);
        assert!(matches!(
            router.open_reattach(&owner.source, &id_of(PICK), PICK, 0),
            Err(ResultRefused::NothingPending)
        ));
        assert_eq!(router.open_len(), 0);

        router
            .answer(result(1, 1, PICK, ResultCode::Ok))
            .1
            .deliver();
        let (_, delivery) = router
            .open_reattach(&owner.source, &id_of(PICK), PICK, 0)
            .expect("an earlier answer waits");
        assert_eq!(delivery.len(), 1);
        delivery.deliver();
        let (first, delivery) = router
            .open_reattach(&owner.source, &id_of(PICK), PICK, 1)
            .expect("one still to come");
        assert_eq!(delivery.len(), 0);
        assert!(matches!(
            router.open_reattach(&owner.source, &id_of(PICK), PICK, 1),
            Err(ResultRefused::NothingPending)
        ));
        assert_eq!(router.open_len(), 1, "one waits for the one to come");
        drop(first);
    }

    /// The oldest reattachment still waiting receives the oldest answer,
    /// whatever order other requests left the router in: a launched request
    /// answered first, then an earlier Activity's answer, still reaches the
    /// first reattachment, not the second.
    #[test]
    fn the_oldest_reattachment_receives_the_answer_whatever_ended_before() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let mut launched = launch(&mut router, &owner, SHARE);
        let (mut older, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        let (mut newer, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();

        router
            .answer(result(2, launched.token().get(), SHARE, ResultCode::Ok))
            .1
            .deliver();
        let (routed, delivery) = router.answer(result(1, 5, PICK, ResultCode::Other(7)));
        assert_eq!(routed, Routed::Reattached);
        delivery.deliver();
        owner.turn();
        assert_eq!(launched.answered().code(), ResultCode::Ok);
        assert_eq!(older.answered().code(), ResultCode::Other(7), "the older");
        assert!(newer.outcome().is_none(), "the newer still waits");
    }

    /// Every earlier Activity's record under a key a reattachment waits for
    /// ends it with its reason (the Java side's refusals and an unreadable
    /// answer included), and a reattachment no answer will reach (its
    /// record's key could not be read) ends as `NothingPending` once the
    /// Java side counts nothing more to come: none waits forever.
    #[test]
    fn every_earlier_record_ends_a_waiting_reattachment() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let mut waiting: Vec<Asked> = (0..4)
            .map(|_| {
                let (asked, delivery) = reattach(&mut router, &owner, PICK);
                delivery.deliver();
                asked
            })
            .collect();
        for reply in [Reply::TooMany, Reply::NotLaunched, Reply::Unreadable] {
            let (routed, delivery) = router.answer(answer(1, 1, Some(parts(PICK)), reply));
            assert_eq!(routed, Routed::Reattached);
            delivery.deliver();
        }
        let (routed, _) = router.answer(result(1, 2, PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Reattached);
        let (unread, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        waiting.push(unread);
        let (routed, _) = router.answer(answer(
            1,
            3,
            None,
            Reply::Result {
                code: ResultCode::Ok,
                data: Carried::Nothing,
            },
        ));
        assert_eq!(routed, Routed::LetGo, "its key cannot be read");
        assert_eq!(router.reattached_keys(), [PICK]);
        router.reconcile(PICK, 0).deliver();
        owner.turn();
        let ends: Vec<Option<ResultRefused>> = waiting
            .iter_mut()
            .map(|asked| match asked.outcome() {
                Some(Ok(Ok(_))) => None,
                other => refusal_of(other),
            })
            .collect();
        assert_eq!(
            ends,
            [
                Some(ResultRefused::TooManyOpen),
                Some(ResultRefused::NotLaunched),
                Some(ResultRefused::AnswerUnreadable),
                None,
                Some(ResultRefused::NothingPending),
            ]
        );
        assert_eq!(router.open_len(), 0);
    }

    /// The `Intent` an answer carries stays with the router, under the token
    /// of the request it settled (a reattachment's own, for an earlier
    /// Activity's answer) and the capability whose request received it, and
    /// is taken once through its `ResultIntent`: another capability is refused
    /// with a typed reason, data another Activity's host handed out is not
    /// held here, and the end lets every one go.
    #[test]
    fn an_answered_intent_is_taken_once_by_its_capability() {
        let owner = Owner::new();
        let mut router = bound_router(6);
        let mut asked = launch(&mut router, &owner, PICK);
        router
            .answer(result(6, asked.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        let mut answered = asked.answered();
        assert!(answered.has_data(), "the answer says it carried one");
        let data = answered.take_data().expect("an Intent");
        assert_eq!(data.token(), asked.token());
        assert_eq!(router.take_data(&id("files"), data), Ok("intent"));
        assert!(!answered.has_data(), "taken once");

        let mut other = launch(&mut router, &owner, PICK);
        router
            .answer(result(6, other.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        let data = other.answered().take_data().expect("an Intent");
        assert_eq!(
            router.take_data(&id("camera"), data),
            Err(DataRefused::OtherCapability),
            "a provider cannot read another capability's Intent"
        );

        let mut elsewhere = bound_router(6);
        let mut theirs = launch(&mut elsewhere, &owner, PICK);
        elsewhere
            .answer(result(6, theirs.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        let data = theirs.answered().take_data().expect("an Intent");
        assert_eq!(
            router.take_data(&id("files"), data),
            Err(DataRefused::NotHeld),
            "another Activity's data names none of this one's tokens"
        );

        let _ = router.answer(result(1, 1, SHARE, ResultCode::Ok));
        let (mut reattached, delivery) = reattach(&mut router, &owner, SHARE);
        delivery.deliver();
        owner.turn();
        let data = reattached.answered().take_data().expect("an Intent");
        assert_eq!(data.token(), reattached.token(), "the reattachment's token");
        assert_eq!(router.take_data(&id("files"), data), Ok("intent"));

        let mut untaken = launch(&mut router, &owner, PICK);
        router
            .answer(result(6, untaken.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        let data = untaken.answered().take_data().expect("an Intent");
        let _ = router.end();
        assert_eq!(
            router.take_data(&id("files"), data),
            Err(DataRefused::NotHeld),
            "the end lets go"
        );
    }

    /// A request whose ticket was let go ends as cancelled at the next
    /// sweep, which a dropped ticket wakes the owner for, and an activity it
    /// started is handed back to be cancelled on the Java side, which frees
    /// its place there.
    #[test]
    fn a_ticket_let_go_wakes_the_owner_and_cancels_its_launch() {
        let owner = Owner::new();
        let woken = Rc::new(std::cell::Cell::new(0_u32));
        let mut router = Router::new(epoch(2));
        let counted = Rc::clone(&woken);
        router.bind(Issuer {
            mint: mint(),
            wake: Rc::new(move || counted.set(counted.get() + 1)),
        });
        let mut launched = launch(&mut router, &owner, PICK);
        let (mut waiter, delivery) = reattach(&mut router, &owner, SHARE);
        delivery.deliver();

        launched.let_go();
        waiter.let_go();
        assert_eq!(woken.get(), 2, "each drop wakes the owner");
        router.sweep();
        let (finish, released) = router.take_released();
        assert_eq!(finish, [launched.token()], "its activity is cancelled");
        assert_eq!(released.len(), 2, "both end as cancelled");
        released.deliver();
        owner.turn();
        assert_eq!(
            refusal_of(launched.outcome()),
            Some(ResultRefused::Cancelled)
        );
        assert_eq!(refusal_of(waiter.outcome()), Some(ResultRefused::Cancelled));
        assert_eq!(router.open_len(), 0);
    }

    /// An `Intent` nobody can receive is released: its request's completion
    /// was dropped before the answer, so the answer is dropped with it, and
    /// the place it took is free again; one whose runtime retired, and one a
    /// provider received and dropped unread, too.
    #[test]
    fn an_intent_nobody_receives_is_released() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let dropped = launch(&mut router, &owner, PICK);
        let token = dropped.token().get();
        drop(dropped);
        router
            .answer(result(2, token, PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        router.sweep();
        assert_eq!(router.held_len(), 0, "a dropped completion holds nothing");

        let mut received = launch(&mut router, &owner, PICK);
        router
            .answer(result(2, received.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        let answered = received.answered();
        router.sweep();
        assert_eq!(router.held_len(), 1, "held while the provider holds it");
        drop(answered);
        router.sweep();
        assert_eq!(router.held_len(), 0, "released once it is dropped unread");

        let retiring = Owner::new();
        let pending = launch(&mut router, &retiring, PICK);
        router
            .answer(result(2, pending.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        retiring.retire();
        router.sweep();
        assert_eq!(router.held_len(), 1, "the answer waits in its completion");
        // A retired runtime's completions go with its states and handles.
        drop(pending);
        router.sweep();
        assert_eq!(router.held_len(), 0, "a retired runtime holds nothing");
    }

    /// An answer of another epoch, an earlier Activity's (epoch 0 is one
    /// restored from a process that died), never settles this Activity's
    /// request, even one whose token has the same value: it waits for a
    /// reattachment under its key, which settles it whether it comes first or
    /// second, the older reattachment taking the older answer. Keys compare
    /// part by part: the same key of another capability, and two that would
    /// read alike joined, never receive it.
    #[test]
    fn an_earlier_activitys_answer_waits_for_its_key() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let mut ours = launch(&mut router, &owner, PICK);
        let (routed, _) = router.answer(result(1, ours.token().get(), PICK, ResultCode::Ok));
        assert_eq!(
            routed,
            Routed::Waiting,
            "an earlier epoch's token is not ours"
        );
        let (routed, _) = router.answer(result(0, 1, PICK, ResultCode::Other(2)));
        assert_eq!(routed, Routed::Waiting, "restored from a dead process");

        let (mut share, delivery) = reattach(&mut router, &owner, SHARE);
        assert_eq!(delivery.len(), 0, "nothing waits under another key");
        let (mut other, delivery) = reattach(&mut router, &owner, OTHER_PICK);
        assert_eq!(
            delivery.len(),
            0,
            "nor under the same key of another capability"
        );
        let (mut first, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        let (mut second, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        owner.turn();
        assert_eq!(first.answered().code(), ResultCode::Ok, "oldest first");
        assert_eq!(second.answered().code(), ResultCode::Other(2));
        assert!(ours.outcome().is_none(), "ours still waits for its own");
        assert!(other.outcome().is_none());

        let (mut later, delivery) = reattach(&mut router, &owner, SHARE);
        assert_eq!(delivery.len(), 0);
        let (routed, delivery) = router.answer(result(1, 9, SHARE, ResultCode::Canceled));
        assert_eq!(routed, Routed::Reattached);
        delivery.deliver();
        owner.turn();
        assert_eq!(
            share.answered().code(),
            ResultCode::Canceled,
            "the reattachment waiting longest"
        );
        assert!(later.outcome().is_none());

        let (routed, _) = router.answer(answer(1, 3, Some(parts(PICK)), Reply::NotLaunched));
        assert_eq!(routed, Routed::Waiting, "an earlier refusal waits too");
        let (mut refused, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        owner.turn();
        assert_eq!(
            refusal_of(refused.outcome()),
            Some(ResultRefused::NotLaunched),
            "and ends its reattachment with its reason"
        );

        for (waiting, arriving) in [
            (("a", "b/c"), ("a/b", "c")),
            (("a/b", "c"), ("a", "b/c")),
            (("files", "pick"), ("camera", "pick")),
        ] {
            let mut router = bound_router(3);
            let (mut asked, delivery) = reattach(
                &mut router,
                &owner,
                RequestKey::new(key(waiting.0), key(waiting.1)),
            );
            assert_eq!(delivery.len(), 0);
            let (routed, _) = router.answer(answer(
                1,
                1,
                Some((arriving.0.into(), arriving.1.into())),
                Reply::Result {
                    code: ResultCode::Ok,
                    data: Carried::Nothing,
                },
            ));
            assert_eq!(
                routed,
                Routed::Waiting,
                "{waiting:?} never takes {arriving:?}"
            );
            owner.turn();
            assert!(asked.outcome().is_none());
        }
    }

    /// At most `MAX_EARLIER` earlier answers wait; one more is let go, and
    /// the waiting ones are kept. One whose key could not be read is let go:
    /// nothing could claim it.
    #[test]
    fn the_earlier_answers_that_wait_are_bounded() {
        let owner = Owner::new();
        let mut router = bound_router(3);
        let (routed, _) = router.answer(answer(
            1,
            1,
            None,
            Reply::Result {
                code: ResultCode::Ok,
                data: Carried::Held("intent"),
            },
        ));
        assert_eq!(routed, Routed::LetGo, "an unread key has no claimant");
        for token in 0..MAX_EARLIER {
            let token = u64::try_from(token).expect("BUG: a small count");
            assert_eq!(
                router.answer(result(1, token, PICK, ResultCode::Ok)).0,
                Routed::Waiting
            );
        }
        assert_eq!(
            router.answer(result(1, 99, PICK, ResultCode::Ok)).0,
            Routed::LetGo
        );
        let (_, delivery) = reattach(&mut router, &owner, PICK);
        assert_eq!(delivery.len(), 1, "a waiting answer settles it at once");
        delivery.deliver();
    }

    /// When the Activity ends, every open request ends as `ActivityEnded`,
    /// a later one is refused without running anything, a later answer of an
    /// earlier Activity is let go, and the earlier answers nobody claimed are
    /// counted and dropped.
    #[test]
    fn the_end_refuses_every_open_request() {
        let owner = Owner::new();
        let mut router = bound_router(4);
        let mut launched = launch(&mut router, &owner, PICK);
        let (mut reattached, delivery) = reattach(&mut router, &owner, SHARE);
        delivery.deliver();
        let _ = router.answer(result(1, 1, PICK, ResultCode::Ok));

        let (delivery, let_go) = router.end();
        assert_eq!(let_go, 1);
        delivery.deliver();
        owner.turn();
        for request in [&mut launched, &mut reattached] {
            assert_eq!(
                refusal_of(request.outcome()),
                Some(ResultRefused::ActivityEnded)
            );
        }
        assert!(matches!(
            router.open_launch(&owner.source, &id_of(PICK), PICK, Instant::now()),
            Err(ResultRefused::ActivityEnded)
        ));
        assert_eq!(
            router.answer(result(1, 2, PICK, ResultCode::Ok)).0,
            Routed::LetGo
        );
    }

    /// A reattachment whose provider let its ticket go waits no more: the
    /// earlier Activity's answer reaches the reattachment under the same key
    /// that still waits, and the one let go ends as cancelled and frees its
    /// place.
    #[test]
    fn a_reattachment_let_go_leaves_the_answer_to_the_one_still_waiting() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let (mut gone, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        let (mut live, delivery) = reattach(&mut router, &owner, PICK);
        delivery.deliver();
        gone.let_go();

        let (routed, delivery) = router.answer(result(1, 4, PICK, ResultCode::Ok));
        delivery.deliver();
        assert_eq!(routed, Routed::Reattached);
        let (finish, released) = router.take_released();
        assert!(
            finish.is_empty(),
            "a reattachment started nothing to finish"
        );
        released.deliver();
        owner.turn();
        assert_eq!(
            live.answered().code(),
            ResultCode::Ok,
            "the live one hears it"
        );
        assert_eq!(
            refusal_of(gone.outcome()),
            Some(ResultRefused::Cancelled),
            "the one let go ends as a cancel does"
        );
        assert_eq!(router.open_len(), 0);
    }

    /// Every answer taken off the queue ends its request: one the host could
    /// not read ends it as unreadable, and one whose `Intent` the host could
    /// not keep as data lost with its code, not as an answer without one.
    #[test]
    fn every_answer_taken_off_the_queue_ends_its_request() {
        let owner = Owner::new();
        let mut router = bound_router(2);
        let mut unreadable = launch(&mut router, &owner, PICK);
        let mut lost = launch(&mut router, &owner, SHARE);
        router
            .answer(answer(2, unreadable.token().get(), None, Reply::Unreadable))
            .1
            .deliver();
        router
            .answer(answer(
                2,
                lost.token().get(),
                Some(parts(SHARE)),
                Reply::Result {
                    code: ResultCode::Ok,
                    data: Carried::Lost,
                },
            ))
            .1
            .deliver();
        owner.turn();
        assert_eq!(
            refusal_of(unreadable.outcome()),
            Some(ResultRefused::AnswerUnreadable)
        );
        assert_eq!(
            refusal_of(lost.outcome()),
            Some(ResultRefused::DataLost {
                code: ResultCode::Ok
            })
        );
        assert_eq!(router.open_len(), 0);
        assert_eq!(router.held_len(), 0);
    }

    /// The answers are read as `FluiHost` lays them out, with the kinds and
    /// the bounds the Java side states, one by one: an answer of a kind or
    /// with a code this library does not know is read as unreadable, and one
    /// whose key could not be read keeps its other fields, at the head, in
    /// the middle and at the end of a drain, and the others are read; lengths
    /// that disagree read the answers all of them hold.
    #[test]
    fn answers_are_read_as_the_java_classes_lay_them_out() {
        let host = super::super::java_host::java_source(super::super::java_host::HOST_CLASS);
        for stated in [
            format!("static final int ANSWER_RESULT = {ANSWER_RESULT};"),
            format!("static final int ANSWER_NOT_LAUNCHED = {ANSWER_NOT_LAUNCHED};"),
            format!("static final int ANSWER_TOO_MANY = {ANSWER_TOO_MANY};"),
            format!("static final int MAX_OPEN = {MAX_OPEN_REQUESTS};"),
            format!(
                "static final int MAX_OPEN_PER_CAPABILITY = {MAX_OPEN_REQUESTS_PER_CAPABILITY};"
            ),
            format!("static final int ANSWER_NUMBERS = {ANSWER_NUMBERS};"),
        ] {
            assert!(host.contains(&stated), "{stated:?} in FluiHost");
        }

        let some = |text: &str| Some(text.to_owned());
        let too_long = "k".repeat(flui_platform::capability::MAX_RESULT_KEY_LEN + 1);
        for at in [0, 1, 3] {
            let mut numbers = vec![
                7,
                1,
                ANSWER_RESULT,
                -1,
                7,
                2,
                ANSWER_NOT_LAUNCHED,
                0,
                0,
                5,
                ANSWER_RESULT,
                3,
            ];
            let mut capabilities = vec![some("files"), some("files"), some("files")];
            let mut keys = vec![some("pick"), some("pick"), some("share")];
            let mut data = vec![Carried::Held("intent"), Carried::Nothing, Carried::Lost];
            // A record of an unknown kind, one with a code past an `int`, and
            // one whose key cannot be read, at the head, in the middle or at
            // the end.
            numbers.splice(
                at * 4..at * 4,
                [
                    7,
                    8,
                    99,
                    0,
                    7,
                    10,
                    ANSWER_RESULT,
                    i64::MAX,
                    7,
                    9,
                    ANSWER_RESULT,
                    0,
                ],
            );
            capabilities.splice(at..at, [some("files"), some("files"), some("files")]);
            keys.splice(at..at, [some("pick"), some("pick"), Some(too_long.clone())]);
            data.splice(
                at..at,
                [Carried::Nothing, Carried::Nothing, Carried::Nothing],
            );
            let answers = Answer::from_drained(&numbers, capabilities, keys, data);
            let read: Vec<_> = answers
                .iter()
                .map(|answer| {
                    (
                        answer.epoch,
                        answer.token,
                        answer.key.is_some(),
                        matches!(answer.reply, Reply::Unreadable),
                    )
                })
                .collect();
            let mut expected = vec![
                (7, 1, true, false),
                (7, 2, true, false),
                (0, 5, true, false),
            ];
            expected.splice(
                at..at,
                [
                    (7, 8, true, true),
                    (7, 10, true, true),
                    (7, 9, false, false),
                ],
            );
            assert_eq!(read, expected, "poisoned at {at}");
            let last = answers.last().expect("answers");
            if at != 3 {
                assert!(
                    matches!(
                        last.reply,
                        Reply::Result {
                            data: Carried::Lost,
                            ..
                        }
                    ),
                    "an Intent not kept stays apart from none"
                );
            }
        }

        let answers = Answer::<Held>::from_drained(
            &[7, 1, ANSWER_TOO_MANY, 0, 7, 2, ANSWER_RESULT],
            vec![some("files"), some("files")],
            vec![some("pick"), some("pick")],
            vec![Carried::Nothing, Carried::Nothing],
        );
        assert_eq!(answers.len(), 1, "half an answer is not read");
        assert!(matches!(answers[0].reply, Reply::TooMany));
    }
}
