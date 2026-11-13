//! The fuzz target of the development agent's wire (ADR-0288): `cargo xtask fuzz agent_wire`
//! builds it under `--cfg fuzzing` and libFuzzer mutates what a client writes to a session
//! toward code no input has reached yet, starting from `fuzz/seeds/agent_wire/`. Each input is
//! one conversation with a service whose window shows a counter. A panic, an abort, a line
//! that is not a line of the protocol or an answer nobody asked for ends the run and leaves
//! the input that did it.
//!
//! Built without that flag it holds the conversation of each file named on its command line,
//! which is how a crash input is replayed on the pinned toolchain:
//! `cargo run -p flui-runtime --example fuzz_agent_wire -- <file>`.

#![cfg_attr(fuzzing, no_main)]

#[path = "../tests/support/agent_wire.rs"]
mod agent_wire;

/// The byte that ends a chunk: no JSON text holds it, so a seed stays readable and the
/// fuzzer still decides where the transport cuts a line.
const CHUNK_END: u8 = 0xFF;

/// What an input writes where the window's handle goes. A handle is issued per conversation,
/// so the input names the window by this and the conversation fills it in.
const WINDOW: &[u8] = b"$window";

/// Holds the conversation `input` writes: its first byte says whether the client greets
/// first (an odd byte, so `1` in a seed) and the rest is the chunks its transport delivers.
fn converse(input: &[u8]) {
    let Some((greet, written)) = input.split_first() else {
        return;
    };
    agent_wire::client_bytes_are_answered_or_refused_in_whole_lines(greet % 2 == 1, |window| {
        let written = match window {
            Some(window) => naming(written, window.as_bytes()),
            None => written.to_vec(),
        };
        written
            .split(|byte| *byte == CHUNK_END)
            .map(<[u8]>::to_vec)
            .collect()
    });
}

/// `written` with `window` in place of each [`WINDOW`].
fn naming(written: &[u8], window: &[u8]) -> Vec<u8> {
    let mut named = Vec::with_capacity(written.len());
    let mut rest = written;
    while let Some(at) = rest.windows(WINDOW.len()).position(|bytes| bytes == WINDOW) {
        let (before, from) = rest.split_at(at);
        named.extend_from_slice(before);
        named.extend_from_slice(window);
        rest = from.get(WINDOW.len()..).unwrap_or_default();
    }
    named.extend_from_slice(rest);
    named
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|input: &[u8]| converse(input));

#[cfg(not(fuzzing))]
fn main() -> std::io::Result<()> {
    for path in std::env::args_os().skip(1) {
        converse(&std::fs::read(path)?);
    }
    Ok(())
}
