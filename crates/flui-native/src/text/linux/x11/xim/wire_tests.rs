use super::*;

/// One of each message this client reads or writes.
fn every_message() -> Vec<Message> {
    let event = {
        let mut event = [0u8; 32];
        event[0] = KEY_PRESS;
        event[1] = 38;
        event
    };
    let decl = |id: u16, name: &[u8]| AttrDecl {
        id,
        kind: 3,
        name: name.to_vec(),
    };
    let value = |id: u16, value: &[u8]| AttrValue {
        id,
        value: value.to_vec(),
    };
    vec![
        Message::Connect,
        Message::ConnectReply,
        Message::Disconnect,
        Message::DisconnectReply,
        Message::Error {
            im: 1,
            ic: 2,
            flag: ERROR_IM_VALID | ERROR_IC_VALID,
            code: 999,
        },
        Message::Open {
            locale: b"en_US".to_vec(),
        },
        Message::OpenReply {
            im: 1,
            im_attrs: vec![decl(0, b"queryInputStyle")],
            ic_attrs: vec![decl(0, b"inputStyle"), decl(1, b"clientWindow")],
        },
        Message::Close { im: 1 },
        Message::CloseReply { im: 1 },
        Message::SetEventMask {
            im: 1,
            ic: 2,
            forward: 3,
            synchronous: 0,
        },
        Message::EncodingNegotiation { im: 1 },
        Message::EncodingNegotiationReply { im: 1, index: 0 },
        Message::GetImValues {
            im: 1,
            ids: vec![0],
        },
        Message::GetImValuesReply {
            im: 1,
            values: vec![value(0, &[1, 0, 0, 0, 2, 4, 0, 0])],
        },
        Message::CreateIc {
            im: 1,
            values: vec![value(0, &[2, 4, 0, 0]), value(1, &[7, 0, 0, 0])],
        },
        Message::CreateIcReply { im: 1, ic: 2 },
        Message::DestroyIc { im: 1, ic: 2 },
        Message::DestroyIcReply { im: 1, ic: 2 },
        Message::SetIcValues {
            im: 1,
            ic: 2,
            values: vec![value(4, &[5, 0, 6, 0, 0, 0, 0, 0])],
        },
        Message::SetIcValuesReply { im: 1, ic: 2 },
        Message::SetIcFocus { im: 1, ic: 2 },
        Message::UnsetIcFocus { im: 1, ic: 2 },
        Message::ForwardEvent {
            im: 1,
            ic: 2,
            flag: FORWARD_SYNCHRONOUS,
            serial: 9,
            event,
        },
        Message::Sync { im: 1, ic: 2 },
        Message::SyncReply { im: 1, ic: 2 },
        Message::Commit {
            im: 1,
            ic: 2,
            synchronous: true,
            keysym: None,
            text: Some(b"abc".to_vec()),
        },
        Message::Commit {
            im: 1,
            ic: 2,
            synchronous: false,
            keysym: Some(0x61),
            text: Some(b"a".to_vec()),
        },
        Message::ResetIc { im: 1, ic: 2 },
        Message::ResetIcReply {
            im: 1,
            ic: 2,
            text: b"xy".to_vec(),
        },
        Message::PreeditStart { im: 1, ic: 2 },
        Message::PreeditStartReply {
            im: 1,
            ic: 2,
            limit: -1,
        },
        Message::PreeditDraw {
            im: 1,
            ic: 2,
            caret: 1,
            first: 0,
            length: 0,
            status: 0,
            text: b"ni".to_vec(),
        },
        Message::PreeditCaret {
            im: 1,
            ic: 2,
            position: 1,
            direction: 0,
            style: 0,
        },
        Message::PreeditCaretReply {
            im: 1,
            ic: 2,
            position: 1,
        },
        Message::PreeditDone { im: 1, ic: 2 },
    ]
}

#[test]
fn every_message_reads_as_it_was_written() {
    for message in every_message() {
        let bytes = message.encode();
        assert_eq!(bytes.len() % 4, 0, "{message:?} is padded to four bytes");
        assert_eq!(
            usize::from(u16::from_ne_bytes([bytes[2], bytes[3]])) * 4 + 4,
            bytes.len(),
            "{message:?} counts its own body"
        );
        assert_eq!(Message::decode(&bytes), Ok(message.clone()));
    }
}

