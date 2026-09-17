//! The Android backend's loop exit must release the window it tracked and
//! clear the callbacks that pin the embedder's presentation.
//!
//! # Why a source scan, and what it is *not* evidence of
//!
//! `AndroidPlatform::run` cannot execute on any host this suite runs on: it
//! needs a live `AndroidApp`, constructible only by the real Android
//! runtime, and the Android target is type-checked (the NDK-free `flui-app`
//! check, and `just cross-typecheck`) rather than run. So the site is pinned
//! textually, and this scan proves exactly one thing: **the release and the
//! clear are present in the exit region of `run`, in that order, in the
//! unfused shape.** Present in the region, never "reached" — nothing here
//! says a device executes either statement, and a conditional wrapper that
//! keeps both lines while losing a route (an `if bootstrap_error.is_none()`
//! around them) passes this scan. The runtime half of the same claim is
//! pinned on the primitive both backends call, by the capture-release test
//! in `shared/handlers.rs`.
//!
//! Every anchor below fails with an explicit "could not locate" message
//! instead of scanning nothing, and the inversion refusals come before
//! either shape test: a masking defect that mis-located the loop's close
//! would otherwise degrade this scan to a file-scoped `contains`. If `run`
//! is restructured, this test goes red and its editor moves the anchor with
//! the restructure.
//!
//! It also refuses the evasions that keep the text and lose the effect: a
//! fused `if let`, an early exit between the take and the clear, and any
//! second naming of the taken binding. That last refusal is stated over the
//! binding rather than over a line prefix, because an `if let`/`while let`/
//! `for` binding, a destructuring pattern and a `let` that does not start its
//! line all keep the shape assertions true; it is deliberately conservative,
//! so a harmless rebinding is refused too. What none of them can decide is
//! whether a route reaches the statements at all: a conditional wrapper above
//! them, or an early exit spelled in a way the token list cannot see (a macro
//! expanding to one, an imported `exit`). That residual is why the claim is
//! "present in the region", never "reached".

/// The Android platform module, read at compile time: a moved file fails to
/// compile rather than silently scanning nothing.
const ANDROID_PLATFORM_SOURCE: &str = include_str!("../src/platforms/android/mod.rs");

/// The signature `AndroidPlatform::run` is located by.
const RUN_SIGNATURE: &str = "fn run(self: Box<Self>";

/// The loop the exit region follows.
const LOOP_HEADER: &str = "loop {";

/// The tail call the exit region precedes.
const QUIT_CALL: &str = "invoke_quit()";

/// The take that releases the platform's own window reference.
const WINDOW_TAKE: &str = ".window.lock().take()";

/// The clear the exit path owes the window's callback slots.
const CALLBACKS_CLEAR: &str = ".callbacks().clear()";

/// The brace depth of `run`'s direct children, in [`line_start_depths`]'
/// scale (where the signature line is 0, so every line inside the body that
/// is not nested further reads 1).
const RUN_BODY_TOP_LEVEL: i32 = 1;

/// Cap on the exit region's non-blank lines. The tail is a handful of
/// statements; a region larger than this means the walk missed the loop's
/// close and swallowed the arm above it.
const MAX_EXIT_REGION_LINES: usize = 20;

