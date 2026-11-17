//! Activity results: how a capability provider starts an Android activity and
//! hears what it answers (ADR-0154 §9, ADR-0255 decision 8).
//!
//! The host owns the Activity, so a provider reaches it through its context:
//! [`ProviderContext::activity`](super::ProviderContext::activity) hands out
//! the [`Activity`] while one runs, and `None` on every other platform and
//! between two Activities. A request is identified by a [`ResultToken`] the
//! host mints per call, never by an integer the crates share, and it is
//! answered by a [`Completion`] that ends exactly once: with the
//! [`ActivityResult`], or with the [`ResultRefused`] that says why there is
//! none.
//!
//! Android replaces an Activity (a configuration change it does not declare,
//! "don't keep activities", the process dying behind another app) and FLUI's
//! runtime ends with it. A request in flight then ends as
//! [`ResultRefused::ActivityEnded`], and the answer it gets later reaches the
//! next Activity, where the request's [`ResultKey`] claims it through
//! [`Activity::reattach`]: the key, unlike the token, survives the
//! replacement, the process's death included.
//!
//! No JNI type appears here. The provider brings its own `jni` crate, of
//! whatever version it chooses, and the host lends it raw pointers for the
//! length of one call ([`JniFrame`]).

use std::{cell::Cell, ffi::c_void, fmt, num::NonZeroU64, ptr::NonNull, rc::Rc};

use flui_foundation::{IdExhausted, IdMint};

use super::{Completion, CompletionSource};

/// The identity of one request an [`Activity`] took, minted by the host per
/// call.
///
/// Values never repeat within the Activity that minted them, and the host
/// matches an answer to its request by the token together with the Activity's
/// own identity, so an answer meant for an earlier Activity never settles a
/// request of a later one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResultToken(NonZeroU64);

impl ResultToken {
    /// The raw value, which the host hands its Java side and compares with
    /// what comes back; there is no conversion from a number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Where a host mints its [`ResultToken`]s: increasing, never reissued, and
/// within the positive range of a Java `long`, which carries them across.
#[derive(Debug)]
pub struct ResultTokenMint(IdMint<NonZeroU64>);

impl ResultTokenMint {
    /// A mint that has issued nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self(IdMint::new().ending_at(i64::MAX.unsigned_abs()))
    }

    /// The next token.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once the last value was issued; the mint never wraps.
    pub fn next(&self) -> Result<ResultToken, IdExhausted> {
        self.0.next().map(ResultToken)
    }
}

impl Default for ResultTokenMint {
    fn default() -> Self {
        Self::new()
    }
}

/// The most bytes a [`ResultKey`] holds.
pub const MAX_RESULT_KEY_LEN: usize = 64;

/// The name a provider gives the requests of one purpose
/// (`"flui-system.open-document"`), which outlives the Activity: an answer
/// that arrives after the Activity was replaced is claimed by this key
/// ([`Activity::reattach`]).
///
/// A constant of the provider's crate: 1 to [`MAX_RESULT_KEY_LEN`] bytes of
/// printable ASCII, so it is saved with the Activity's state as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResultKey(&'static str);

/// Why a name is not a [`ResultKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidResultKey {
    /// The name is empty.
    #[error("a result key is empty")]
    Empty,
    /// The name is longer than [`MAX_RESULT_KEY_LEN`] bytes.
    #[error("a result key is longer than {MAX_RESULT_KEY_LEN} bytes")]
    TooLong,
    /// The name holds a byte outside printable ASCII (`!` to `~`).
    #[error("a result key holds a byte outside printable ASCII")]
    NotPrintable,
}

impl ResultKey {
    /// `name` as a key; `const`, so a provider names its keys as constants.
    ///
    /// # Errors
    ///
    /// [`InvalidResultKey`] when `name` is empty, longer than
    /// [`MAX_RESULT_KEY_LEN`] bytes or holds a byte outside `!` to `~`.
    pub const fn try_new(name: &'static str) -> Result<Self, InvalidResultKey> {
        match Self::check(name) {
            Ok(()) => Ok(Self(name)),
            Err(invalid) => Err(invalid),
        }
    }

