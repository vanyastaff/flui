//! The bounds a native recording's writers and its reader count with one `Budget`.

use flui_protocol::native_recording::{
    Budget, CutReason, MAX_LINE_BYTES, MAX_RECORDING_BYTES, MAX_ROWS, Overrun,
};

/// A writer's admission: each row is checked against the line, header, row and byte bounds in
/// that order, room is kept for the `cut` row while a row may follow, and a refused row takes
/// nothing.
#[test]
fn a_writer_admits_a_row_only_while_the_recording_and_its_cut_fit() {
    let line = MAX_LINE_BYTES + 1;
    let fills = |rows: usize| {
        let mut budget = Budget::after_header(100);
        for _ in 0..rows {
            budget.admit_row(1, 0, false).expect("a short row fits");
        }
        budget
    };
    // Rows as long as a line may be, until `left` bytes are left of the recording.
    let leaves = |left: usize| {
        let mut budget = Budget::after_header(100);
        while budget.bytes() < MAX_RECORDING_BYTES - left {
            let row = line.min(MAX_RECORDING_BYTES - left - budget.bytes());
            budget
                .admit_row(row, 0, false)
                .expect("a row within the bound fits");
        }
        budget
    };
    // (case, the budget, the row's line, the header's growth, whether a row follows, expected)
    type Case = (
        &'static str,
        Budget,
        usize,
        usize,
        bool,
        Result<(), Overrun>,
    );
    let cases: [Case; 9] = [
        ("a line at the bound", fills(0), line, 0, true, Ok(())),
        (
            "a line past it",
            fills(0),
            line + 1,
            0,
            true,
            Err(Overrun::LineBytes),
        ),
        (
            "a header grown to the bound",
            fills(0),
            2,
            line - 100,
            true,
            Ok(()),
        ),
        (
            "a header grown past it",
            fills(0),
            2,
            line - 99,
            true,
            Err(Overrun::HeaderBytes),
        ),
        (
            "the last row, nothing after it",
            fills(MAX_ROWS - 1),
            1,
            0,
            false,
            Ok(()),
        ),
        (
            "the last row, with a cut's place after it",
            fills(MAX_ROWS - 1),
            1,
            0,
            true,
            Err(Overrun::Rows),
        ),
        (
            "the bytes to the bound, nothing after",
            leaves(10),
            10,
            0,
            false,
            Ok(()),
        ),
        (
            "the bytes to the bound, with a cut's room after",
            leaves(10),
            10,
            0,
            true,
            Err(Overrun::RecordingBytes),
        ),
        (
            "a length that would overflow",
            Budget::after_header(usize::MAX),
            1,
            usize::MAX,
            true,
            Err(Overrun::HeaderBytes),
        ),
    ];
    for (case, mut budget, row, declared, follows, expected) in cases {
        let before = budget;
        assert_eq!(budget.admit_row(row, declared, follows), expected, "{case}");
        if expected.is_err() {
            assert_eq!(budget, before, "{case}: a refused row takes nothing");
        } else {
            assert_eq!(budget.rows(), before.rows() + 1, "{case}");
            assert_eq!(budget.bytes(), before.bytes() + row + declared, "{case}");
        }
    }
}

/// A writer that admits each row before it writes it never writes what the reader's count of
/// the same lines refuses: the rows and bytes it admits, with the `cut` it ends on, read whole.
#[test]
fn what_a_writer_admits_with_its_cut_a_reader_takes() {
    for (row, expected) in [(1, CutReason::Rows), (4096, CutReason::Bytes)] {
        let header = 200;
        let mut writer = Budget::after_header(header);
        let mut written = Vec::new();
        let cut = loop {
            match writer.admit_row(row, 0, true) {
                Ok(()) => written.push(row),
                Err(overrun) => break overrun.cut_reason().expect("the recording is full"),
            }
        };
        assert_eq!(cut, expected);
        // The longest `cut` line a writer ends on.
        written.push(r#"{"row":"cut","reason":"store_journal"}"#.len() + 1);
        let mut reader = Budget::after_header(header);
        for (index, line) in written.iter().enumerate() {
            assert_eq!(reader.read_line(*line), Ok(()), "line {}", index + 2);
        }
        assert!(reader.bytes() <= MAX_RECORDING_BYTES);
        assert!(reader.rows() <= MAX_ROWS);
    }
}

/// A reader refuses the line that takes a recording past its bytes, then the row past its rows.
#[test]
fn a_reader_refuses_the_line_past_either_bound() {
    let mut full = Budget::after_header(MAX_RECORDING_BYTES - 1);
    assert_eq!(full.read_line(1), Ok(()));
    assert_eq!(full.read_line(1), Err(Overrun::RecordingBytes));

    let mut rows = Budget::after_header(1);
    for _ in 0..MAX_ROWS {
        rows.read_line(1).expect("within the rows");
    }
    assert_eq!(rows.read_line(1), Err(Overrun::Rows));
    assert_eq!(rows.rows(), MAX_ROWS, "the refused line is not taken");
}

/// The cut a writer ends on names the bound that filled the recording, and a line too long on
/// its own ends nothing.
#[test]
fn an_overrun_names_its_limit_and_its_cut() {
    for (overrun, limit, cut) in [
        (Overrun::LineBytes, MAX_LINE_BYTES, None),
        (Overrun::HeaderBytes, MAX_LINE_BYTES, None),
        (Overrun::Rows, MAX_ROWS, Some(CutReason::Rows)),
        (
            Overrun::RecordingBytes,
            MAX_RECORDING_BYTES,
            Some(CutReason::Bytes),
        ),
    ] {
        assert_eq!(overrun.limit(), limit, "{overrun:?}");
        assert_eq!(overrun.cut_reason(), cut, "{overrun:?}");
        assert!(
            overrun.to_string().contains(&limit.to_string()),
            "{overrun}"
        );
    }
}
