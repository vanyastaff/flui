//! The requests one Activity takes for results, and how each answer finds
//! its request (ADR-0154 §9, ADR-0255 decision 8).
//!
//! A provider's request is minted a [`ResultToken`] here and handed to
//! `FluiHost.launch` on the Java main thread with this Activity's epoch, a
//! number `FluiHost.nextEpoch` gives each platform of the process. Java picks
//! the request code Android sees from the range it keeps for the host, keeps
//! the epoch, the token and the key beside it, saves the open requests with
//! the Activity's state, and queues the answer `onActivityResult` brings with
//! all three. The owner drains the queue ([`Answer::from_drained`]) and
//! [`ResultRouter::answer`] routes it:
//!
//! - an answer of this epoch settles the open request with its token, once;
//!   an answer for a token that is not open (cancelled, already answered) is
//!   stale and dropped;
//! - an answer of another epoch belongs to an earlier Activity, whose runtime
//!   ended with it: it settles a [`reattach`](ResultRouter::open_reattach)
//!   waiting under its key, or waits for one, at most [`MAX_EARLIER`] at
//!   once. Epoch 0 names a request Java restored from the saved state of a
//!   process that died.
//!
//! A request the host could not hand to Android (its call failed or
//! panicked, or its posted work was dropped) ends through
//! [`ResultRouter::hand_off_failed`]. The router is owner-local and runs no
//! user code: what it settles comes back as a [`Delivery`], which the caller
//! resolves once its own borrow has ended.

use std::collections::VecDeque;
use std::num::NonZeroU64;

use flui_platform::capability::{
    ActivityResult, CompletionSource, MAX_OPEN_REQUESTS, MAX_REQUEST_KEY_LEN, RequestKey, Resolver,
    ResultCode, ResultOutcome, ResultRefused, ResultRequest, ResultToken, ResultTokenMint,
};

/// `FluiHost.ANSWER_RESULT`: `onActivityResult` answered the request.
pub(crate) const ANSWER_RESULT: i64 = 1;
/// `FluiHost.ANSWER_NOT_LAUNCHED`: Android refused to start its activity.
pub(crate) const ANSWER_NOT_LAUNCHED: i64 = 2;
/// `FluiHost.ANSWER_TOO_MANY`: the host held as many requests across the
/// process as it holds.
pub(crate) const ANSWER_TOO_MANY: i64 = 3;
/// The numbers of one answer in `FluiHost.drainAnswers`: epoch, token, kind,
/// result code.
pub(crate) const ANSWER_NUMBERS: usize = 4;
/// The most answers of earlier Activities that wait for a reattachment.
pub(crate) const MAX_EARLIER: usize = MAX_OPEN_REQUESTS;

/// What one request was answered with; `D` is the `Intent` as the host holds
/// it.
#[derive(Debug)]
pub(crate) enum Reply<D> {
    /// The started activity's answer.
    Result {
        /// The code it finished with.
        code: ResultCode,
        /// The `Intent` it answered with, if any.
        data: Option<D>,
    },
    /// Android refused to start it.
    NotLaunched,
    /// The host's Java side held as many requests as it holds.
    TooMany,
}

/// One answer `FluiHost` queued.
#[derive(Debug)]
pub(crate) struct Answer<D> {
    /// The epoch of the Activity that took the request; 0 for a request
    /// restored from an earlier process.
    pub(crate) epoch: u64,
    /// The request's token, as the Java side carried it.
    pub(crate) token: u64,
    /// The request's key, as [`RequestKey`] spells it.
    pub(crate) key: Box<str>,
    /// What it was answered with.
    pub(crate) reply: Reply<D>,
}

/// Whether `key` could spell a [`RequestKey`]: what a drained key is held to
/// before it is kept.
pub(crate) fn key_fits(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_REQUEST_KEY_LEN && key.bytes().all(|b| b.is_ascii_graphic())
}