    /// Whether `name` could be a key, by the rule of
    /// [`try_new`](Self::try_new): what a host holds a part read back from
    /// Java to.
    ///
    /// # Errors
    ///
    /// As [`try_new`](Self::try_new).
    pub const fn check(name: &str) -> Result<(), InvalidResultKey> {
        let bytes = name.as_bytes();
        if bytes.is_empty() {
            return Err(InvalidResultKey::Empty);
        }
        if bytes.len() > MAX_RESULT_KEY_LEN {
            return Err(InvalidResultKey::TooLong);
        }
        let mut index = 0;
        while index < bytes.len() {
            if !bytes[index].is_ascii_graphic() {
                return Err(InvalidResultKey::NotPrintable);
            }
            index += 1;
        }
        Ok(())
    }

    /// The name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// The code an activity finished with (`Activity.setResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "Android's two named codes and every other value: the whole range of an int"
)]
pub enum ResultCode {
    /// `RESULT_OK`: the activity did what it was started for.
    Ok,
    /// `RESULT_CANCELED`: the user backed out, or the activity ended without
    /// setting a result.
    Canceled,
    /// Any other code, which the started activity defines
    /// (`RESULT_FIRST_USER` and up).
    Other(i32),
}

impl ResultCode {
    /// The code Android reported.
    #[must_use]
    pub const fn from_raw(code: i32) -> Self {
        match code {
            -1 => Self::Ok,
            0 => Self::Canceled,
            other => Self::Other(other),
        }
    }

    /// The code as Android states it.
    #[must_use]
    pub const fn raw(self) -> i32 {
        match self {
            Self::Ok => -1,
            Self::Canceled => 0,
            Self::Other(code) => code,
        }
    }
}

/// What a JNI call of a provider is lent by the host for the length of one
/// call: the process's `JavaVM*`, the calling thread's `JNIEnv*` and a
/// reference to one object.
///
/// The provider wraps them with its own `jni` crate. The `JavaVM*` lives as
/// long as the process; the `JNIEnv*` and the object are valid on the
/// calling thread until the call that lent them returns, and must not be kept
/// past it.
#[derive(Debug, Clone, Copy)]
pub struct JniFrame {
    vm: NonNull<c_void>,
    env: NonNull<c_void>,
    object: NonNull<c_void>,
}

impl JniFrame {
    /// The frame of a host's call: `vm` and `env` as JNI hands them out, and
    /// `object` a reference valid until the call returns.
    #[must_use]
    pub const fn new(vm: NonNull<c_void>, env: NonNull<c_void>, object: NonNull<c_void>) -> Self {
        Self { vm, env, object }
    }

    /// The process's `JavaVM*`.
    #[must_use]
    pub const fn vm(&self) -> *mut c_void {
        self.vm.as_ptr()
    }

    /// The calling thread's `JNIEnv*`.
    #[must_use]
    pub const fn env(&self) -> *mut c_void {
        self.env.as_ptr()
    }

    /// The lent object: the Activity, or the `Intent` of a result.
    #[must_use]
    pub const fn object(&self) -> *mut c_void {
        self.object.as_ptr()
    }
}

/// What a provider is lent to fill the `Intent` of a request
/// ([`Activity::launch_for_result`]): the [`JniFrame`] of the Activity and a
/// new, empty `android.content.Intent` the host made and launches once the
/// provider has filled it.
///
/// No reference flows from the provider to the host: the provider only calls
/// methods of the two objects it was lent (`setAction`, `addCategory`,
/// `putExtra`, ...), so a mistake in its JNI code cannot hand the host a
/// pointer it would trust.
#[derive(Debug, Clone, Copy)]
pub struct IntentFrame {
    activity: JniFrame,
    intent: NonNull<c_void>,
}

impl IntentFrame {
    /// `intent`, a reference to the empty `Intent` the host made, lent with
    /// the Activity's frame until the call returns.
    #[must_use]
    pub const fn new(activity: JniFrame, intent: NonNull<c_void>) -> Self {
        Self { activity, intent }
    }

    /// The Activity's frame: the VM, the thread's `JNIEnv*` and the Activity.
    #[must_use]
    pub const fn activity(&self) -> JniFrame {
        self.activity
    }

