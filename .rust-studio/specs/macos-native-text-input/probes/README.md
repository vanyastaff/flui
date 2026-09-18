# Probes — can a *real* input method be driven from this host?

The shipped probe (`crates/flui-platform/examples/ime_probe.rs`, run by `just macos-ime`) drives
AppKit's `NSTextInputClient` protocol by hand: the probe itself calls `setMarkedText:` /
`insertText:`, standing in for the input method. The gap that leaves is named in the plan's §6 and
§8 — no genuine input method runs, so no real *composition* is covered.

These four standalone programs were written to attack that gap directly, off the Rust build (they
compile in seconds with `clang`, so they were usable while `just ci` held cargo's lock). They are
kept because the answer they returned is a *negative* that the plan's record depends on, and
because re-running them is how that negative gets re-checked — on this host or another.

They are not part of the build and nothing runs them automatically.

## What was measured (2026-09-17, this host)

| probe | question | answer |
|---|---|---|
| `sources.m` | which keyboard input sources exist? | **Two, both raw keylayouts** — `com.apple.keylayout.US` and `com.apple.keylayout.Russian`. No source of type `kTISTypeInputMethod`. |
| `deadkey_live.m` | does a dead-key press sequence compose? | **No.** Option-E then `e` → two independent `insertText:` calls (`´`, `e`), zero `setMarkedText:`, `hasMarkedText=0`. |
| `deadkey_us_selected.m` | …because the *Russian* layout was active? | **No** — same result with `com.apple.keylayout.US` explicitly selected via `TISSelectInputSource` (status 0), original source restored afterwards. |
| `layout_tables.m` | …because the U.S. layout has no dead key? | **No** — reading the layout's own tables, Option-E is `""` with `deadKeyState=1` and the following `e` resolves to `é`. The dead key is defined. |

Read together: the layout defines the composition, the input source is one that would use it, and
the input *context* still declines to engage it. The conversion engine an input method would provide
is simply not present on this host — which `sources.m` is what rules out retrying with a different
source. Enabling one is a System Settings change, not something a probe may make for its user.

**So the ROADMAP's IME gap is not closed by this work**, and the shipped probe no longer claims it
is: assertion F asserts the *route* a modified key takes (input-method path, keyboard path silent)
and reports the characters, because which character a host's layout yields — and whether its context
composes it — is a property of the host, not a contract of this backend.

## Building and running

Each is a single `.m` file. `sources.m` and `layout_tables.m` run as plain binaries:

```bash
clang -fobjc-arc -framework Cocoa -framework Carbon -o sources sources.m && ./sources
clang -fobjc-arc -framework Cocoa -framework Carbon -o layout_tables layout_tables.m && ./layout_tables
```

The two `deadkey_*` probes construct an `NSWindow` and so hit the same floor the Rust probe
documents — an unbundled window throws `_CFBundleGetValueForInfoKey`, a foreign NSException Rust
cannot catch. Stage them into a minimal bundle first (this `Info.plist` is the same one
`just macos-ime` stages into its `.app`):

```bash
clang -fobjc-arc -framework Cocoa -o DeadKey deadkey_live.m
mkdir -p DeadKey.app/Contents/MacOS
cp Info.plist DeadKey.app/Contents/Info.plist
cp DeadKey     DeadKey.app/Contents/MacOS/DeadKey
./DeadKey.app/Contents/MacOS/DeadKey
```

Every probe writes its callbacks and findings to **stderr** (`fprintf(stderr, …)`, deliberately not
`stdout`) and exits 0 regardless — they are instruments, not assertions.

One caveat on `deadkey_us_selected.m`: it changes the machine's selected input source for the
duration of the run. It restores the original afterwards and logged the restore
(`PROBE restored input source = com.apple.keylayout.Russian`), but this is a probe that touches a
user-level setting, which is why it is here and not in the workspace's test suite.
