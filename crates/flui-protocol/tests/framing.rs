//! How a byte transport frames the protocol (ADR-0238 §2): lines of at most a reader's limit,
//! and binary payloads behind a header of their id and length, each a little-endian `u64`,
//! refused when it announces more than the reader takes.

use flui_protocol::{BlobHeader, BlobId, FramingError};

#[test]
fn a_payload_header_round_trips_and_refuses_what_the_reader_does_not_take() {
    let header = BlobHeader::new(BlobId::new(3), 0x0102_0304);
    let bytes = header.encode();
    assert_eq!(bytes.len(), BlobHeader::SIZE);
    assert_eq!(
        &bytes[..8],
        &3_u64.to_le_bytes(),
        "the id comes first, little-endian"
    );
    assert_eq!(
        &bytes[8..],
        &0x0102_0304_u64.to_le_bytes(),
        "then the length"
    );

    let table: &[(&str, BlobHeader, u64, Result<BlobHeader, FramingError>)] = &[
        ("within the limit", header, 1 << 30, Ok(header)),
        ("exactly the limit", header, 0x0102_0304, Ok(header)),
        (
            "one byte over the limit",
            header,
            0x0102_0303,
            Err(FramingError::PayloadTooLong {
                len: 0x0102_0304,
                max_len: 0x0102_0303,
            }),
        ),
        (
            "a length no reader could allocate",
            BlobHeader::new(BlobId::new(1), u64::MAX),
            1 << 30,
            Err(FramingError::PayloadTooLong {
                len: u64::MAX,
                max_len: 1 << 30,
            }),
        ),
        (
            "payload 0, which no sender uses",
            BlobHeader::new(BlobId::new(0), 4),
            1 << 30,
            Err(FramingError::ZeroId),
        ),
    ];
    for (case, header, max_len, expected) in table {
        assert_eq!(
            BlobHeader::decode(&header.encode(), *max_len),
            *expected,
            "{case}"
        );
    }
}

#[test]
fn the_exit_on_disconnect_switch_is_exactly_one() {
    for (value, on) in [
        ("1", true),
        ("0", false),
        ("", false),
        ("true", false),
        (" 1", false),
    ] {
        assert_eq!(
            flui_protocol::launch::exit_on_disconnect(value),
            on,
            "{value:?}"
        );
    }
    assert_ne!(flui_protocol::launch::EXIT_DISCONNECTED, 0);
    assert_ne!(flui_protocol::launch::EXIT_DISCONNECTED, 101);
}

/// A case's name, the pushes, the lines taken after the last one, and whether the reader
/// refused a line as too long.
type Case<'a> = (&'a str, &'a [&'a [u8]], &'a [&'a [u8]], bool);

#[test]
fn a_line_reader_takes_whole_lines_however_the_bytes_arrive() {
    // Every reader here takes lines of at most 8 bytes.
    let table: &[Case<'_>] = &[
        ("one line", &[b"abc\n"], &[b"abc"], false),
        ("two in one read", &[b"a\nb\n"], &[b"a", b"b"], false),
        ("one over two reads", &[b"ab", b"c\n"], &[b"abc"], false),
        (
            "a carriage return is dropped",
            &[b"abc\r\n"],
            &[b"abc"],
            false,
        ),
        ("an empty line", &[b"\n"], &[b""], false),
        ("a line not yet ended", &[b"abc"], &[], false),
        ("exactly the limit", &[b"12345678\n"], &[b"12345678"], false),
        ("over the limit, ended", &[b"123456789\n"], &[], true),
        (
            "over the limit, not yet ended",
            &[b"1234", b"56789"],
            &[],
            true,
        ),
        (
            "a line before a long one",
            &[b"ok\n123456789012"],
            &[b"ok"],
            true,
        ),
    ];
    for (case, pushes, lines, too_long) in table {
        let mut reader = flui_protocol::LineReader::new(8);
        let mut taken = Vec::new();
        let mut refused = false;
        for push in *pushes {
            reader.push(push);
            loop {
                match reader.next_line() {
                    Ok(Some(line)) => taken.push(line),
                    Ok(None) => break,
                    Err(error) => {
                        assert_eq!(error, FramingError::LineTooLong { max_len: 8 }, "{case}");
                        refused = true;
                        break;
                    }
                }
            }
        }
        let expected: Vec<Vec<u8>> = lines.iter().map(|line| line.to_vec()).collect();
        assert_eq!(taken, expected, "{case}: lines");
        assert_eq!(refused, *too_long, "{case}: refused as too long");
    }
}