    /// The `Intent` to fill.
    #[must_use]
    pub const fn intent(&self) -> *mut c_void {
        self.intent.as_ptr()
    }
}

/// What a started activity answered: its code, and the `Intent` it carried,
/// if any, as a [`ResultData`].
///
/// The `Intent` stays with the host, on the owner thread; the provider reads
/// it once by handing the [`ResultData`] to [`Activity::take_data`]. Its
/// extras come from another application: the provider reads only what it
/// asked for and checks each size before it copies. Dropping the result, or
/// its `ResultData`, unread lets the host release the `Intent`: a result
/// nobody receives (a dropped completion, a retired runtime, a provider that
/// went) holds nothing. Only the host makes one ([`ResultMint::result`]).
#[derive(Debug)]
pub struct ActivityResult {
    code: ResultCode,
    data: Option<ResultData>,
}

impl ActivityResult {
    /// The code the activity finished with.
    #[must_use]
    pub fn code(&self) -> ResultCode {
        self.code
    }

    /// Whether the activity answered with an `Intent` not taken yet.
    #[must_use]
    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// The `Intent` the activity answered with, for
    /// [`Activity::take_data`]; `None` once taken.
    pub fn take_data(&mut self) -> Option<ResultData> {
        self.data.take()
    }
}

/// The identity of the mint that made a value: compared by address, so a
/// value one Activity's host handed out is told apart from another's.
#[derive(Clone)]
struct MintIdentity(Rc<()>);

impl MintIdentity {
    fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for MintIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MintIdentity")
    }
}

/// The provider's side of something the host keeps for it under a token:
/// dropping it records the release and wakes the host, which reads
/// [`Released`] at its next owner turn. Owner thread; the drop runs no
/// user code and no JNI.
struct Holder {
    token: ResultToken,
    mint: MintIdentity,
    held: Rc<Cell<bool>>,
    wake: Rc<dyn Fn()>,
}

/// The host's side of a [`Holder`].
#[derive(Debug)]
struct Released(Rc<Cell<bool>>);

impl Holder {
    fn new(token: ResultToken, mint: MintIdentity, wake: Rc<dyn Fn()>) -> (Self, Released) {
        let held = Rc::new(Cell::new(true));
        (
            Self {
                token,
                mint,
                held: Rc::clone(&held),
                wake,
            },
            Released(held),
        )
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        self.held.set(false);
        (self.wake)();
    }
}

impl fmt::Debug for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Holder")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl Released {
    fn get(&self) -> bool {
        !self.0.get()
    }
}

/// The `Intent` one answer carried, held by the host until this value is
/// handed to [`Activity::take_data`] or dropped. Only the host makes one
/// ([`ResultMint::data`]), so a provider holds only the data the host handed
/// it.
#[derive(Debug)]
pub struct ResultData(Holder);

/// The host's side of a [`ResultData`]: whether the provider still holds it.
#[derive(Debug)]
pub struct ResultDataHold(Released);

impl ResultData {
    /// The request whose answer carried it.
    #[must_use]
    pub fn token(&self) -> ResultToken {
        self.0.token
    }
}

impl ResultDataHold {
    /// Whether the provider let the [`ResultData`] go unread, so the host
    /// releases the `Intent`.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.0.get()
    }
}

/// A provider's claim on one open request: while it lives, the request waits
/// for its answer; dropped, the request ends as cancelled: an activity it
/// started is finished, a reattachment stops waiting, and the host frees its
/// place. Dropping it is the one way to cancel. A provider that wants the
/// answer keeps it, in its state or in the closure its completion calls;
/// whatever a retired runtime or provider owned goes with it. Only the host
/// makes one ([`ResultMint::ticket`]).
#[derive(Debug)]
#[must_use = "dropping the ticket cancels its request"]
pub struct ResultTicket(Holder);

/// The host's side of a [`ResultTicket`]: whether the provider still holds
/// it.
#[derive(Debug)]
pub struct ResultTicketHold(Released);

impl ResultTicket {
    /// The request it claims.
    #[must_use]
    pub fn token(&self) -> ResultToken {
        self.0.token
    }
}

impl ResultTicketHold {
    /// Whether the provider let the [`ResultTicket`] go, so the host ends
    /// its request.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.0.get()
    }
}

