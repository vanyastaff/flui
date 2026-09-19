# `decoy-wide-space.ttf` — provenance

**Generated, not copied.** Produced by [`tools/decoy-face/generate.py`](../../../../tools/decoy-face/generate.py),
which has no dependencies and writes the file from scratch. It carries **no
third-party font licence** because it contains no third party's work: two empty
glyph outlines, a `cmap` mapping only `U+0020`, and the minimum table set a
parser expects.

**What it is for.** `oversized_space_from_an_emoji_face_is_closed`
(`flui-painting`) reproduces issue #927's symptom — an emoji face shaping the
SPACE of a Latin run while the letters shape elsewhere. That needs a face
carrying `U+0020` and no letters; neither shipped icon font qualifies
(`MaterialIcons-Regular.ttf` and `CupertinoIcons.ttf` map neither), so the test
used to build its fixture from the **host's** emoji font.

**Why that mattered.** The assertion is merge-blocking and reads a space
advance, so parameterising it on a distro font package meant a Noto Color Emoji
metrics change could turn CI red on a *font update*, with the cause nowhere
near the diff that triggered it (issue #932).

**Two deliberate properties:**

- **Space advance 1.3 em** (1300 units at 1000 upem) — wide enough that the
  symptom is unambiguous, and fixed by construction rather than by whatever the
  host ships.
- **PostScript name contains "Emoji"** (`FLUIDecoyEmoji`). That substring is the
  heuristic cosmic-text itself uses to decide `not_emoji`
  (`font/system.rs:35`), which is what sorts emoji faces FIRST in its unfiltered
  fallback tail. A fixture the shaper classifies the way it classifies a real
  emoji font is what lets a hermetic test reach that path — see issue #930.

Regenerate with `python3 tools/decoy-face/generate.py`; the output is
byte-stable.
