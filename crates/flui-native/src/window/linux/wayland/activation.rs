//! How a Wayland window asks to be activated (`xdg-activation`): the token
//! it asks the compositor for, what that token names, and when the window
//! hands it back.
//!
//! A client cannot focus its own window on Wayland. It asks the compositor
//! for a token (`xdg_activation_v1.get_activation_token`), names on it the
//! user input the request comes from (its seat and serial) and the surface
//! that input went to, commits it, and once the token comes (`done`) hands it
//! back with the surface to activate (`xdg_activation_v1.activate`). The
//! compositor decides from the token whether the window takes the focus: a
//! token that names a recent input of a focused surface may, one that names
//! no input asks for the user's attention instead (most compositors mark the
//! window urgent). So the window never takes the focus from another
//! application on its own, and its capability says `Emulated`.
//!
//! A token the launcher handed the process (`XDG_ACTIVATION_TOKEN`) is used
//! the same way, by the first window the process holds.
//!
//! A surface not mapped yet cannot be activated, so a token waits until the
//! compositor has shown the window once.

#![forbid(unsafe_code)]

use crate::owner::linux::wayland::activation::{ActivationToken, UserInput};

/// A request on a token object, after `get_activation_token` made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenRequest<Seat, Surface> {
    /// `set_serial`: the input the activation comes from.
    SetSerial {
        /// Its serial.
        serial: u32,
        /// Its seat.
        seat: Seat,
    },
    /// `set_surface`: the surface that input went to.
    SetSurface(Surface),
    /// `commit`: the compositor answers with the token.
    Commit,
}

/// The requests that make a token for the last user input, `input` (`None`
/// when the seats reported none), in the order the protocol takes them:
/// what the token names, then the commit. A seat or surface that no longer
/// exists (`seat_alive`, `surface_alive`) is not named: a serial is named
/// only with its seat. A token that names no input asks the compositor for
/// the user's attention rather than the focus.
pub(crate) fn token_requests<Seat, Surface>(
    input: Option<UserInput<Seat, Surface>>,
    seat_alive: impl Fn(&Seat) -> bool,
    surface_alive: impl Fn(&Surface) -> bool,
) -> Vec<TokenRequest<Seat, Surface>> {
    let mut requests = Vec::with_capacity(3);
    if let Some(UserInput {
        seat,
        serial,
        surface,
    }) = input
    {
        if seat_alive(&seat) {
            requests.push(TokenRequest::SetSerial { serial, seat });
        }
        if surface_alive(&surface) {
            requests.push(TokenRequest::SetSurface(surface));
        }
    }
    requests.push(TokenRequest::Commit);
    requests
}

/// The tokens a window holds and has not handed back: those the compositor
/// answered its requests with and the one its launcher gave, in the order
/// they came.
#[derive(Debug, Default)]
pub(crate) struct Tokens(Vec<ActivationToken>);

impl Tokens {
    /// A token came for the window.
    pub(crate) fn grant(&mut self, token: ActivationToken) {
        self.0.push(token);
    }

    /// The tokens the window hands back now, each once and in the order
    /// they came: every one held once the compositor has shown the window
    /// (`mapped`), none before.
    pub(crate) fn take_due(&mut self, mapped: bool) -> Vec<ActivationToken> {
        if mapped {
            std::mem::take(&mut self.0)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seat or surface by a name of the test's, and whether it still
    /// exists.
    type Seat = (&'static str, bool);
    type Surface = (&'static str, bool);

    fn requests(input: Option<UserInput<Seat, Surface>>) -> Vec<TokenRequest<Seat, Surface>> {
        token_requests(input, |seat| seat.1, |surface| surface.1)
    }

    fn token(name: &str) -> ActivationToken {
        ActivationToken::new(name.to_owned())
    }

    /// The token names the last input: its serial with its seat, then the
    /// surface it went to, and the commit comes last, as the protocol
    /// requires; without an input the token names nothing and asks for
    /// attention.
    #[test]
    fn a_token_names_the_last_input_before_its_commit() {
        let seat = ("seat0", true);
        let surface = ("focused", true);
        assert_eq!(
            requests(Some(UserInput {
                seat,
                serial: 42,
                surface,
            })),
            vec![
                TokenRequest::SetSerial { serial: 42, seat },
                TokenRequest::SetSurface(surface),
                TokenRequest::Commit,
            ]
        );
        assert_eq!(requests(None), vec![TokenRequest::Commit], "no input");
    }

    /// A seat the compositor removed takes its serial with it, and a surface
    /// that closed is not named: the token names what still exists.
    #[test]
    fn a_token_names_no_seat_or_surface_that_is_gone() {
        let input = |seat, surface| {
            Some(UserInput {
                seat,
                serial: 7,
                surface,
            })
        };
        assert_eq!(
            requests(input(("unplugged", false), ("focused", true))),
            vec![
                TokenRequest::SetSurface(("focused", true)),
                TokenRequest::Commit
            ]
        );
        assert_eq!(
            requests(input(("seat0", true), ("closed", false))),
            vec![
                TokenRequest::SetSerial {
                    serial: 7,
                    seat: ("seat0", true)
                },
                TokenRequest::Commit
            ]
        );
    }

    /// A token waits until the window was shown, then each is handed back
    /// once, in the order they came; one that comes later waits for the
    /// next turn and is not lost.
    #[test]
    fn a_token_is_handed_back_once_the_window_was_shown() {
        let mut tokens = Tokens::default();
        tokens.grant(token("launcher"));
        tokens.grant(token("asked"));
        assert!(tokens.take_due(false).is_empty(), "not mapped yet");
        assert_eq!(tokens.take_due(true), [token("launcher"), token("asked")]);
        assert!(tokens.take_due(true).is_empty(), "each is used once");
        tokens.grant(token("later"));
        assert_eq!(tokens.take_due(true), [token("later")]);
    }
}
