#!/usr/bin/env python3
"""Generate `decoy-wide-space.ttf` — a minimal face that maps ONLY U+0020.

Why it exists
-------------
`oversized_space_from_an_emoji_face_is_closed` reproduces issue #927's actual
symptom: an emoji face shaping the SPACE of a Latin run at ~1.24 em while the
letters shape elsewhere. Reproducing it needs a face that carries `U+0020` and
no letters. Neither shipped icon font qualifies — both map neither — so the
test used to build its fixture from the HOST's emoji font, which made a
merge-blocking assertion depend on a distro package's space advance: a Noto
Color Emoji metrics change would turn CI red on a font update (issue #932).

This face is generated, not copied, so it carries no third-party licence, and
its space advance is fixed at 1.3 em by construction rather than by whatever
the host happens to ship.

Its PostScript name deliberately contains "Emoji": that substring is the
heuristic cosmic-text itself uses to decide `not_emoji`
(`font/system.rs:35`), which is what puts emoji faces FIRST in its unfiltered
fallback tail (issue #930). A fixture that the shaper classifies the same way a
real emoji font is classified is what lets a hermetic test reach that path.

Run: `python3 tools/decoy-face/generate.py` (no dependencies).
"""

import struct
from pathlib import Path

UNITS_PER_EM = 1000
SPACE_ADVANCE = 1300  # 1.3 em — deliberately wide, the symptom under test.
NUM_GLYPHS = 2  # .notdef, space
FAMILY = "FLUI Decoy Emoji"
# `post_script_name.contains("Emoji")` is cosmic-text's own emoji predicate.
POSTSCRIPT = "FLUIDecoyEmoji"


def pad4(data: bytes) -> bytes:
    return data + b"\0" * (-len(data) % 4)


def checksum(data: bytes) -> int:
    data = pad4(data)
    return sum(struct.unpack(f">{len(data) // 4}I", data)) & 0xFFFFFFFF


def head_table() -> bytes:
    return struct.pack(
        ">IIIIHHqqhhhhHHhhh",
        0x00010000,  # version
        0x00010000,  # fontRevision
        0,  # checkSumAdjustment — patched after assembly
        0x5F0F3CF5,  # magicNumber
        0b0000_0000_0000_0011,  # flags
        UNITS_PER_EM,
        0,  # created
        0,  # modified
        0, 0, 0, 0,  # xMin yMin xMax yMax — empty outlines
        0,  # macStyle
        8,  # lowestRecPPEM
        2,  # fontDirectionHint
        0,  # indexToLocFormat: short
        0,  # glyphDataFormat
    )


def hhea_table() -> bytes:
    return struct.pack(
        ">IhhhHhhhhhhhhhhhH",
        0x00010000,
        800,   # ascender
        -200,  # descender
        0,     # lineGap
        SPACE_ADVANCE,  # advanceWidthMax
        0, 0, 0,  # minLeft/minRight/xMaxExtent
        1, 0,     # caretSlopeRise/Run
        0,        # caretOffset
        0, 0, 0, 0,  # reserved
        0,        # metricDataFormat
        NUM_GLYPHS,  # numberOfHMetrics
    )


def maxp_table() -> bytes:
    return struct.pack(">IH", 0x00010000, NUM_GLYPHS) + b"\0" * 26


def hmtx_table() -> bytes:
    # One longHorMetric per glyph (numberOfHMetrics == numGlyphs).
    return struct.pack(">Hh", 0, 0) + struct.pack(">Hh", SPACE_ADVANCE, 0)


def loca_table() -> bytes:
    # Short format: offsets/2. Both glyphs empty, so every offset is 0.
    return struct.pack(">HHH", 0, 0, 0)


def glyf_table() -> bytes:
    return b""


def cmap_table() -> bytes:
    # Format 4, one segment mapping U+0020 -> glyph 1, plus the required
    # 0xFFFF terminator segment.
    seg_count = 2
    end_codes = [0x0020, 0xFFFF]
    start_codes = [0x0020, 0xFFFF]
    # idDelta maps 0x20 -> 1: (0x20 + delta) & 0xFFFF == 1
    id_deltas = [(1 - 0x0020) & 0xFFFF, 1]
    id_range_offsets = [0, 0]

    subtable = struct.pack(
        ">HHHHHHH",
        4,  # format
        16 + 8 * seg_count,  # length
        0,  # language
        seg_count * 2,
        2 ** 1 * 2,  # searchRange
        1,           # entrySelector
        seg_count * 2 - 2 ** 1 * 2,  # rangeShift
    )
    subtable += struct.pack(f">{seg_count}H", *end_codes)
    subtable += struct.pack(">H", 0)  # reservedPad
    subtable += struct.pack(f">{seg_count}H", *start_codes)
    subtable += struct.pack(f">{seg_count}h", *[d - 0x10000 if d > 0x7FFF else d for d in id_deltas])
    subtable += struct.pack(f">{seg_count}H", *id_range_offsets)

    header = struct.pack(">HHHHI", 0, 1, 3, 1, 12)  # version, numTables, (3,1) -> offset 12
    return header + subtable


