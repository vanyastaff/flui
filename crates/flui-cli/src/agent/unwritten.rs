//! What an endpoint this command serves still owes one client: the relay's
//! ([`super::browser::relay`]) and the run endpoint's ([`super::supervisor`]).

use std::io;

/// What an endpoint still owes one client, written from the front as the client takes it. A
/// write advances an offset rather than moving what is left, so a reply a Windows pipe takes
/// [`flui_protocol::endpoint::PIPE_WRITE_CHUNK`] bytes at a time costs work linear in its
/// length. The written front is let go once it is at least as long as what follows it, so what
/// the buffer holds stays within twice what is owed (plus what is being added), whatever the
/// client's pace, and each byte moves at most once for every byte written before it.
#[derive(Debug, Default)]
pub(crate) struct Unwritten {
    bytes: Vec<u8>,
    /// How much of `bytes` is written.
    sent: usize,
}

impl Unwritten {
    /// How many bytes are still owed.
    pub(crate) fn len(&self) -> usize {
        self.bytes.len() - self.sent
    }

    /// Whether nothing is owed.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Owes `bytes` after what is owed already, letting go of the written front first once it
    /// is at least as long as what is owed.
    pub(crate) fn extend_from_slice(&mut self, bytes: &[u8]) {
        if self.sent > 0 && self.sent >= self.len() {
            self.bytes.drain(..self.sent);
            self.sent = 0;
        }
        self.bytes.extend_from_slice(bytes);
    }

    /// How many bytes the buffer holds, the written front included.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.bytes.len()
    }

    /// Owes nothing more.
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.sent = 0;
    }

    /// Writes what `stream` takes: `true` once all of it is written, `false` when the stream
    /// has no room.
    ///
    /// # Errors
    ///
    /// The write failed, or the stream took nothing and said so (`WriteZero`).
    pub(crate) fn write_to(&mut self, mut stream: impl io::Write) -> io::Result<bool> {
        while self.sent < self.bytes.len() {
            match stream.write(&self.bytes[self.sent..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(written) => self.sent += written,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.clear();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use flui_protocol::endpoint::PIPE_WRITE_CHUNK;
    use flui_protocol::testing::{PipeAnswer, ScriptedPipe};

    use super::Unwritten;

    /// A client that takes most of each reply but never catches up is owed its bytes in order,
    /// and the buffer holds at most twice what is owed after each reply is added. Fails if the
    /// written front is kept until the client catches up, which a client that stays behind never
    /// does, so the buffer grows for the life of the connection.
    #[test]
    fn what_a_client_that_stays_behind_is_owed_stays_bounded_and_in_order() {
        const REPLY: usize = 2 * PIPE_WRITE_CHUNK;
        let mut owed = Unwritten::default();
        let mut taken = Vec::new();
        let mut sent = Vec::new();
        for round in 0..256_usize {
            let reply = (0..REPLY)
                .map(|i| u8::try_from((round * 7 + i) % 251).expect("BUG: below 251"))
                .collect::<Vec<u8>>();
            owed.extend_from_slice(&reply);
            sent.extend_from_slice(&reply);
            assert!(
                owed.held() <= 2 * owed.len(),
                "round {round}: {} bytes held for {} owed",
                owed.held(),
                owed.len()
            );
            // The client takes all of each reply but a few bytes, a pipe's chunk at a time.
            let client = ScriptedPipe::new(
                [],
                [
                    PipeAnswer::Bytes(PIPE_WRITE_CHUNK),
                    PipeAnswer::Bytes(PIPE_WRITE_CHUNK - 24),
                    PipeAnswer::WouldBlock,
                ],
            );
            assert!(!owed.write_to(&client).expect("a write"), "round {round}");
            assert!(
                client.is_spent(),
                "round {round}: every scripted write is made"
            );
            taken.extend_from_slice(&client.written());
        }
        let client = ScriptedPipe::new([], [PipeAnswer::Bytes(usize::MAX)]);
        assert!(
            owed.write_to(&client).expect("a write"),
            "all of it is written"
        );
        taken.extend_from_slice(&client.written());
        assert!(owed.is_empty());
        assert_eq!(taken, sent, "written whole and in order");
    }

    /// While the written front is shorter than what follows it, what is added is appended and
    /// nothing moves. Fails if the front is let go on every addition once anything is written,
    /// which moves a large backlog for each small reply, so a client that takes a little at a
    /// time costs work quadratic in what it is owed.
    #[test]
    fn a_short_written_front_is_kept_while_a_long_backlog_follows_it() {
        let mut owed = Unwritten::default();
        owed.extend_from_slice(&[7; 1000]);
        let client = ScriptedPipe::new([], [PipeAnswer::Bytes(10), PipeAnswer::WouldBlock]);
        assert!(!owed.write_to(&client).expect("a write"));
        assert_eq!(owed.len(), 990);
        for added in 1..=3_usize {
            owed.extend_from_slice(&[9; 5]);
            assert_eq!(
                owed.held(),
                1000 + 5 * added,
                "addition {added}: the written front was let go"
            );
            assert_eq!(owed.len(), 990 + 5 * added);
        }
    }

    /// A write that takes nothing is `WriteZero`, an interrupted one is made again, and a failed
    /// one is handed on with what was written kept. Fails if one of the three answers is taken
    /// for another.
    #[test]
    fn what_is_owed_is_written_as_the_stream_answers() {
        let line = b"0123456789";
        let written_as = |answers: Vec<PipeAnswer>| {
            let mut owed = Unwritten::default();
            owed.extend_from_slice(line);
            let pipe = ScriptedPipe::new([], answers);
            let outcome = owed.write_to(&pipe).map_err(|error| error.kind());
            assert!(pipe.is_spent(), "every scripted write is made");
            (outcome, owed.len())
        };
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(0)]),
            (Err(io::ErrorKind::WriteZero), line.len()),
            "a write that takes nothing"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Interrupted, PipeAnswer::Bytes(line.len())]),
            (Ok(true), 0),
            "an interrupted write, made again"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(4), PipeAnswer::Closed]),
            (Err(io::ErrorKind::BrokenPipe), line.len() - 4),
            "a failed write after a partial one"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(4), PipeAnswer::WouldBlock]),
            (Ok(false), line.len() - 4),
            "no room after a partial write"
        );
    }
}
