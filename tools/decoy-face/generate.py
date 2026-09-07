#!/usr/bin/env python3
"""Generate the synthetic font fixtures under `crates/flui-engine/assets/fonts`.

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

The same machinery supplies three more fixtures, for the same reason in a
different place: every font asset the repository ships is a single-weight,
non-monospaced, static face, so three arms of `font_resolve`'s weight
resolution had nothing that could tell a correct implementation from a broken
one. `FACES` at the bottom of this file lists what each fixture exists to
discriminate.

Run: `python3 tools/decoy-face/generate.py` (no dependencies). Output is
deterministic: regenerating over an unchanged `FACES` reproduces every file
byte for byte.
"""

import struct
from dataclasses import dataclass
from pathlib import Path

UNITS_PER_EM = 1000


@dataclass(frozen=True)
class FaceSpec:
    """One generated face. Every field is something a test asserts on."""

    family: str
    postscript: str
    subfamily: str
    out: str
    #: `hmtx` advance for U+0020, in font units.
    space_advance: int = 1000
    #: Map U+0041 as well, so `can_render_latin` accepts the face and the
    #: generic binding can choose it. The outline stays empty — coverage is
    #: what every probe in `font_resolve` actually reads.
    letters: bool = False
    #: `OS/2.usWeightClass`, which is what fontdb reports as `face.weight`.
    weight: int = 400
    #: `post.isFixedPitch`, which is what fontdb reports as `face.monospaced`.
    monospaced: bool = False
    #: `(min, default, max)` for an `fvar` `wght` axis, or `None` for a static
    #: face. Present so a test can reach the variable-weight arm of
    #: `family_accepts_weight`, which no static fixture can.
    variable_wght: tuple[int, int, int] | None = None

    @property
    def num_glyphs(self) -> int:
        return 3 if self.letters else 2  # .notdef, space, [A]


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


def hhea_table(spec: FaceSpec) -> bytes:
    return struct.pack(
        ">IhhhHhhhhhhhhhhhH",
        0x00010000,
        800,   # ascender
        -200,  # descender
        0,     # lineGap
        spec.space_advance,  # advanceWidthMax
        0, 0, 0,  # minLeft/minRight/xMaxExtent
        1, 0,     # caretSlopeRise/Run
        0,        # caretOffset
        0, 0, 0, 0,  # reserved
        0,        # metricDataFormat
        spec.num_glyphs,  # numberOfHMetrics
    )


def maxp_table(spec: FaceSpec) -> bytes:
    return struct.pack(">IH", 0x00010000, spec.num_glyphs) + b"\0" * 26


def hmtx_table(spec: FaceSpec) -> bytes:
    # One longHorMetric per glyph (numberOfHMetrics == numGlyphs).
    metrics = struct.pack(">Hh", 0, 0) + struct.pack(">Hh", spec.space_advance, 0)
    if spec.letters:
        metrics += struct.pack(">Hh", spec.space_advance, 0)
    return metrics


def loca_table(spec: FaceSpec) -> bytes:
    # Short format: offsets/2. Every glyph is empty, so every offset is 0.
    return struct.pack(f">{spec.num_glyphs + 1}H", *([0] * (spec.num_glyphs + 1)))


def glyf_table() -> bytes:
    return b""


def cmap_table(spec: FaceSpec) -> bytes:
    # Format 4: one segment per mapped codepoint, plus the required 0xFFFF
    # terminator segment.
    codes = [(0x0020, 1)] + ([(0x0041, 2)] if spec.letters else [])
    end_codes = [code for code, _ in codes] + [0xFFFF]
    start_codes = list(end_codes)
    # idDelta maps `code` to `glyph`: (code + delta) & 0xFFFF == glyph
    id_deltas = [(glyph - code) & 0xFFFF for code, glyph in codes] + [1]
    seg_count = len(end_codes)
    id_range_offsets = [0] * seg_count

    entry_selector = seg_count.bit_length() - 1
    search_range = 2 * (2 ** entry_selector)
    subtable = struct.pack(
        ">HHHHHHH",
        4,  # format
        16 + 8 * seg_count,  # length
        0,  # language
        seg_count * 2,
        search_range,
        entry_selector,
        seg_count * 2 - search_range,  # rangeShift
    )
    subtable += struct.pack(f">{seg_count}H", *end_codes)
    subtable += struct.pack(">H", 0)  # reservedPad
    subtable += struct.pack(f">{seg_count}H", *start_codes)
    subtable += struct.pack(f">{seg_count}h", *[d - 0x10000 if d > 0x7FFF else d for d in id_deltas])
    subtable += struct.pack(f">{seg_count}H", *id_range_offsets)

    header = struct.pack(">HHHHI", 0, 1, 3, 1, 12)  # version, numTables, (3,1) -> offset 12
    return header + subtable


def name_table(spec: FaceSpec) -> bytes:
    # Windows platform (3), Unicode BMP (1), English US (0x409), UTF-16BE.
    records = [
        (1, spec.family),                              # family
        (2, spec.subfamily),                           # subfamily
        (4, f"{spec.family} {spec.subfamily}".strip()  # full name
            if spec.subfamily != "Regular" else spec.family),
        (6, spec.postscript),  # PostScript name — the field cosmic-text reads
    ]
    storage = b""
    entries = b""
    for name_id, value in records:
        encoded = value.encode("utf-16-be")
        entries += struct.pack(">HHHHHH", 3, 1, 0x0409, name_id, len(encoded), len(storage))
        storage += encoded
    header = struct.pack(">HHH", 0, len(records), 6 + 12 * len(records))
    return header + entries + storage