/// What makes the values a host hands a provider: [`ResultTicket`],
/// [`ResultData`], [`ActivityResult`] and [`ResultRequest`]. It has no
/// public constructor: a host receives one when the composition root offers
/// its bridge ([`HostBridge::install_activity`], [`ActivityBridge::bind`]),
/// so no provider makes the values it then hands back to the host.
#[derive(Clone)]
pub struct ResultMint {
    identity: MintIdentity,
}

impl ResultMint {
    fn new() -> Self {
        Self {
            identity: MintIdentity(Rc::new(())),
        }
    }

    /// The ticket of the request `token`, whose drop calls `wake`, and the
    /// side the host keeps to learn when the provider lets it go.
    pub fn ticket(
        &self,
        token: ResultToken,
        wake: Rc<dyn Fn()>,
    ) -> (ResultTicket, ResultTicketHold) {
        let (holder, released) = Holder::new(token, self.identity.clone(), wake);
        (ResultTicket(holder), ResultTicketHold(released))
    }

    /// The `Intent` held for the request `token`, whose drop calls `wake`,
    /// and the side the host keeps to learn when the provider lets it go.
    #[must_use]
    pub fn data(&self, token: ResultToken, wake: Rc<dyn Fn()>) -> (ResultData, ResultDataHold) {
        let (holder, released) = Holder::new(token, self.identity.clone(), wake);
        (ResultData(holder), ResultDataHold(released))
    }

    /// The answer `code`, with the `Intent` it carried as `data`.
    #[must_use]
    pub fn result(&self, code: ResultCode, data: Option<ResultData>) -> ActivityResult {
        ActivityResult { code, data }
    }

    /// The request `ticket` claims, answered through `completion`.
    pub fn request(
        &self,
        ticket: ResultTicket,
        completion: Completion<ResultOutcome>,
    ) -> ResultRequest {
        ResultRequest { ticket, completion }
    }

    /// Whether this mint made `data`: data another Activity's host handed
    /// out names a token of that Activity, not of this one.
    #[must_use]
    pub fn made(&self, data: &ResultData) -> bool {
        self.identity.is(&data.0.mint)
    }
}

impl fmt::Debug for ResultMint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResultMint")
    }
}

/// Why the host does not lend an `Intent` to [`Activity::take_data`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum DataRefused {
    /// The data was handed to another capability: a provider reads only the
    /// `Intent`s of its own requests.
    #[error("the Intent belongs to another capability's request")]
    OtherCapability,
    /// The host holds no `Intent` for it: another Activity's host handed it
    /// out, or the Activity has ended.
    #[error("the host holds no Intent for this data")]
    NotHeld,
    /// The thread could not reach Java to lend it.
    #[error("the Intent could not be lent")]
    NotLent,
}

/// Why a request has no [`ActivityResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ResultRefused {
    /// The host holds as many requests as it holds: across the process
    /// [`MAX_OPEN_REQUESTS`] not yet answered, or [`MAX_OPEN_REQUESTS_PER_CAPABILITY`]
    /// of one capability's, an earlier Activity's included; or as many
    /// answered `Intent`s nobody took yet, or launches not yet handed to
    /// Android. One must end first.
    #[error("as many activity requests are open as the host holds")]
    TooManyOpen,
    /// The Activity minted its last token.
    #[error("the Activity has no token left for another request")]
    TokensExhausted,
    /// No `Intent` was made for the request: the host could not make the
    /// empty one, or the provider's closure answered `false` or left a Java
    /// exception pending.
    #[error("no Intent was made for the request")]
    IntentNotMade,
    /// Android refused to start the activity (no application handles the
    /// `Intent`, or the caller may not start it).
    #[error("Android did not start the activity")]
    NotLaunched,
    /// The host could not hand the request to Android: its call failed or
    /// panicked, or the work it posted to Android's main thread was dropped.
    #[error("the host could not hand the request to Android")]
    HandOffFailed,
    /// The provider dropped the request's [`ResultTicket`].
    #[error("the request was cancelled")]
    Cancelled,
    /// No earlier Activity's answer under the key waits, and none is still
    /// to come ([`Activity::reattach`]).
    #[error("no earlier answer under the key waits or is still to come")]
    NothingPending,
    /// The Activity ended before the answer. If Android starts it again, the
    /// answer reaches the new Activity, where [`Activity::reattach`] under the
    /// request's key receives it.
    #[error("the Activity ended before the answer arrived")]
    ActivityEnded,
    /// The activity answered `code` with an `Intent` the host could not keep
    /// (Java ran out of memory as it took it), so the answer is not
    /// delivered as one that carried no `Intent`.
    #[error("the activity answered {code:?} with an Intent the host could not keep")]
    DataLost {
        /// The code the activity finished with.
        code: ResultCode,
    },
    /// The host could not read the answer `FluiHost` queued for the request
    /// (a kind or a code it does not know: the Java classes and the library
    /// disagree).
    #[error("the host could not read the request's answer")]
    AnswerUnreadable,
}