def name_table() -> bytes:
    # Windows platform (3), Unicode BMP (1), English US (0x409), UTF-16BE.
    records = [
        (1, FAMILY),        # family
        (2, "Regular"),     # subfamily
        (4, FAMILY),        # full name
        (6, POSTSCRIPT),    # PostScript name — the field cosmic-text reads
    ]
    storage = b""
    entries = b""
    for name_id, value in records:
        encoded = value.encode("utf-16-be")
        entries += struct.pack(">HHHHHH", 3, 1, 0x0409, name_id, len(encoded), len(storage))
        storage += encoded
    header = struct.pack(">HHH", 0, len(records), 6 + 12 * len(records))
    return header + entries + storage


def os2_table() -> bytes:
    """OS/2 version 4, 96 bytes, written field by field.

    fontdb reads weight/width/style from here; everything else is the
    minimum a validating parser expects to find.
    """
    parts = [
        struct.pack(">H", 4),               # version
        struct.pack(">h", SPACE_ADVANCE),   # xAvgCharWidth
        struct.pack(">H", 400),             # usWeightClass
        struct.pack(">H", 5),               # usWidthClass
        struct.pack(">H", 0),               # fsType
        struct.pack(">10h", *([0] * 10)),   # sub/superscript + strikeout
        struct.pack(">h", 0),               # sFamilyClass
        b"\0" * 10,                         # panose
        struct.pack(">4I", 0, 0, 0, 0),     # ulUnicodeRange1..4
        b"FLUI",                            # achVendID
        struct.pack(">H", 0x0040),          # fsSelection: REGULAR
        struct.pack(">H", 0x0020),          # usFirstCharIndex
        struct.pack(">H", 0x0020),          # usLastCharIndex
        struct.pack(">h", 800),             # sTypoAscender
        struct.pack(">h", -200),            # sTypoDescender
        struct.pack(">h", 0),               # sTypoLineGap
        struct.pack(">H", 1000),            # usWinAscent
        struct.pack(">H", 200),             # usWinDescent
        struct.pack(">2I", 0, 0),           # ulCodePageRange1..2
        struct.pack(">h", 0),               # sxHeight
        struct.pack(">h", 0),               # sCapHeight
        struct.pack(">H", 0x0020),          # usDefaultChar
        struct.pack(">H", 0x0020),          # usBreakChar
        struct.pack(">H", 1),               # usMaxContext
    ]
    table = b"".join(parts)
    assert len(table) == 96, f"OS/2 v4 must be 96 bytes, got {len(table)}"
    return table


TABLES = {
    b"OS/2": os2_table(),
    b"cmap": cmap_table(),
    b"glyf": glyf_table(),
    b"head": head_table(),
    b"hhea": hhea_table(),
    b"hmtx": hmtx_table(),
    b"loca": loca_table(),
    b"maxp": maxp_table(),
    b"name": name_table(),
    b"post": struct.pack(">IIhhIIIII", 0x00030000, 0, 0, 0, 0, 0, 0, 0, 0),
}


def build() -> bytes:
    tags = sorted(TABLES)
    num_tables = len(tags)
    search_range = 16 * (2 ** (num_tables.bit_length() - 1))
    entry_selector = num_tables.bit_length() - 1
    offset = 12 + 16 * num_tables

    records = b""
    body = b""
    for tag in tags:
        data = TABLES[tag]
        records += struct.pack(">4sIII", tag, checksum(data), offset + len(body), len(data))
        body += pad4(data)

    header = struct.pack(
        ">IHHHH", 0x00010000, num_tables, search_range, entry_selector,
        num_tables * 16 - search_range,
    )
    font = header + records + body

    # head.checkSumAdjustment = 0xB1B0AFBA - checksum(whole font)
    head_offset = None
    for i, tag in enumerate(tags):
        if tag == b"head":
            head_offset = struct.unpack(">I", font[12 + 16 * i + 8: 12 + 16 * i + 12])[0]
    adjustment = (0xB1B0AFBA - checksum(font)) & 0xFFFFFFFF
    font = font[: head_offset + 8] + struct.pack(">I", adjustment) + font[head_offset + 12:]
    return font


if __name__ == "__main__":
    out = Path(__file__).resolve().parents[2] / "crates/flui-engine/assets/fonts/decoy-wide-space.ttf"
    data = build()
    out.write_bytes(data)
    print(f"wrote {out} ({len(data)} bytes)")
