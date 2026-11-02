//! The input context's rules: the key hold, the daemon's choice and the
//! protocols' values.

use std::path::Path;

use web_time::Instant;

use super::{
    ANSWER_TIMEOUT, ContentType, Daemon, Endpoint, KeyHold, MAX_HELD_KEYS, Told, byte_of_char,
    chars_before, deletion, endpoints, fcitx_capability, ibus_address, ibus_address_file,
    ibus_content, preedit_caret,
};

/// The variables set, whether the process is sandboxed, and the daemon to
/// ask first.
type EnvironmentCase<'a> = (&'a [(&'a str, &'a str)], bool, Daemon);

/// What the input method was told, its deletion, and the bytes deleted.
type DeletionCase = (Told, i32, u32, Option<(u32, u32)>);

#[test]
fn keys_behind_an_unanswered_key_wait_in_order() {
    let now = Instant::now();
    let mut hold = KeyHold::default();
    let first = hold.ask('a', now, |_| {});
    hold.pass('b');
    let third = hold.ask('c', now, |_| {});
    assert!(
        hold.ready().is_empty(),
        "nothing passes the first key's wait"
    );
    assert!(hold.answer(third, false));
    assert!(
        hold.ready().is_empty(),
        "an answer behind the wait delivers nothing yet"
    );
    assert!(hold.answer(first, false));
    assert_eq!(hold.ready(), ['a', 'b', 'c']);
    assert_eq!(hold.deadline(), None, "nothing is held");
}

#[test]
fn a_key_the_input_method_took_is_not_delivered() {
    let now = Instant::now();
    let mut hold = KeyHold::default();
    let taken = hold.ask('a', now, |_| {});
    hold.pass('b');
    assert!(hold.answer(taken, true));
    assert_eq!(hold.ready(), ['b']);
}

#[test]
fn an_unanswered_key_is_delivered_at_its_deadline_and_its_late_answer_dropped() {
    let now = Instant::now();
    let mut hold = KeyHold::default();
    let slow = hold.ask('a', now, |_| {});
    let next = hold.ask('b', now + ANSWER_TIMEOUT / 2, |_| {});
    assert_eq!(hold.deadline(), Some(now + ANSWER_TIMEOUT));
    hold.expire(now + ANSWER_TIMEOUT / 2, |key| panic!("{key} is not due"));
    assert_eq!(hold.ready(), Vec::<char>::new(), "not due yet");
    let mut overdue = Vec::new();
    hold.expire(now + ANSWER_TIMEOUT, |key| overdue.push(*key));
    assert_eq!(overdue, ['a'], "the overdue key is told");
    assert_eq!(
        hold.ready(),
        ['a'],
        "the overdue key goes, the next still waits"
    );
    assert!(
        !hold.answer(slow, true),
        "an answer after the deadline names no waiting key"
    );
    assert!(hold.answer(next, false));
    assert_eq!(hold.ready(), ['b']);
}

#[test]
fn keys_of_a_context_that_went_are_delivered_in_order() {
    let now = Instant::now();
    let mut hold = KeyHold::default();
    hold.ask(('x', 1), now, |_| {});
    hold.ask(('y', 2), now, |_| {});
    hold.ask(('x', 3), now, |_| {});
    hold.release_where(|(window, _)| *window == 'x');
    assert_eq!(
        hold.ready(),
        [('x', 1)],
        "the other window's key still waits"
    );
    hold.release_where(|_| true);
    assert_eq!(hold.ready(), [('y', 2), ('x', 3)]);
}

#[test]
fn a_full_hold_lets_its_oldest_waiting_key_go() {
    let now = Instant::now();
    let mut hold = KeyHold::default();
    for key in 0..MAX_HELD_KEYS {
        hold.ask(key, now, |_| {});
    }
    let mut evicted = Vec::new();
    hold.ask(MAX_HELD_KEYS, now, |key| evicted.push(*key));
    assert_eq!(evicted, [0], "the input method is told which key went");
    assert_eq!(hold.ready(), [0], "the oldest key no longer waits");
}

#[test]
fn the_environment_names_the_input_method_to_ask_first() {
    let cases: &[EnvironmentCase<'_>] = &[
        (&[], false, Daemon::IBus),
        (&[("XMODIFIERS", "@im=fcitx")], false, Daemon::Fcitx5),
        (&[("XMODIFIERS", "@im=ibus")], false, Daemon::IBus),
        (
            &[("XMODIFIERS", "@im=ibus"), ("GTK_IM_MODULE", "fcitx")],
            false,
            Daemon::IBus,
        ),
        (&[("GTK_IM_MODULE", "fcitx5")], false, Daemon::Fcitx5),
        (&[("QT_IM_MODULE", "fcitx")], true, Daemon::Fcitx5),
        (
            &[("XMODIFIERS", "@im=none"), ("QT_IM_MODULE", "ibus")],
            false,
            Daemon::IBus,
        ),
    ];
    for (vars, sandboxed, first) in cases {
        let var = |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        };
        let order = endpoints(var, *sandboxed);
        assert_eq!(
            order[0],
            Endpoint {
                daemon: *first,
                portal: *sandboxed
            },
            "{vars:?}"
        );
        assert_ne!(order[0].daemon, order[1].daemon, "{vars:?}");
    }
}