/// The most requests the host holds not yet answered across the process,
/// every capability and Activity together; one more is refused as
/// [`ResultRefused::TooManyOpen`].
pub const MAX_OPEN_REQUESTS: usize = 16;

/// The most of [`MAX_OPEN_REQUESTS`] one capability holds, and the most
/// answered `Intent`s of one capability nobody took yet, so one provider
/// cannot keep the others from asking.
pub const MAX_OPEN_REQUESTS_PER_CAPABILITY: usize = MAX_OPEN_REQUESTS / 2;

/// What a request answers with.
pub type ResultOutcome = Result<ActivityResult, ResultRefused>;

/// The identity of one registered capability, minted as its provider is
/// registered ([`CapabilityRegistrar::provide`](super::CapabilityRegistrar::provide)):
/// its name, and an identity compared by address, so no caller names
/// another capability by its string. An [`Activity`] is scoped by one.
#[derive(Clone)]
pub struct CapabilityId(Rc<CapabilityName>);

/// What a [`CapabilityId`] names; its allocation is the identity.
struct CapabilityName {
    name: &'static str,
}

impl CapabilityId {
    pub(crate) fn mint(name: &'static str) -> Self {
        Self(Rc::new(CapabilityName { name }))
    }

    /// The capability's [`NAME`](super::Capability::NAME).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.0.name
    }
}

impl PartialEq for CapabilityId {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CapabilityId {}

impl fmt::Debug for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CapabilityId").field(&self.0.name).finish()
    }
}

/// A request's key as the host keeps it across Activities: the name of the
/// capability that took it, which the registry holds unique, and the
/// provider's [`ResultKey`]. Two providers cannot receive each other's
/// answers by choosing the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestKey {
    capability: ResultKey,
    key: ResultKey,
}

impl RequestKey {
    /// The key `key` of the capability named `capability`.
    #[must_use]
    pub const fn new(capability: ResultKey, key: ResultKey) -> Self {
        Self { capability, key }
    }

    /// The capability's name.
    #[must_use]
    pub const fn capability(self) -> ResultKey {
        self.capability
    }

    /// The provider's key.
    #[must_use]
    pub const fn key(self) -> ResultKey {
        self.key
    }

    /// Whether `capability` and `key` are this key's two parts, compared
    /// apart: the host stores and compares them apart everywhere, so no two
    /// keys can meet by how they would read joined.
    #[must_use]
    pub fn matches(self, capability: &str, key: &str) -> bool {
        self.capability.as_str() == capability && self.key.as_str() == key
    }
}

/// A request the [`Activity`] took: its [`ResultTicket`], and the completion
/// its answer settles. Dropping it, or its ticket, cancels the request. Only
/// the host makes one ([`ResultMint::request`]).
#[derive(Debug)]
#[must_use = "dropping the request cancels it"]
pub struct ResultRequest {
    ticket: ResultTicket,
    completion: Completion<ResultOutcome>,
}

impl ResultRequest {
    /// The request's token.
    #[must_use]
    pub fn token(&self) -> ResultToken {
        self.ticket.token()
    }

    /// The ticket, which the provider keeps while it wants the answer, and
    /// the completion.
    pub fn into_parts(self) -> (ResultTicket, Completion<ResultOutcome>) {
        (self.ticket, self.completion)
    }
}

