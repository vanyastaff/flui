//! How a byte transport frames a binary payload (ADR-0238 §2): a header of the payload's id and
//! length, each a little-endian `u64`, refused when it announces more than the reader takes.

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