def os2_table(spec: FaceSpec) -> bytes:
    """OS/2 version 4, 96 bytes, written field by field.

    fontdb reads weight/width/style from here; everything else is the
    minimum a validating parser expects to find.
    """
    parts = [
        struct.pack(">H", 4),               # version
        struct.pack(">h", spec.space_advance),  # xAvgCharWidth
        struct.pack(">H", spec.weight),         # usWeightClass
        struct.pack(">H", 5),               # usWidthClass
        struct.pack(">H", 0),               # fsType
        struct.pack(">10h", *([0] * 10)),   # sub/superscript + strikeout
        struct.pack(">h", 0),               # sFamilyClass
        b"\0" * 10,                         # panose
        struct.pack(">4I", 0, 0, 0, 0),     # ulUnicodeRange1..4
        b"FLUI",                            # achVendID
        struct.pack(">H", 0x0040),          # fsSelection: REGULAR
        struct.pack(">H", 0x0020),          # usFirstCharIndex
        struct.pack(">H", 0x0041 if spec.letters else 0x0020),  # usLastCharIndex
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


def fvar_table(spec: FaceSpec) -> bytes:
    """A one-axis `fvar` naming `wght`.

    Enough for `skrifa`'s `font.axes().get_by_tag(b"wght")`, which is all
    `variable_weight_covers` — and cosmic-text's own `variable_weight_match`
    — actually read. No `gvar`: nothing here rasterises an instance, and a
    deltaless variable face is still a variable face to both probes.
    """
    minimum, default, maximum = spec.variable_wght
    fixed = lambda v: struct.pack(">i", int(v * 65536))
    header = struct.pack(
        ">HHHHHHHH",
        1, 0,   # version major/minor
        16,     # axesArrayOffset
        2,      # reserved (countSizePairs)
        1,      # axisCount
        20,     # axisSize
        0,      # instanceCount
        8,      # instanceSize (4 + axisCount * 4)
    )
    axis = (
        b"wght"
        + fixed(minimum)
        + fixed(default)
        + fixed(maximum)
        + struct.pack(">HH", 0, 256)  # flags, axisNameID
    )
    return header + axis


def tables_for(spec: FaceSpec) -> dict[bytes, bytes]:
    tables = {
        b"OS/2": os2_table(spec),
        b"cmap": cmap_table(spec),
        b"glyf": glyf_table(),
        b"head": head_table(),
        b"hhea": hhea_table(spec),
        b"hmtx": hmtx_table(spec),
        b"loca": loca_table(spec),
        b"maxp": maxp_table(spec),
        b"name": name_table(spec),
        # `post` v3.0; `isFixedPitch` is the field fontdb reports as
        # `face.monospaced`.
        b"post": struct.pack(
            ">IIhhIIIII", 0x00030000, 0, 0, 0, int(spec.monospaced), 0, 0, 0, 0
        ),
    }
    if spec.variable_wght is not None:
        tables[b"fvar"] = fvar_table(spec)
    return tables


def build(spec: FaceSpec) -> bytes:
    TABLES = tables_for(spec)
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


#: Every generated fixture, and the property each one exists to give a test.
FACES = [
    # The oversized space of issue #927, with "Emoji" in the PostScript name so
    # cosmic-text classifies it the way it classifies a real emoji font.
    FaceSpec(
        family="FLUI Decoy Emoji",
        postscript="FLUIDecoyEmoji",
        subfamily="Regular",
        out="decoy-wide-space.ttf",
        space_advance=1300,  # 1.3 em — deliberately wide, the symptom.
    ),
    # One family at two weights, monospaced, carrying letters. Three probes in
    # `font_resolve` have no other fixture that discriminates them:
    #   * `family_accepts_weight` must NOT accept a monospaced face at a weight
    #     it does not carry (the arm that read cosmic-text's request-level
    #     `is_mono` as a face property);
    #   * `snap_weight` must pick by CSS order rather than absolute distance —
    #     100 and 600 against a W500 request disagree, 100 being the CSS answer
    #     and 600 the nearest;
    #   * the generic binding needs a family it can actually choose, which
    #     means `can_render_latin` coverage.
    FaceSpec(
        family="FLUI Probe Mono",
        postscript="FLUIProbeMono-Thin",
        subfamily="Thin",
        out="probe-mono-100.ttf",
        letters=True,
        weight=100,
        monospaced=True,
    ),
    FaceSpec(
        family="FLUI Probe Mono",
        postscript="FLUIProbeMono-SemiBold",
        subfamily="SemiBold",
        out="probe-mono-600.ttf",
        letters=True,
        weight=600,
        monospaced=True,
    ),
    # A variable face whose `wght` axis spans 100..900 while `usWeightClass`
    # says 400 — the only fixture that reaches the variable-weight arm of
    # `family_accepts_weight`, which every static face leaves untouched.
    FaceSpec(
        family="FLUI Probe Variable",
        postscript="FLUIProbeVariable",
        subfamily="Regular",
        out="probe-variable-wght.ttf",
        letters=True,
        weight=400,
        variable_wght=(100, 400, 900),
    ),
]


if __name__ == "__main__":
    assets = Path(__file__).resolve().parents[2] / "crates/flui-engine/assets/fonts"
    for spec in FACES:
        data = build(spec)
        out = assets / spec.out
        out.write_bytes(data)
        print(f"wrote {out} ({len(data)} bytes)")