/// A message cut anywhere reads as truncated, never past its bytes.
#[test]
fn a_message_cut_short_is_refused() {
    for message in every_message() {
        let bytes = message.encode();
        for len in 0..bytes.len() {
            let cut = &bytes[..len];
            if len < 4 {
                assert!(
                    Message::decode(cut).is_err(),
                    "{message:?} cut to {len} bytes"
                );
            } else {
                // A header that still claims the whole body is refused.
                assert_eq!(
                    Message::decode(cut),
                    Err(WireError::Truncated),
                    "{message:?} cut to {len} bytes"
                );
            }
        }
    }
}

/// A list whose byte length runs past the message, which the generated
/// parser of the `xim` crates subtracts without a check, is refused.
#[test]
fn a_list_longer_than_its_message_is_refused() {
    // XIM_OPEN_REPLY: an input method id, then a list of 0xffff bytes in a
    // body of eight.
    let mut bytes = vec![31, 0];
    bytes.extend_from_slice(&2u16.to_ne_bytes());
    bytes.extend_from_slice(&1u16.to_ne_bytes());
    bytes.extend_from_slice(&u16::MAX.to_ne_bytes());
    bytes.extend_from_slice(&[0; 4]);
    assert_eq!(Message::decode(&bytes), Err(WireError::Truncated));
}

#[test]
fn a_message_longer_than_any_read_is_refused() {
    let bytes = vec![0; MAX_MESSAGE + 1];
    assert_eq!(Message::decode(&bytes), Err(WireError::TooLong));
}

/// Bytes from nowhere decode or are refused, without a panic.
#[test]
fn any_bytes_decode_or_are_refused() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let majors = [
        2, 20, 31, 37, 39, 45, 51, 55, 60, 61, 62, 63, 65, 73, 75, 76, 78,
    ];
    for _ in 0..20_000 {
        let len = usize::try_from(next() % 96).expect("BUG: under 96");
        let mut bytes: Vec<u8> = (0..len).map(|_| next().to_le_bytes()[0]).collect();
        if let Some(first) = bytes.first_mut() {
            // Mostly opcodes this client reads.
            *first = majors[usize::try_from(next()).unwrap_or(0) % majors.len()];
        }
        if bytes.len() >= 4 && next() % 4 != 0 {
            let units = u16::try_from((bytes.len() - 4) / 4).expect("BUG: under 96 bytes");
            bytes[2..4].copy_from_slice(&units.to_ne_bytes());
        }
        let _ = Message::decode(&bytes);
    }
}

#[test]
fn compound_text_reads_as_utf8() {
    // UTF-8 inside its escape, and ISO 2022's Japanese set.
    assert_eq!(
        text_of(&[0x1b, 0x25, 0x47, 0xe3, 0x81, 0x82, 0x1b, 0x25, 0x40]).as_deref(),
        Some("あ")
    );
    assert_eq!(
        text_of(&[0x1b, 0x24, 0x28, 0x42, 0x45, 0x6c, 0x35, 0x7e]).as_deref(),
        Some("東京")
    );
    assert_eq!(text_of(b"plain").as_deref(), Some("plain"));
}

/// A GB 2312 or KS C 5601 run with a byte past 0x7f, which `xim_ctext`
/// would shift past a byte's range, is refused.
#[test]
fn compound_text_that_overflows_its_decoder_is_refused() {
    assert_eq!(text_of(&[0x1b, 0x24, 0x28, 0x41, 0xff]), None);
    assert_eq!(text_of(&[0x1b, 0x24, 0x28, 0x43, 0x30, 0x90]), None);
    // Invalid UTF-8 inside its escape.
    assert_eq!(text_of(&[0x1b, 0x25, 0x47, 0xff]), None);
    // The same sets within their range still read.
    assert!(text_of(&[0x1b, 0x24, 0x28, 0x41, 0x30, 0x21]).is_some());
}