/// What a host implements to offer its [`Activity`]. Owner thread only.
pub trait ActivityBridge: 'static {
    /// The mint the host makes what it hands providers with; the
    /// composition root's [`HostBridge::install_activity`] calls it once,
    /// before the Activity is offered.
    fn bind(&self, mint: ResultMint);

    /// Take a request of `capability` under `key`, whose `Intent` `intent`
    /// fills, and start it; see [`Activity::launch_for_result`].
    ///
    /// # Errors
    ///
    /// What [`Activity::launch_for_result`] refuses.
    fn launch_for_result(
        &self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
        intent: &mut dyn FnMut(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused>;

    /// See [`Activity::reattach`].
    ///
    /// # Errors
    ///
    /// What [`Activity::reattach`] refuses.
    fn reattach(
        &self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused>;

    /// Run `f` once with the `Intent` `data` names lent in the frame, and let
    /// it go, when `capability`'s request received it; see
    /// [`Activity::take_data`].
    ///
    /// # Errors
    ///
    /// What [`Activity::take_data`] refuses.
    fn take_data(
        &self,
        capability: &CapabilityId,
        data: ResultData,
        f: &mut dyn FnMut(JniFrame),
    ) -> Result<(), DataRefused>;

    /// Run `f` once with the Activity lent in the frame; `false`, and `f`
    /// does not run, when the Activity has ended or the thread cannot
    /// attach.
    fn lend(&self, f: &mut dyn FnMut(JniFrame)) -> bool;
}

/// The running Android Activity, as one capability's provider reaches it.
///
/// Owner thread only (`!Send`), cheap to clone. Its requests are keyed under
/// the capability that acquired it, so two capabilities' requests never
/// meet. It is valid while the Activity runs; once it ended, every request
/// is refused as [`ResultRefused::ActivityEnded`] and
/// [`with_java`](Self::with_java) answers `None`.
#[derive(Clone)]
pub struct Activity {
    bridge: Rc<dyn ActivityBridge>,
    capability: CapabilityId,
    name: ResultKey,
}

impl Activity {
    /// Start the activity of the `Intent` that `intent` fills, and hear its
    /// answer through the returned request's completion.
    ///
    /// `intent` runs once, inside this call, on the owner thread, with the
    /// Activity and a new, empty `Intent` lent in the frame; it fills the
    /// `Intent` and answers `true`, or answers `false` to give the request
    /// up. The activity starts on Android's main thread once the Activity is
    /// started: a request taken while it is stopped waits for its `onStart`.
    /// The completion settles at an owner turn with the answer, or with the
    /// [`ResultRefused`] that says why there is none; it settles once, and
    /// every request ends. Dropping the request's [`ResultTicket`] cancels
    /// it.
    ///
    /// `key` names the request's purpose within this capability: if the
    /// Activity ends before the answer, the request ends as
    /// [`ResultRefused::ActivityEnded`] and the answer, when it comes, waits
    /// for [`reattach`](Self::reattach) under `key` in the next Activity.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::TooManyOpen`], [`ResultRefused::TokensExhausted`],
    /// [`ResultRefused::IntentNotMade`] and, once the Activity ended,
    /// [`ResultRefused::ActivityEnded`]. A refusal takes nothing: `intent`
    /// does not run when the request is refused before it.
    pub fn launch_for_result(
        &self,
        completions: &CompletionSource,
        key: ResultKey,
        intent: impl FnOnce(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused> {
        let mut intent = Some(intent);
        self.bridge.launch_for_result(
            completions,
            &self.capability,
            RequestKey::new(self.name, key),
            &mut |frame| intent.take().is_some_and(|intent| intent(frame)),
        )
    }

    /// The next answer to a request an earlier Activity took under `key`:
    /// settled at the next owner turn when one is waiting, else when it
    /// arrives. Of several reattachments under one key, the oldest still
    /// held receives the oldest answer.
    ///
    /// # Errors
    ///
    /// [`ResultRefused::NothingPending`] when no earlier answer under `key`
    /// waits and none is still to come for another reattachment, so a
    /// provider that reattaches as it starts holds nothing when there is
    /// nothing to hear; [`ResultRefused::TokensExhausted`] and, once the
    /// Activity ended, [`ResultRefused::ActivityEnded`].
    pub fn reattach(
        &self,
        completions: &CompletionSource,
        key: ResultKey,
    ) -> Result<ResultRequest, ResultRefused> {
        self.bridge.reattach(
            completions,
            &self.capability,
            RequestKey::new(self.name, key),
        )
    }

    /// `f`'s answer, with the `Intent` `data` names lent in the frame, on the
    /// owner thread; the host lets the `Intent` go after. An `Intent` counts
    /// against the bounds until its [`ResultData`] is taken here or dropped.
    ///
    /// # Errors
    ///
    /// [`DataRefused::OtherCapability`] when `data` was handed to another
    /// capability's request, [`DataRefused::NotHeld`] when this Activity's
    /// host holds no `Intent` for it, and [`DataRefused::NotLent`]; `f` does
    /// not run.
    pub fn take_data<R>(
        &self,
        data: ResultData,
        f: impl FnOnce(JniFrame) -> R,
    ) -> Result<R, DataRefused> {
        let mut f = Some(f);
        let mut answer = None;
        self.bridge
            .take_data(&self.capability, data, &mut |frame| {
                if let Some(f) = f.take() {
                    answer = Some(f(frame));
                }
            })?;
        answer.ok_or(DataRefused::NotLent)
    }

    /// `f`'s answer, with the Activity lent in the frame on the owner thread:
    /// its content resolver, its permissions, its package. `None`, and `f`
    /// does not run, once the Activity has ended.
    pub fn with_java<R>(&self, f: impl FnOnce(JniFrame) -> R) -> Option<R> {
        let mut f = Some(f);
        let mut answer = None;
        self.bridge.lend(&mut |frame| {
            if let Some(f) = f.take() {
                answer = Some(f(frame));
            }
        });
        answer
    }
}

impl fmt::Debug for Activity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Activity")
            .field("capability", &self.capability)
            .finish_non_exhaustive()
    }
}

/// What the host hands a UI runtime's providers beyond the window: the
/// running Activity's bridge, while one runs (ADR-0154 §9), and the
/// application's id, when it has one (ADR-0351).
///
/// Owner-local and shared: the composition root installs what each run's
/// host offers, and a provider's context reads it at acquisition, scoped by
/// the capability the registrar minted for that provider.
#[derive(Clone, Default)]
pub struct HostBridge {
    activity: Rc<std::cell::RefCell<Option<Rc<dyn ActivityBridge>>>>,
    app_id: Rc<std::cell::RefCell<Option<flui_foundation::AppId>>>,
}

impl HostBridge {
    /// A bridge that offers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Name the application `app_id` to providers from now on, `None` for an
    /// application that declares none: the id a desktop portal and a shell
    /// know it by, the same one its windows carry.
    pub fn name_application(&self, app_id: Option<flui_foundation::AppId>) {
        drop(self.app_id.replace(app_id));
    }

    /// The application's id, as the composition root named it.
    pub(crate) fn app_id(&self) -> Option<flui_foundation::AppId> {
        self.app_id.borrow().clone()
    }

    /// Offer the Activity `activity` bridges from now on, `None` for none;
    /// a new bridge receives its [`ResultMint`] first. The one offered before
    /// is returned, for the caller to drop outside its own borrows.
    #[must_use = "the displaced Activity is dropped by the caller, outside its borrows"]
    pub fn install_activity(
        &self,
        activity: Option<Rc<dyn ActivityBridge>>,
    ) -> Option<Rc<dyn ActivityBridge>> {
        if let Some(bridge) = &activity {
            bridge.bind(ResultMint::new());
        }
        self.activity.replace(activity)
    }

    /// The Activity offered now, as the capability `capability` reaches it;
    /// `None` when none is offered, or when its name is not a [`ResultKey`],
    /// since its requests could not be keyed.
    pub(crate) fn activity(&self, capability: &CapabilityId) -> Option<Activity> {
        let name = ResultKey::try_new(capability.name()).ok()?;
        let bridge = self.activity.borrow().clone()?;
        Some(Activity {
            bridge,
            capability: capability.clone(),
            name,
        })
    }
}

impl fmt::Debug for HostBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostBridge")
            .field("activity", &self.activity.borrow().is_some())
            .field("app_id", &*self.app_id.borrow())
            .finish()
    }
}