#[test]
fn the_ibus_address_is_read_where_ibus_writes_it_for_the_display() {
    let config = Path::new("/home/u/.config");
    let file = |display: Option<&str>, file: Option<&str>| {
        let var = |name: &str| match name {
            "DISPLAY" => display.map(str::to_owned),
            "IBUS_ADDRESS_FILE" => file.map(str::to_owned),
            _ => None,
        };
        ibus_address_file(var, "abc\n", config)
    };
    assert_eq!(file(Some(":1"), None), config.join("ibus/bus/abc-unix-1"));
    assert_eq!(
        file(Some("host:2.0"), None),
        config.join("ibus/bus/abc-host-2")
    );
    assert_eq!(file(None, None), config.join("ibus/bus/abc-unix-0"));
    assert_eq!(file(Some(":1"), Some("/f")), Path::new("/f"));
    assert_eq!(
        ibus_address("# comment\nIBUS_ADDRESS=unix:path=/tmp/x,guid=1\nIBUS_DAEMON_PID=7\n")
            .as_deref(),
        Some("unix:path=/tmp/x,guid=1")
    );
    assert_eq!(ibus_address("IBUS_ADDRESS=\n"), None);
}

#[test]
fn a_field_states_its_purpose_in_each_protocol() {
    let password = ContentType {
        hint: ContentType::HINT_HIDDEN_TEXT | ContentType::HINT_SENSITIVE_DATA,
        purpose: ContentType::PASSWORD,
    };
    assert_eq!(ibus_content(password), (8, (1 << 11) | (1 << 12)));
    let notes = ContentType {
        hint: ContentType::HINT_MULTILINE,
        purpose: ContentType::NORMAL,
    };
    assert_eq!(ibus_content(notes), (0, 1 << 14));
    let email = ContentType {
        hint: 0,
        purpose: ContentType::EMAIL,
    };
    assert_eq!(ibus_content(email), (6, 0));
    let base = (1 << 1) | (1 << 4) | (1 << 6);
    assert_eq!(fcitx_capability(email), base | (1 << 7));
    assert_eq!(
        fcitx_capability(password),
        base | (1 << 3) | (1 << 36),
        "a protected field is a password and sensitive"
    );
    assert_eq!(fcitx_capability(notes), base | (1 << 35));
}

#[test]
fn offsets_count_characters_where_the_daemons_do() {
    let text = "aé中😀b";
    assert_eq!(chars_before(text, 0), 0);
    assert_eq!(chars_before(text, 3), 2);
    assert_eq!(
        chars_before(text, 2),
        2,
        "a byte inside a character counts it"
    );
    assert_eq!(chars_before(text, text.len()), 5);
    assert_eq!(byte_of_char(text, 2), 3);
    assert_eq!(byte_of_char(text, 4), 10);
    assert_eq!(byte_of_char(text, 9), text.len());
    assert_eq!(preedit_caret("中文", 1), 3);
}

#[test]
fn a_deletion_in_characters_is_told_as_bytes_around_the_selection() {
    let told = |text: &str, cursor: usize, anchor: usize| Told {
        text: text.to_owned(),
        cursor,
        anchor,
    };
    let cases: &[DeletionCase] = &[
        // Two characters before the caret: "é中" is five bytes.
        (told("aé中b", 6, 6), -2, 2, Some((5, 0))),
        // One after it.
        (told("aé中b", 6, 6), 0, 1, Some((0, 1))),
        // Across it.
        (told("aé中b", 3, 3), -1, 2, Some((2, 3))),
        // A selection is kept: the span reaches both its ends.
        (told("abcd", 3, 1), -3, 4, Some((1, 1))),
        // Wholly before the caret, not touching it.
        (told("abcd", 4, 4), -3, 1, None),
        // Past the text's start.
        (told("ab", 1, 1), -2, 2, None),
        // Past its end: what is there is deleted.
        (told("ab", 1, 1), 0, 9, Some((0, 1))),
    ];
    for (told, offset, chars, expected) in cases {
        assert_eq!(
            deletion(told, *offset, *chars),
            *expected,
            "{told:?} {offset} {chars}"
        );
    }
}