/// `ANDROID_PLATFORM_SOURCE` with comment text stripped and literal contents
/// masked — in that order.
///
/// Stripping runs FIRST because the comments in and around `run` carry
/// apostrophes (`` `run`'s ``, `loop's`, `winit's own`), and a char-literal
/// masker that ran first would swallow from one apostrophe to the next,
/// taking a real closure's braces with it and mis-locating the loop's close.
///
/// A `//` inside a string literal would be mis-stripped by the first step;
/// `run` has none today, and the anchors in [`exit_region`] turn such a
/// mis-strip into an explicit failure rather than a pass.
fn preprocessed_source() -> String {
    let stripped: String = ANDROID_PLATFORM_SOURCE
        .lines()
        .map(|line| match line.find("//") {
            Some(comment_start) => &line[..comment_start],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    mask_literals(&stripped)
}

/// Replaces the CONTENTS of every string and char literal with a filler
/// character, keeping each delimiter and every line break.
///
/// Strings span line continuations on purpose: several `tracing::*!` calls
/// in this file split one message over two lines with a trailing `\`, and a
/// masker that stopped at the line break would leave the second line's
/// closing quote to open a fresh literal that swallows the code after it.
///
/// Char literals are matched STRICTLY — `'` + one char + `'`, or `'` + an
/// escape + `'` — never "swallow to the next `'`": this file's code lines
/// carry lifetimes (`Formatter<'_>`, `&'static str`) and no char literals at
/// all, so a lax masker would eat from the first lifetime through the rest
/// of the line and report "could not locate" for the wrong reason.
fn mask_literals(code: &str) -> String {
    const FILLER: char = '~';
    let chars: Vec<char> = code.chars().collect();
    let mut masked = String::with_capacity(code.len());
    let mut index = 0;

    while let Some(&ch) = chars.get(index) {
        match ch {
            '"' => {
                masked.push('"');
                index += 1;
                while let Some(&inner) = chars.get(index) {
                    if inner == '\\' {
                        // Consume the escape together with the character it
                        // escapes, so an escaped quote cannot close the
                        // literal. A line continuation is the one escape
                        // whose own character is kept.
                        index += 1;
                        if let Some(&escaped) = chars.get(index) {
                            masked.push(if escaped == '\n' { '\n' } else { FILLER });
                        }
                        index += 1;
                    } else if inner == '"' {
                        masked.push('"');
                        index += 1;
                        break;
                    } else {
                        masked.push(if inner == '\n' { '\n' } else { FILLER });
                        index += 1;
                    }
                }
            }
            '\'' => {
                if let Some(length) = char_literal_len(&chars, index) {
                    masked.push('\'');
                    for _ in 1..length - 1 {
                        masked.push(FILLER);
                    }
                    masked.push('\'');
                    index += length;
                } else {
                    masked.push('\'');
                    index += 1;
                }
            }
            _ => {
                masked.push(ch);
                index += 1;
            }
        }
    }
    masked
}

/// Length in characters of the strict char literal starting at `start`, or
/// `None` when the `'` there is a lifetime tick instead. The escape branch
/// looks a bounded distance ahead, so an unterminated escape cannot reach
/// across the file.
fn char_literal_len(chars: &[char], start: usize) -> Option<usize> {
    match chars.get(start + 1)? {
        '\\' => {
            let window_end = (start + 12).min(chars.len());
            let close = (start + 2..window_end).find(|&index| chars[index] == '\'')?;
            Some(close - start + 1)
        }
        '\n' => None,
        _ => (chars.get(start + 2) == Some(&'\'')).then_some(3),
    }
}

/// Brace depth at the start of each line of `source`, counted from
/// `body_open` — `run`'s opening brace, whose own line is index 0. Everything
/// before that offset is discarded, so the `impl` and `fn` braces are not
/// counted.
///
/// The returned vector is indexed by line offset from the signature line, so
/// it lines up with `body_lines` only while `run`'s `{` stays on that line. A
/// reformat that moves the brace to its own line misaligns every read, and
/// the scan then fails loudly on its first anchor ("expected exactly one
/// top-level `loop {` line … found 0") rather than passing on shifted text.
fn line_start_depths(source: &str, body_open: usize) -> Vec<i32> {
    let mut depths = Vec::new();
    let mut depth = 0;
    let mut at_line_start = false;

    for (offset, ch) in source.char_indices() {
        if offset == body_open {
            depth = 0;
            at_line_start = true;
            depths.clear();
        }
        if at_line_start {
            depths.push(depth);
            at_line_start = false;
        }
        match ch {
            '\n' => at_line_start = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depths
}

/// The lines of `AndroidPlatform::run` strictly between the close of its
/// `loop` and its `invoke_quit()` call, preprocessed, plus the inversion
/// refusals. Every anchor fails explicitly rather than yielding an empty
/// region a shape test would read as "nothing to check".
fn exit_region() -> Vec<String> {
    let source = preprocessed_source();

    let mut signature_hits = source
        .match_indices(RUN_SIGNATURE)
        .map(|(offset, _)| offset);
    let Some(signature_at) = signature_hits.next() else {
        panic!(
            "could not locate `AndroidPlatform::run`: `{RUN_SIGNATURE}` appears nowhere in \
             platforms/android/mod.rs; read the file by hand and update this scan"
        );
    };
    assert!(
        signature_hits.next().is_none(),
        "could not locate `AndroidPlatform::run`: `{RUN_SIGNATURE}` appears more than once in \
         platforms/android/mod.rs; read the file by hand and update this scan"
    );

    let Some(body_offset) = source[signature_at..].find('{') else {
        panic!(
            "could not locate `AndroidPlatform::run`'s body: its signature is never followed \
             by a `{{`; read the file by hand and update this scan"
        )
    };
    let body_open = signature_at + body_offset;

    let depths = line_start_depths(&source, body_open);
    let signature_line = source[..signature_at].matches('\n').count();
    // `run`'s own close, so the anchors below cannot reach a sibling
    // function's `loop {` or a second `invoke_quit()` further down the file:
    // an unrelated function added after `run` must not make this scan fail
    // with a message about `run`. A line that starts back at depth 0 is the
    // first line after the body's closing brace; the brace is the line
    // before it.
    let Some(back_at_top_level) = (1..depths.len()).find(|&line| depths[line] == 0) else {
        panic!(
            "could not locate the close of `AndroidPlatform::run`'s body: no line after its \
             opening brace returns the file to depth 0; read the file by hand and update this \
             scan"
        )
    };
    let body_close = back_at_top_level - 1;
    let body_lines: Vec<&str> = source
        .lines()
        .skip(signature_line)
        .take(body_close + 1)
        .collect();

    let loop_lines: Vec<usize> = body_lines
        .iter()
        .enumerate()
        .filter(|(line, text)| text.trim() == LOOP_HEADER && depths[*line] == RUN_BODY_TOP_LEVEL)
        .map(|(line, _)| line)
        .collect();
    let [loop_line] = loop_lines[..] else {
        panic!(
            "could not locate `run`'s `loop`: expected exactly one top-level `{LOOP_HEADER}` \
             line in `AndroidPlatform::run`, found {}; read the file by hand and update this \
             scan",
            loop_lines.len()
        );
    };

    // The line after which the body is back at the level the `loop` line
    // started from: that line is the loop's close.
    let loop_close = (loop_line + 1..body_close)
        .find(|&line| depths[line + 1] == RUN_BODY_TOP_LEVEL)
        .unwrap_or_else(|| {
            panic!(
                "could not locate the close of `run`'s `loop`: no line after it returns the \
                 body to its top level; read the file by hand and update this scan"
            )
        });

    let quit_lines: Vec<usize> = body_lines
        .iter()
        .enumerate()
        .filter(|(_, text)| text.contains(QUIT_CALL))
        .map(|(line, _)| line)
        .collect();
    let [quit_line] = quit_lines[..] else {
        panic!(
            "could not locate `{QUIT_CALL}` in `AndroidPlatform::run`: expected exactly one \
             line naming it, found {}; read the file by hand and update this scan",
            quit_lines.len()
        );
    };
    assert!(
        quit_line > loop_close,
        "could not locate `run`'s exit region: `{QUIT_CALL}` sits before the close of the \
         `loop`, so there is no tail to scan; read the file by hand and update this scan"
    );

    let region: Vec<String> = body_lines[loop_close + 1..quit_line]
        .iter()
        .map(|line| (*line).to_string())
        .collect();
    assert_exit_region_is_not_inverted(&region);
    assert_exit_region_has_no_early_exit(&region);
    region
}

/// Refuses the inversion a textual walker is known for. If a masking defect
/// shifted the loop's close upward, the region would swallow the `Destroy`
/// arm and both shape tests would degrade to a file-scoped `contains` that
/// the arm alone satisfies.
fn assert_exit_region_is_not_inverted(region: &[String]) {
    for banned in ["MainEvent::", "=>", "poll_events"] {
        assert!(
            !region.iter().any(|line| line.contains(banned)),
            "could not locate `run`'s exit region: it contains `{banned}`, which belongs to the \
             loop's body rather than to its tail, so the brace walk mis-located the loop's \
             close; read the file by hand and update this scan"
        );
    }

    let non_blank = region.iter().filter(|line| !line.trim().is_empty()).count();
    assert!(
        non_blank <= MAX_EXIT_REGION_LINES,
        "could not locate `run`'s exit region: it holds {non_blank} non-blank lines, more than \
         the {MAX_EXIT_REGION_LINES} a tail of a few statements can hold, so the brace walk \
         mis-located the loop's close; read the file by hand and update this scan"
    );
}

/// Refuses an early exit between the take and the clear.
///
/// A `return` there keeps both statements while losing the clear — and the
/// quit hook — on the route it takes, and `?` is the same hazard spelled the
/// way `run`'s own `Result` return type makes idiomatic; `panic!` and
/// `process::exit` are the unconditional forms; `break` is the one that
/// needs no loop at all, because a labelled block accepts it. None belongs in
/// a close tail that has to finish. The real tail's own `return Err(..)` sits
/// after `invoke_quit()`, outside the region, so the ban costs nothing.
///
/// This is a list of spellings, not a syntactic analysis: a macro expanding
/// to one of them, or an imported `exit(..)`, is invisible to it. That
/// residual is the header's, beside the conditional wrapper it already
/// admits — the scan decides presence and shape, never that a route reaches
/// them.
fn assert_exit_region_has_no_early_exit(region: &[String]) {
    for banned in ["return", "?", "panic!", "process::exit", "break"] {
        assert!(
            !region.iter().any(|line| line.contains(banned)),
            "the exit region contains `{banned}`, an early exit between the take and the clear: \
             on the route it takes, the window's callbacks are never cleared and the quit hook \
             never runs, so the registration cycle stays closed for the process's life. It \
             belongs after `invoke_quit()`, or the clear belongs before it"
        );
    }
}

/// Line indices of `region` whose text contains `needle`.
fn indices_containing(region: &[String], needle: &str) -> Vec<usize> {
    region
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(index, _)| index)
        .collect()
}

/// The exit region's one `callbacks().clear()` line: its index and its
/// trimmed text.
fn window_clear_line(region: &[String]) -> (usize, &str) {
    let clear_lines = indices_containing(region, CALLBACKS_CLEAR);
    assert_eq!(
        clear_lines.len(),
        1,
        "missing exit-path clear: `AndroidPlatform::run` has {} line(s) containing \
         `{CALLBACKS_CLEAR}` between its `loop` and its `{QUIT_CALL}`, expected exactly one. The \
         window's callback slots are the platform's only owning path into the embedder's \
         presentation (window -> slot -> closure -> raster lane -> renderer -> the window's own \
         `Arc`), so a loop that exits without clearing them strands a window, a raster lane and \
         a renderer for the life of the process, one more of each per activity recreation",
        clear_lines.len()
    );
    (clear_lines[0], region[clear_lines[0]].trim())
}

/// The exit region's take statement: its line index, and the identifier it
/// binds the platform's window to.
///
/// The `let`-statement requirement refuses the fused
/// `if let Some(w) = platform.window.lock().take() { w.callbacks().clear(); }`
/// whose scrutinee guard lives through the body — on edition 2024 an `if let`
/// drops its scrutinee's temporaries only after the whole `if let` — and
/// would therefore run the clear under the `window` lock. Whether the clear
/// reaches the window this take released is
/// [`assert_only_the_expected_lines_name_the_binding`]'s question.
fn window_take_binding(region: &[String]) -> (usize, &str) {
    let take_lines = indices_containing(region, WINDOW_TAKE);
    assert_eq!(
        take_lines.len(),
        1,
        "missing exit-path release: `AndroidPlatform::run` has {} line(s) containing \
         `{WINDOW_TAKE}` between its `loop` and its `{QUIT_CALL}`, expected exactly one. The \
         platform's own `window` field outlives the loop and is the window's close body's last \
         step on this backend, so the take belongs on the exit path, ahead of the clear",
        take_lines.len()
    );
    let take_index = take_lines[0];
    let take_line = region[take_index].trim();
    assert!(
        take_line.starts_with("let ") && take_line.ends_with(';'),
        "the platform's window must be taken in its OWN `let` statement, so the `window` guard \
         is released before the clear runs: found `{take_line}`"
    );
    let binding = take_line
        .strip_prefix("let ")
        .and_then(|rest| rest.split_once(" = ").map(|(binding, _)| binding));
    let Some(binding) = binding.filter(|binding| !binding.is_empty()) else {
        panic!("could not read the exit region's take statement: found `{take_line}`")
    };
    assert!(
        binding.chars().all(|ch| ch.is_alphanumeric() || ch == '_'),
        "the exit region's take statement binds `{binding}`, not a plain identifier: found \
         `{take_line}`"
    );
    // The expression, not just the name: a `.filter`/`.map`/`.ok()` chain, or a
    // take off another receiver, re-derives the value while every shape
    // assertion above stays true — the name tie would then be satisfied by a
    // window the platform never released, or by `None`.
    let expected = format!("let {binding} = platform.window.lock().take();");
    assert_eq!(
        take_line,
        expected.as_str(),
        "the platform's own window reference must be released by exactly `{expected}`: found \
         `{take_line}`. Anything else — a transformed `Option`, a take off a different receiver — \
         leaves the platform's field emptied while the clear reaches a value that is not the \
         window it released"
    );
    (take_index, binding)
}

/// Refuses every way the region can name the binding other than the three
/// lines allowed to.
///
/// A `let` reusing the name is the obvious evasion and it is not the only
/// one: an `if let`, `while let`, `for` or `match` binding, a destructuring
/// pattern, and a `let` that does not start its line all keep the receiver
/// tie and the ordering assertions true while the clear reaches a window the
/// platform never released. So the rule is stated over the binding rather
/// than over a line prefix — the take, the `if let` unwrap and the clear are
/// the only lines that may name it. That is deliberately conservative and the
/// conservatism reaches past rebinding: `let window = window;` reaches the
/// taken window and is refused all the same, a pure read of the name is
/// refused with the other unused lines, and renaming the unwrap's own inner
/// binding is refused by the clear-statement rule, which requires the
/// receiver to be the take's binding. Deciding whether an unusual use still
/// reaches the taken window needs the value, not the text, so the scan asks
/// for the shape the plan specifies and names it in the message when an
/// editor's line does not match.
///
/// String literals are masked before this runs, so a message that happens to
/// mention the binding is not a naming of it.
fn assert_only_the_expected_lines_name_the_binding(
    region: &[String],
    take_index: usize,
    clear_index: usize,
    binding: &str,
) {
    let unwrap = format!("if let Some({binding}) = {binding} {{");
    for (index, line) in region.iter().enumerate() {
        let trimmed = line.trim();
        if !names_binding(trimmed, binding) {
            continue;
        }
        assert!(
            index == take_index || index == clear_index || trimmed == unwrap,
            "the exit region names `{binding}` on line {index} (`{trimmed}`), which is neither \
             the take (line {take_index}), the clear (line {clear_index}) nor the `{unwrap}` \
             unwrap. A second binding of that name leaves every shape assertion true while the \
             clear may reach a window the platform never released"
        );
    }
}

/// Whether `line` uses `binding` as a whole identifier — `window` must not
/// match `windows`, and a shorter binding must not match a longer name.
fn names_binding(line: &str, binding: &str) -> bool {
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    line.match_indices(binding).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + binding.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// The identifier `clear()` is called on in `line`.
fn clear_receiver(line: &str) -> &str {
    let Some((receiver, _)) = line.split_once(".callbacks()") else {
        panic!("could not read the exit region's clear statement: found `{line}`")
    };
    let receiver = receiver.trim_end();
    assert!(
        !receiver.is_empty() && receiver.chars().all(|ch| ch.is_alphanumeric() || ch == '_'),
        "the exit-path clear's receiver is `{receiver}`, not a plain binding: found `{line}`. \
         The clear must reach the window this backend took out of its own `window` field, not \
         a reference to a different window"
    );
    receiver
}

/// The clear the loop's exit owes its window, in the unfused shape.
///
/// The clear count is asserted before the binding it is tied to, so a tree
/// without the site goes red on "missing exit-path clear" rather than on the
/// extraction that follows.
#[test]
fn android_run_clears_the_window_callbacks_between_its_loop_and_the_quit_hook() {
    let region = exit_region();

    let (clear_index, clear_line) = window_clear_line(&region);
    let (take_index, binding) = window_take_binding(&region);
    assert!(
        !clear_line.contains(".lock()"),
        "the exit-path clear must not run under the platform's `window` lock: found \
         `{clear_line}`. It drops embedder closures, which may re-enter platform code that \
         takes the same lock (ADR-0038 §5)"
    );
    let own_statement = format!("{binding}.callbacks().clear();");
    assert_eq!(
        clear_line,
        own_statement.as_str(),
        "the exit-path clear must be its own statement on the window the take released: found \
         `{clear_line}`, which bundles it with something else"
    );
    assert_eq!(
        clear_receiver(clear_line),
        binding,
        "the exit path must clear the SAME window it took out of the platform's `window` field: \
         found `{clear_line}`"
    );
    assert_only_the_expected_lines_name_the_binding(&region, take_index, clear_index, binding);
}

/// The platform's own reference goes before the clear, so that after the
/// release no arm and no tail can find a window to dispatch to.
#[test]
fn android_run_releases_its_own_window_reference_before_clearing() {
    let region = exit_region();

    let (take_index, binding) = window_take_binding(&region);
    let (clear_index, _) = window_clear_line(&region);

    assert!(
        take_index < clear_index,
        "the platform's own window reference must be released BEFORE the callbacks are cleared, \
         in the order the winit and headless close bodies use (their `complete_window_close` / \
         `complete_close`: dispatch close, drop the platform's tracking entry, then clear)"
    );
    assert_only_the_expected_lines_name_the_binding(&region, take_index, clear_index, binding);
}