impl<D> Answer<D> {
    /// The answers `FluiHost.drainAnswers` returned: [`ANSWER_NUMBERS`]
    /// numbers each, oldest first, with one key and one `Intent` (or none)
    /// each. Lengths that disagree are a defect of one side, which the
    /// handshake rules out: nothing is read, and it is logged. An answer of
    /// a kind this library does not know, or whose key could not spell a
    /// [`RequestKey`], is skipped.
    pub(crate) fn from_drained(
        numbers: &[i64],
        keys: Vec<Option<String>>,
        data: Vec<Option<D>>,
    ) -> Vec<Self> {
        let (rows, rest) = numbers.as_chunks::<ANSWER_NUMBERS>();
        if !rest.is_empty() || rows.len() != keys.len() || rows.len() != data.len() {
            tracing::warn!(
                target: "flui_native::android",
                numbers = numbers.len(),
                keys = keys.len(),
                data = data.len(),
                "FluiHost's answers are not laid out as this library reads them"
            );
            return Vec::new();
        }
        rows.iter()
            .zip(keys)
            .zip(data)
            .filter_map(|((&[epoch, token, kind, code], key), data)| {
                let key = key.filter(|key| key_fits(key))?;
                let reply = match kind {
                    ANSWER_RESULT => Reply::Result {
                        code: ResultCode::from_raw(i32::try_from(code).ok()?),
                        data,
                    },
                    ANSWER_NOT_LAUNCHED => Reply::NotLaunched,
                    ANSWER_TOO_MANY => Reply::TooMany,
                    _ => {
                        tracing::warn!(
                            target: "flui_native::android",
                            kind,
                            "FluiHost queued an answer of a kind this library does not know"
                        );
                        return None;
                    }
                };
                Some(Self {
                    epoch: epoch.cast_unsigned(),
                    token: token.cast_unsigned(),
                    key: key.into_boxed_str(),
                    reply,
                })
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

/// One open request.
struct Open {
    token: ResultToken,
    key: RequestKey,
    purpose: Purpose,
    resolver: Resolver<ResultOutcome>,
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
    /// An earlier Activity's that nothing can receive: a refused launch,
    /// [`MAX_EARLIER`] answers already waiting, or this Activity has ended.
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

    /// Resolve each outcome. A runtime that retired since the request was
    /// taken receives nothing; its completion already ended.
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

/// An earlier Activity's answer waiting for its key.
struct Earlier<D> {
    key: Box<str>,
    code: ResultCode,
    data: Option<D>,
}

/// The requests of one Activity, the answers of earlier ones that wait for
/// their key, and the answered `Intent`s, of type `D`, held for
/// [`take_data`](Self::take_data).
pub(crate) struct ResultRouter<D> {
    /// This Activity's epoch, which `FluiHost.nextEpoch` minted.
    epoch: NonZeroU64,
    mint: ResultTokenMint,
    /// With `held`, at most [`MAX_OPEN_REQUESTS`].
    open: Vec<Open>,
    /// Earlier Activities' answers, oldest first; at most [`MAX_EARLIER`].
    earlier: VecDeque<Earlier<D>>,
    /// The `Intent` each answered request received, oldest first, until it
    /// is taken. Each came from an admitted request, so with `open` it stays
    /// within [`MAX_OPEN_REQUESTS`] and none is ever let go to make room.
    held: Vec<(ResultToken, D)>,
    ended: bool,
}

impl<D> ResultRouter<D> {
    /// The router of the Activity whose epoch is `epoch`.
    pub(crate) fn new(epoch: NonZeroU64) -> Self {
        Self {
            epoch,
            mint: ResultTokenMint::new(),
            open: Vec::new(),
            earlier: VecDeque::new(),
            held: Vec::new(),
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

    /// Whether another request may open now.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::ActivityEnded`] once [`end`](Self::end) ran, and
    /// [`ResultRefused::TooManyOpen`] while the open requests and the
    /// `Intent`s not taken yet number [`MAX_OPEN_REQUESTS`].
    pub(crate) fn admits(&self) -> Result<(), ResultRefused> {
        if self.ended {
            return Err(ResultRefused::ActivityEnded);
        }
        if self.open.len() + self.held.len() >= MAX_OPEN_REQUESTS {
            return Err(ResultRefused::TooManyOpen);
        }
        Ok(())
    }

    /// A token and a pending completion for one more request, if admitted.
    fn take_token(
        &self,
        completions: &CompletionSource,
    ) -> Result<(ResultToken, ResultRequest, Resolver<ResultOutcome>), ResultRefused> {
        self.admits()?;
        let token = self
            .mint
            .next()
            .map_err(|_| ResultRefused::TokensExhausted)?;
        let (completion, resolver) = completions.pending::<ResultOutcome>();
        Ok((token, ResultRequest::new(token, completion), resolver))
    }

    /// The answer `code` for the request `token`, holding `data` under it.
    fn settle(&mut self, token: ResultToken, code: ResultCode, data: Option<D>) -> ActivityResult {
        let carried = data.is_some();
        if let Some(data) = data {
            self.held.push((token, data));
        }
        ActivityResult::new(code, carried)
    }

    /// Open a request for an activity this Activity is about to start under
    /// `key`; the caller hands Java its token with [`epoch`](Self::epoch).
    ///
    /// # Errors
    ///
    /// As [`admits`](Self::admits), and [`ResultRefused::TokensExhausted`].
    pub(crate) fn open_launch(
        &mut self,
        completions: &CompletionSource,
        key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused> {
        let (token, request, resolver) = self.take_token(completions)?;
        self.open.push(Open {
            token,
            key,
            purpose: Purpose::Launched,
            resolver,
        });
        Ok(request)
    }

    /// Open a reattachment under `key`: settled with the oldest earlier
    /// answer under `key` when one waits, else by the next one to arrive
    /// while `still_to_come` says the host still holds an earlier request
    /// under `key` that has not been answered.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::NothingPending`] when nothing waits and nothing is
    /// to come, which opens nothing; otherwise as
    /// [`open_launch`](Self::open_launch).
    pub(crate) fn open_reattach(
        &mut self,
        completions: &CompletionSource,
        key: RequestKey,
        still_to_come: bool,
    ) -> Result<(ResultRequest, Delivery), ResultRefused> {
        self.admits()?;
        let waiting = self
            .earlier
            .iter()
            .position(|earlier| key.is_spelled(&earlier.key));
        if waiting.is_none() && !still_to_come {
            return Err(ResultRefused::NothingPending);
        }
        let (token, request, resolver) = self.take_token(completions)?;
        let mut delivery = Delivery::default();
        match waiting.and_then(|index| self.earlier.remove(index)) {
            Some(earlier) => {
                let outcome = Ok(self.settle(token, earlier.code, earlier.data));
                delivery.push(resolver, outcome);
            }
            None => self.open.push(Open {
                token,
                key,
                purpose: Purpose::Reattached,
                resolver,
            }),
        }
        Ok((request, delivery))
    }

    /// Route `answer` to the request it answers.
    pub(crate) fn answer(&mut self, answer: Answer<D>) -> (Routed, Delivery) {
        let mut delivery = Delivery::default();
        if answer.epoch == self.epoch.get() {
            let index = self.open.iter().position(|open| {
                open.purpose == Purpose::Launched && open.token.get() == answer.token
            });
            let Some(index) = index else {
                return (Routed::Stale, delivery);
            };
            let open = self.open.swap_remove(index);
            let outcome = match answer.reply {
                Reply::Result { code, data } => Ok(self.settle(open.token, code, data)),
                Reply::NotLaunched => Err(ResultRefused::NotLaunched),
                Reply::TooMany => Err(ResultRefused::TooManyOpen),
            };
            delivery.push(open.resolver, outcome);
            return (Routed::Settled, delivery);
        }
        let Reply::Result { code, data } = answer.reply else {
            // The earlier Activity's request never started; its runtime
            // ended with it, and nothing waits for a refusal.
            return (Routed::LetGo, delivery);
        };
        let waiting = self.open.iter().position(|open| {
            open.purpose == Purpose::Reattached && open.key.is_spelled(&answer.key)
        });
        if let Some(index) = waiting {
            // `remove`, not `swap_remove`: of two reattachments under one
            // key, the older receives the older answer.
            let open = self.open.remove(index);
            let outcome = Ok(self.settle(open.token, code, data));
            delivery.push(open.resolver, outcome);
            return (Routed::Reattached, delivery);
        }
        if self.ended || self.earlier.len() >= MAX_EARLIER {
            return (Routed::LetGo, delivery);
        }
        self.earlier.push_back(Earlier {
            key: answer.key,
            code,
            data,
        });
        (Routed::Waiting, delivery)
    }

    /// The host could not hand the launched request `token` to Android: it
    /// ends as [`ResultRefused::HandOffFailed`]. `None` when it is not open
    /// (answered or cancelled meanwhile).
    pub(crate) fn hand_off_failed(&mut self, token: ResultToken) -> Option<Delivery> {
        let index = self
            .open
            .iter()
            .position(|open| open.purpose == Purpose::Launched && open.token == token)?;
        let open = self.open.remove(index);
        let mut delivery = Delivery::default();
        delivery.push(open.resolver, Err(ResultRefused::HandOffFailed));
        Some(delivery)
    }

    /// The `Intent` the request `token` was answered with, once; `None` when
    /// it carried none, it was taken, or the Activity ended.
    pub(crate) fn take_data(&mut self, token: ResultToken) -> Option<D> {
        let index = self.held.iter().position(|(held, _)| *held == token)?;
        Some(self.held.remove(index).1)
    }

    /// End the open request `token` as [`ResultRefused::Cancelled`]; what it
    /// was open for, or `None` when it was not open.
    pub(crate) fn cancel(&mut self, token: ResultToken) -> Option<(Purpose, Delivery)> {
        let index = self.open.iter().position(|open| open.token == token)?;
        let open = self.open.remove(index);
        let mut delivery = Delivery::default();
        delivery.push(open.resolver, Err(ResultRefused::Cancelled));
        Some((open.purpose, delivery))
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
        let mut delivery = Delivery::default();
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::{Arc, Mutex};

    use flui_platform::capability::{
        Abandoned, Completion, CompletionSource, MAX_OPEN_REQUESTS, MAX_REQUEST_KEY_LEN, OwnerJob,
        OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions, RequestKey, ResultCode,
        ResultKey, ResultOutcome, ResultRefused, ResultRequest, ResultToken,
    };

    use super::{
        ANSWER_NOT_LAUNCHED, ANSWER_NUMBERS, ANSWER_RESULT, ANSWER_TOO_MANY, Answer, Delivery,
        MAX_EARLIER, Purpose, Reply, ResultRouter, Routed,
    };

    const fn key(name: &'static str) -> ResultKey {
        match ResultKey::try_new(name) {
            Ok(key) => key,
            Err(_) => panic!("BUG: a test key is printable and short"),
        }
    }

    /// One capability's key, and the same key under another capability.
    const PICK: RequestKey = RequestKey::new(key("files"), key("pick"));
    const SHARE: RequestKey = RequestKey::new(key("files"), key("share"));
    const OTHER_PICK: RequestKey = RequestKey::new(key("camera"), key("pick"));

    fn epoch(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("BUG: test epochs are not zero")
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

    /// A request a test took: its token and its completion.
    struct Asked {
        token: ResultToken,
        completion: Completion<ResultOutcome>,
    }

    impl Asked {
        fn of(request: ResultRequest) -> Self {
            let (token, completion) = request.into_parts();
            Self { token, completion }
        }

        fn token(&self) -> ResultToken {
            self.token
        }

        /// The outcome once settled, `None` while it waits and once taken.
        fn outcome(&mut self) -> Option<Result<ResultOutcome, Abandoned>> {
            self.completion.poll()
        }
    }

    /// The `Intent` a test's answer carries: a name the router holds as it
    /// would hold a JNI global reference.
    type Held = &'static str;

    /// The router a test drives.
    type Router = ResultRouter<Held>;

    fn launch(router: &mut Router, owner: &Owner, key: RequestKey) -> Asked {
        Asked::of(router.open_launch(&owner.source, key).expect("admitted"))
    }

    /// A reattachment under `key` while an earlier request is still to come.
    fn reattach(router: &mut Router, owner: &Owner, key: RequestKey) -> (Asked, Delivery) {
        let (request, delivery) = router
            .open_reattach(&owner.source, key, true)
            .expect("admitted");
        (Asked::of(request), delivery)
    }

    /// An answer of `code` for the request `token` of `epoch` under `key`,
    /// carrying the `Intent` `"intent"`.
    fn result(epoch: u64, token: u64, key: RequestKey, code: ResultCode) -> Answer<Held> {
        answer(
            epoch,
            token,
            key,
            Reply::Result {
                code,
                data: Some("intent"),
            },
        )
    }

    fn answer(epoch: u64, token: u64, key: RequestKey, reply: Reply<Held>) -> Answer<Held> {
        Answer {
            epoch,
            token,
            key: key.to_string().into_boxed_str(),
            reply,
        }
    }

    fn code_of(settled: Option<Result<ResultOutcome, Abandoned>>) -> Result<ResultCode, String> {
        match settled {
            Some(Ok(Ok(result))) => Ok(result.code()),
            other => Err(format!("{other:?}")),
        }
    }

    fn refusal_of(settled: Option<Result<ResultOutcome, Abandoned>>) -> Option<ResultRefused> {
        match settled {
            Some(Ok(Err(refused))) => Some(refused),
            _ => None,
        }
    }

    /// Each answer settles the request whose token it carries, once, at the
    /// next owner turn and not inside the drain; a second answer for that
    /// token and one for a token never minted are stale. Two requests in
    /// flight settle in the order their answers arrive.
    #[test]
    fn an_answer_settles_the_request_its_token_names_once() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(7));
        let mut first = launch(&mut router, &owner, PICK);
        let mut second = launch(&mut router, &owner, PICK);
        assert_ne!(first.token(), second.token(), "one token per call");

        let (routed, delivery) =
            router.answer(result(7, second.token().get(), PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        assert!(second.outcome().is_none(), "settles at an owner turn");
        owner.turn();
        assert_eq!(code_of(second.outcome()), Ok(ResultCode::Ok));
        assert!(first.outcome().is_none(), "the other request still waits");

        let (routed, delivery) =
            router.answer(result(7, second.token().get(), PICK, ResultCode::Canceled));
        assert_eq!(routed, Routed::Stale, "an answered token is not open");
        assert_eq!(delivery.len(), 0);
        let (routed, _) = router.answer(result(7, u64::MAX, PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Stale, "a token never minted");

        let (routed, delivery) =
            router.answer(result(7, first.token().get(), PICK, ResultCode::Other(5)));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        owner.turn();
        assert_eq!(code_of(first.outcome()), Ok(ResultCode::Other(5)));
        assert_eq!(router.open_len(), 0);
    }

    /// Every way a launched request ends without an answer ends it with its
    /// own reason: Android refused (`NotLaunched`), the host's Java side was
    /// full (`TooManyOpen`), the host could not hand it over
    /// (`HandOffFailed`, for a failed call, a dropped closure or a panic
    /// alike), or the provider cancelled it (`Cancelled`, and Java finishes
    /// the activity, whose later answer is stale). A token handled twice
    /// does nothing the second time.
    #[test]
    fn each_launch_that_is_not_answered_ends_with_its_reason() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(1));
        let mut refused = launch(&mut router, &owner, PICK);
        let mut full = launch(&mut router, &owner, PICK);
        let mut lost = launch(&mut router, &owner, PICK);
        let mut cancelled = launch(&mut router, &owner, SHARE);

        let (routed, delivery) =
            router.answer(answer(1, refused.token().get(), PICK, Reply::NotLaunched));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        let (routed, delivery) = router.answer(answer(1, full.token().get(), PICK, Reply::TooMany));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        router
            .hand_off_failed(lost.token())
            .expect("open")
            .deliver();
        assert!(router.hand_off_failed(lost.token()).is_none(), "ended once");
        let (purpose, delivery) = router.cancel(cancelled.token()).expect("open");
        assert_eq!(purpose, Purpose::Launched, "Java finishes the activity");
        delivery.deliver();
        assert!(router.cancel(cancelled.token()).is_none());
        let (routed, _) = router.answer(result(
            1,
            cancelled.token().get(),
            SHARE,
            ResultCode::Canceled,
        ));
        assert_eq!(routed, Routed::Stale);

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
        assert_eq!(router.open_len(), 0);
    }

    /// At most `MAX_OPEN_REQUESTS` are open, reattachments and answered
    /// `Intent`s not taken yet included; the next is refused and takes no
    /// token. An `Intent` is never let go to make room: taking it does.
    #[test]
    fn the_open_requests_and_untaken_intents_are_bounded() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(1));
        let mut open: Vec<Asked> = (1..MAX_OPEN_REQUESTS)
            .map(|_| launch(&mut router, &owner, PICK))
            .collect();
        let (reattached, delivery) = reattach(&mut router, &owner, SHARE);
        assert_eq!(delivery.len(), 0, "no earlier answer waits");
        assert_eq!(router.admits(), Err(ResultRefused::TooManyOpen));
        assert!(matches!(
            router.open_launch(&owner.source, PICK),
            Err(ResultRefused::TooManyOpen)
        ));
        assert!(matches!(
            router.open_reattach(&owner.source, PICK, true),
            Err(ResultRefused::TooManyOpen)
        ));

        let last = open.pop().expect("one open");
        router
            .answer(result(1, last.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        assert_eq!(
            router.admits(),
            Err(ResultRefused::TooManyOpen),
            "the answered Intent still counts until it is taken"
        );
        assert_eq!(router.take_data(last.token()), Some("intent"));
        let next = launch(&mut router, &owner, PICK);
        assert!(next.token() > reattached.token(), "tokens keep increasing");
    }

    /// An answer of another epoch, an earlier Activity's (epoch 0 is one
    /// restored from a process that died), never settles this Activity's
    /// request, even one whose token has the same value: it waits for a
    /// reattachment under its key, which settles it whether it comes first or
    /// second, the older reattachment taking the older answer. The same key
    /// of another capability never receives it.
    #[test]
    fn an_earlier_activitys_answer_waits_for_its_key() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(2));
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
        assert_eq!(code_of(first.outcome()), Ok(ResultCode::Ok), "oldest first");
        assert_eq!(code_of(second.outcome()), Ok(ResultCode::Other(2)));
        assert!(ours.outcome().is_none(), "ours still waits for its own");
        assert!(other.outcome().is_none());

        let (mut later, delivery) = reattach(&mut router, &owner, SHARE);
        assert_eq!(delivery.len(), 0);
        let (routed, delivery) = router.answer(result(1, 9, SHARE, ResultCode::Canceled));
        assert_eq!(routed, Routed::Reattached);
        delivery.deliver();
        owner.turn();
        assert_eq!(
            code_of(share.outcome()),
            Ok(ResultCode::Canceled),
            "the reattachment waiting longest"
        );
        assert!(later.outcome().is_none());

        let (routed, _) = router.answer(answer(1, 3, PICK, Reply::NotLaunched));
        assert_eq!(routed, Routed::LetGo, "an earlier refusal reaches nobody");
    }

    /// A reattachment with no earlier answer waiting and none still to come
    /// is refused as `NothingPending` and holds no place; one still to come
    /// waits for it.
    #[test]
    fn a_reattachment_with_nothing_pending_holds_nothing() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(3));
        assert!(matches!(
            router.open_reattach(&owner.source, PICK, false),
            Err(ResultRefused::NothingPending)
        ));
        assert_eq!(router.open_len(), 0);

        router
            .answer(result(1, 1, PICK, ResultCode::Ok))
            .1
            .deliver();
        let (_, delivery) = router
            .open_reattach(&owner.source, PICK, false)
            .expect("an earlier answer waits");
        assert_eq!(delivery.len(), 1);
        delivery.deliver();
        let (_, delivery) = reattach(&mut router, &owner, PICK);
        assert_eq!(delivery.len(), 0);
        assert_eq!(router.open_len(), 1, "it waits for the answer to come");
    }

    /// At most `MAX_EARLIER` earlier answers wait; one more is let go, and
    /// the waiting ones are kept.
    #[test]
    fn the_earlier_answers_that_wait_are_bounded() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(3));
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
        let mut router = Router::new(epoch(4));
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
            router.open_launch(&owner.source, PICK),
            Err(ResultRefused::ActivityEnded)
        ));
        assert_eq!(
            router.answer(result(1, 2, PICK, ResultCode::Ok)).0,
            Routed::LetGo
        );
    }

    /// An answer that arrives after the runtime that asked retired settles
    /// nothing and does not fail: the request already ended as
    /// `OwnerRetired`.
    #[test]
    fn an_answer_after_the_runtime_retired_reaches_nobody() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(5));
        let mut request = launch(&mut router, &owner, PICK);
        owner.retire();
        assert!(matches!(
            request.outcome(),
            Some(Err(Abandoned::OwnerRetired))
        ));
        let (routed, delivery) =
            router.answer(result(5, request.token().get(), PICK, ResultCode::Ok));
        assert_eq!(routed, Routed::Settled);
        delivery.deliver();
        assert!(request.outcome().is_none(), "settled once, before");
    }

    /// The `Intent` an answer carries stays with the router, under the token
    /// of the request it settled (a reattachment's own, for an earlier
    /// Activity's answer), and is taken once; the end lets every one go.
    #[test]
    fn an_answered_intent_is_taken_once_under_its_request() {
        let owner = Owner::new();
        let mut router = Router::new(epoch(6));
        let mut asked = launch(&mut router, &owner, PICK);
        router
            .answer(result(6, asked.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        owner.turn();
        match asked.outcome() {
            Some(Ok(Ok(result))) => assert!(result.has_data(), "the answer says it carried one"),
            other => panic!("the answer: {other:?}"),
        }
        assert_eq!(router.take_data(asked.token()), Some("intent"));
        assert_eq!(router.take_data(asked.token()), None, "taken once");

        let _ = router.answer(result(1, 1, SHARE, ResultCode::Ok));
        let (reattached, delivery) = reattach(&mut router, &owner, SHARE);
        delivery.deliver();
        assert_eq!(router.take_data(reattached.token()), Some("intent"));

        let untaken = launch(&mut router, &owner, PICK);
        router
            .answer(result(6, untaken.token().get(), PICK, ResultCode::Ok))
            .1
            .deliver();
        let _ = router.end();
        assert_eq!(router.take_data(untaken.token()), None, "the end lets go");
    }

    /// The answers are read as `FluiHost` lays them out, with the kinds and
    /// the bound the Java side states; an unknown kind and a key that could
    /// not spell a `RequestKey` are skipped, and lengths that disagree read
    /// nothing.
    #[test]
    fn answers_are_read_as_the_java_classes_lay_them_out() {
        let host = super::super::java_host::java_source(super::super::java_host::HOST_CLASS);
        for stated in [
            format!("static final int ANSWER_RESULT = {ANSWER_RESULT};"),
            format!("static final int ANSWER_NOT_LAUNCHED = {ANSWER_NOT_LAUNCHED};"),
            format!("static final int ANSWER_TOO_MANY = {ANSWER_TOO_MANY};"),
            format!("static final int MAX_OPEN = {MAX_OPEN_REQUESTS};"),
            format!("static final int ANSWER_NUMBERS = {ANSWER_NUMBERS};"),
        ] {
            assert!(host.contains(&stated), "{stated:?} in FluiHost");
        }

        let long_key = "k".repeat(MAX_REQUEST_KEY_LEN + 1);
        let answers = Answer::from_drained(
            &[
                7,
                1,
                ANSWER_RESULT,
                -1, //
                7,
                2,
                ANSWER_NOT_LAUNCHED,
                0, //
                7,
                3,
                99,
                0, //
                7,
                4,
                ANSWER_RESULT,
                0, //
                0,
                5,
                ANSWER_RESULT,
                3, //
                7,
                6,
                ANSWER_TOO_MANY,
                0,
            ],
            vec![
                Some("files/pick".to_owned()),
                Some("files/pick".to_owned()),
                Some("files/pick".to_owned()),
                Some(long_key),
                Some("files/share".to_owned()),
                Some("files/pick".to_owned()),
            ],
            vec![Some("intent"), None, None, None, None, None],
        );
        let read: Vec<_> = answers
            .iter()
            .map(|answer| {
                let reply = match &answer.reply {
                    Reply::Result { code, data } => Some((*code, data.is_some())),
                    Reply::NotLaunched | Reply::TooMany => None,
                };
                (answer.epoch, answer.token, &*answer.key, reply)
            })
            .collect();
        assert_eq!(
            read,
            [
                (7, 1, "files/pick", Some((ResultCode::Ok, true))),
                (7, 2, "files/pick", None),
                (0, 5, "files/share", Some((ResultCode::Other(3), false))),
                (7, 6, "files/pick", None),
            ]
        );
        assert!(matches!(answers[3].reply, Reply::TooMany));
        assert!(
            Answer::<Held>::from_drained(&[7, 1, ANSWER_RESULT], vec![None], vec![None]).is_empty(),
            "half an answer"
        );
        assert!(
            Answer::<Held>::from_drained(&[7, 1, ANSWER_RESULT, 0], vec![], vec![None]).is_empty(),
            "a key short"
        );
    }
}
