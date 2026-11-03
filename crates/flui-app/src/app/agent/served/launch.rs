//! What the launch tells the agent: where to listen, the token a driver
//! presents, and whether to exit when the driver goes (ADR-0239 §3, §5). The
//! values are read through the platform's one seam (ADR-0288 §7): the
//! environment on the desktop and the iOS Simulator, the launching intent's
//! extras on Android.

use flui_platform::LaunchKey;
use flui_runtime::agent::Limits;
use std::fmt;

use flui_protocol::launch::{
    ENDPOINT_EXTRA, EXIT_ON_DISCONNECT_EXTRA, MIN_TOKEN_LEN, TOKEN_EXTRA, exit_on_disconnect,
};

/// The key the agent's endpoint is launched under.
pub(crate) const ENDPOINT: LaunchKey = launch_key(ENDPOINT_EXTRA);
/// The key the token is launched under.
pub(crate) const TOKEN: LaunchKey = launch_key(TOKEN_EXTRA);
/// The key that, `1`, makes the driver's going end the app.
pub(crate) const EXIT_ON_DISCONNECT: LaunchKey = launch_key(EXIT_ON_DISCONNECT_EXTRA);

const fn launch_key(key: &'static str) -> LaunchKey {
    LaunchKey::try_new(key).expect("BUG: a protocol launch key keeps LaunchKey's rules")
}

/// Which peers may hold a connection, by their effective user ID.
pub(super) type PeerRule = fn(u32) -> bool;

/// Where the agent listens, the token a driver presents, and what a
/// driver's disconnect does.
#[derive(Clone)]
pub(crate) struct Launch {
    pub(super) address: String,
    pub(super) token: String,
    exit_on_disconnect: bool,
    pub(super) limits: Limits,
    /// `Some` for an abstract socket, which any process of the system reaches: only the peers
    /// it admits hold a connection. `None` where the system keeps the endpoint the current
    /// user's (a pipe's DACL, a socket file in a private directory).
    pub(super) peers: Option<PeerRule>,
}

impl Launch {
    /// The launch `value` reads, now: the platform's
    /// [`launch_value`](flui_native::Platform::launch_value) in production.
    /// `None`, and nothing created, when it names no endpoint; `None` with one
    /// warning when the token is missing or shorter than [`MIN_TOKEN_LEN`]
    /// bytes.
    pub(crate) fn read(value: impl Fn(LaunchKey) -> Option<String>) -> Option<Self> {
        let address = value(ENDPOINT)?;
        let exit = value(EXIT_ON_DISCONNECT).is_some_and(|value| exit_on_disconnect(&value));
        Self::try_new(address, value(TOKEN).unwrap_or_default(), exit)
    }

    /// A launch at `address` admitting `token`; `None`, with one warning,
    /// when the token is shorter than [`MIN_TOKEN_LEN`] bytes.
    ///
    /// On Linux and Android an address `@name` names the abstract socket `name`; elsewhere,
    /// and without the `@`, an address is a pipe's name or a socket file's path.
    pub(crate) fn try_new(
        address: String,
        token: String,
        exit_on_disconnect: bool,
    ) -> Option<Self> {
        if token.len() < MIN_TOKEN_LEN {
            tracing::warn!(
                min_len = MIN_TOKEN_LEN,
                "the driver agent's token is missing or too short; the app runs without it"
            );
            return None;
        }
        let peers = abstract_name(&address).map(|_| same_user_or_system as PeerRule);
        Some(Self {
            address,
            token,
            exit_on_disconnect,
            limits: Limits::default(),
            peers,
        })
    }

    /// With `limits` in place of the defaults.
    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// With `rule` admitting the peers of an abstract socket in place of
    /// [`same_user_or_system`].
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn with_peer_rule(mut self, rule: PeerRule) -> Self {
        if self.peers.is_some() {
            self.peers = Some(rule);
        }
        self
    }

    /// Whether the app exits when its driver's connection closes.
    pub(crate) fn exit_on_disconnect(&self) -> bool {
        self.exit_on_disconnect
    }
}

impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launch")
            .field("address", &self.address)
            .field("token", &"<redacted>")
            .field("exit_on_disconnect", &self.exit_on_disconnect)
            .field("limits", &self.limits)
            .field("abstract", &self.peers.is_some())
            .finish()
    }
}

/// The abstract socket's name `address` names (`@name`), on the systems that have them.
pub(super) fn abstract_name(address: &str) -> Option<&str> {
    if cfg!(any(target_os = "linux", target_os = "android")) {
        address.strip_prefix('@')
    } else {
        None
    }
}

/// Whether an abstract socket admits `peer`, by its effective user ID (ADR-0288 §6): the app's
/// own user, root, and on Android the shell, which `adb forward` connects as. Any process of
/// the system reaches an abstract socket, and on Android any other app could start this one
/// with an endpoint and token of its choosing; the rule keeps both out.
pub(super) fn same_user_or_system(peer: u32) -> bool {
    peer_admitted(peer, current_user(), cfg!(target_os = "android"))
}

/// [`same_user_or_system`]'s rule for `own`, the app's user, on Android or not.
pub(super) fn peer_admitted(peer: u32, own: u32, android: bool) -> bool {
    /// Android's `AID_SHELL`.
    const SHELL: u32 = 2000;
    /// Root.
    const ROOT: u32 = 0;
    peer == own || peer == ROOT || (android && peer == SHELL)
}

/// The app's effective user ID, where abstract sockets exist.
fn current_user() -> u32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        flui_native::current_user_id()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        // No abstract socket is bound here, so no peer is checked against it.
        u32::MAX
    }
}
