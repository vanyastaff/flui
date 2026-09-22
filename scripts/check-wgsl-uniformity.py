"""Refuse a WGSL derivative taken inside a branch.

WebGPU's uniformity analysis (Tint, in every browser) admits `dpdx`/`dpdy`/
`fwidth` and their coarse/fine variants — and the implicit-derivative texture
samplers `textureSample`/`textureSampleBias`/`textureSampleCompare` — only in
uniform control flow. Native wgpu goes through naga, whose analysis is
weaker: on 2026-09-21 every clip-capable pipeline compiled natively and
failed module creation in the browser because `sdfToAlpha` (which takes
derivatives) was called under an `if` on per-instance clip data, and the arc
shader took its angular gradient after a per-instance early `return`. naga's
validator with every flag on accepted both, so there is no host-side oracle
for this class; this check is the structural stand-in.

The rule is syntactic and deliberately conservative: inside every function,
a call to a derivative builtin — or to any function that transitively calls
one — must not sit inside an `if`, `else`, `switch`, `loop`, `for` or `while`
block, and must not follow a `return` inside such a block earlier in the same
function. A branch on a genuinely uniform value is legal for Tint but refused
here unless a comment on the branch keyword's line, or on a line just
before it, carries the marker `wgsl-uniformity: uniform` and says why the
condition is uniform (a uniform-buffer field, a constant): that block, and
an `else` chained to it, are then walked as straight-line code. A marker
arms exactly the next branch keyword after it. Everything else: compute the
derivative unconditionally and `select` afterwards, which is what the
shaders do today.

The walk is over tokens, not lines, so layout cannot hide a branch: `} else
{` on the closing line of an `if`, a one-line `if c { return x; }`, a `for`
header split across lines and a call split from its argument list are all
seen for what they are.

Usage: check-wgsl-uniformity.py [ROOT]   (default: crates/flui-engine/src/shaders)
       check-wgsl-uniformity.py --self-test
Exit 1 with one line per finding; 0 when clean. `--self-test` runs the
walker over the embedded fixture — every layout the line-anchored first
version missed — and exits 0 only when each expected finding, and no other,
is reported.
"""
import pathlib
import re
import sys

DERIVATIVES = {
    'dpdx', 'dpdy', 'fwidth', 'dpdxCoarse', 'dpdyCoarse', 'fwidthCoarse',
    'dpdxFine', 'dpdyFine', 'fwidthFine',
    'textureSample', 'textureSampleBias', 'textureSampleCompare',
}
BRANCH_KEYWORDS = {'if', 'else', 'switch', 'loop', 'for', 'while'}
UNIFORM_MARKER = 'wgsl-uniformity: uniform'
# Identifiers, and every single punctuation character that matters to the
# walk; everything else (numbers, operators, commas) is skipped.
TOKEN = re.compile(r'[A-Za-z_]\w*|[{}();]')


def strip_comments(text):
    """Blank comments without moving anything: a block comment keeps its
    newlines so every later token stays on its own line — the marker
    lines are numbered against the raw text."""
    text = re.sub(r'/\*.*?\*/', lambda m: re.sub(r'[^\n]', ' ', m.group(0)), text, flags=re.S)
    return '\n'.join(line.split('//', 1)[0] for line in text.splitlines())


def uniform_marked_lines(text):
    """Line numbers whose comment carries the uniform-branch marker."""
    return {
        number for number, line in enumerate(text.splitlines(), start=1)
        if UNIFORM_MARKER in line
    }


def arm_uniform_branches(tokens, marker_lines):
    """Rewrite `tokens` to (line, tok, armed): a marker arms the NEXT branch
    keyword at or after its line, and nothing else. A marker with no branch
    after it in its own function is dropped at the next `fn`, so it can
    never arm a branch in a later function."""
    tokens = list(tokens)
    armed_tokens = []
    pending = sorted(marker_lines)
    for index, (number, tok) in enumerate(tokens):
        armed = False
        if tok == 'fn':
            pending = [line for line in pending if line >= number]
        # The `else` of an `else if` is not the branch the marker means:
        # the `if` that follows carries the condition, so it takes the arm.
        chained_if = tok == 'else' and index + 1 < len(tokens) and tokens[index + 1][1] == 'if'
        if tok in BRANCH_KEYWORDS and not chained_if and pending and pending[0] <= number:
            armed = True
            pending.pop(0)
        armed_tokens.append((number, tok, armed))
    return armed_tokens


def tokenize(text):
    """Yield (line_number, token) for every token of the comment-stripped text."""
    for number, line in enumerate(text.splitlines(), start=1):
        for match in TOKEN.finditer(line):
            yield number, match.group(0)


def functions(tokens):
    """Yield (name, body_tokens) for every `fn NAME(...) ... { body }`.

    `body_tokens` are the tokens strictly between the body's braces.
    """
    i = 0
    while i < len(tokens):
        if tokens[i][1] != 'fn' or i + 1 >= len(tokens):
            i += 1
            continue
        name = tokens[i + 1][1]
        # Skip to the body's opening brace: the first `{` at paren depth 0
        # after the parameter list.
        j = i + 2
        paren = 0
        while j < len(tokens):
            tok = tokens[j][1]
            if tok == '(':
                paren += 1
            elif tok == ')':
                paren -= 1
            elif tok == '{' and paren == 0:
                break
            j += 1
        depth = 0
        k = j
        while k < len(tokens):
            tok = tokens[k][1]
            if tok == '{':
                depth += 1
            elif tok == '}':
                depth -= 1
                if depth == 0:
                    break
            k += 1
        yield name, tokens[j + 1:k]
        i = k + 1


def calls_in(body):
    """Names called in `body`: an identifier directly followed by `(`."""
    return {
        tok for (_, tok, _), (_, nxt, _) in zip(body, body[1:])
        if tok not in BRANCH_KEYWORDS and nxt == '(' and re.fullmatch(r'[A-Za-z_]\w*', tok)
    }


# What a `{` opened, one entry per open block.
PLAIN, BRANCH, UNIFORM = 'plain', 'branch', 'uniform'


def check_function(path, name, body, derives):
    """Findings for one function body: derivative-taking calls in non-uniform flow."""
    findings = []
    blocks = []             # PLAIN / BRANCH / UNIFORM per open `{`
    pending = PLAIN         # what the next `{` opens, from the statement head so far
    chained_uniform = False # the block just closed was UNIFORM; an `else` continues it
    returned = False        # a `return` inside a BRANCH block earlier in this body
    paren = 0               # a `;` inside a `for (init; cond; step)` header is not a statement end
    for index, (number, tok, armed) in enumerate(body):
        if tok in BRANCH_KEYWORDS:
            if armed or (tok == 'else' and chained_uniform):
                pending = UNIFORM
            elif tok == 'if' and pending == UNIFORM and not armed:
                # `else if <cond>` chained to a uniform branch: the new
                # condition is its own question, and only its own marker
                # can answer it — a bare `else {` inherits, `else if` does
                # not.
                pending = BRANCH
            elif pending != UNIFORM:
                pending = BRANCH
        elif tok == '(':
            paren += 1
        elif tok == ')':
            paren -= 1
        elif tok == '{':
            blocks.append(pending)
            pending = PLAIN
        elif tok == '}':
            chained_uniform = bool(blocks) and blocks.pop() == UNIFORM
            pending = PLAIN
            continue
        elif tok == ';' and paren == 0:
            pending = PLAIN
        elif tok == 'return' and BRANCH in blocks:
            returned = True
        elif index + 1 < len(body) and body[index + 1][1] == '(':
            if tok in derives and tok != name:
                if BRANCH in blocks:
                    findings.append(f'{path}:{number}: `{tok}` takes a derivative inside a branch in `{name}`')
                elif returned:
                    findings.append(f'{path}:{number}: `{tok}` takes a derivative after an early return in `{name}`')
        chained_uniform = False
    return findings


def analyse(texts):
    """Findings across `texts` ({path: wgsl source}), in file order."""
    sources = {
        path: arm_uniform_branches(tokenize(strip_comments(text)), uniform_marked_lines(text))
        for path, text in texts.items()
    }
    # Transitive closure of "takes a derivative", across every file: the
    # helpers live in common/*.wgsl and are concatenated into the modules.
    fn_calls = {}
    for tokens in sources.values():
        for name, body in functions(tokens):
            fn_calls.setdefault(name, set()).update(calls_in(body))
    derives = set(DERIVATIVES)
    changed = True
    while changed:
        changed = False
        for name, called in fn_calls.items():
            if name not in derives and called & derives:
                derives.add(name)
                changed = True

    findings = []
    for path, tokens in sources.items():
        for name, body in functions(tokens):
            findings.extend(check_function(path, name, body, derives))
    return findings, len(derives) - len(DERIVATIVES)


# One function per layout the walk must see through. `EXPECTED` names the
# function and the kind of finding for each one that must be reported; every
# other function must come back clean.
SELF_TEST_FIXTURE = """
fn else_on_closing_line(x: f32, c: bool) -> f32 {
    var r = 0.0;
    if c {
        r = 1.0;
    } else {
        r = dpdx(x);
    }
    return r;
}
fn one_line_if_return(x: f32, c: bool) -> f32 {
    if c { return 1.0; }
    return dpdx(x);
}
fn one_line_if_else_return(x: f32, c: bool) -> f32 {
    if c { return 1.0; } else { return 2.0; }
    return fwidth(x);
}
fn for_header_split_and_call_split(x: f32) -> f32 {
    for (var i = 0;
         i < 2; i++) {
        return 0.0;
    }
    return helper
        (x);
}
fn helper(x: f32) -> f32 { return dpdy(x); }
fn sampler_in_branch(uv: vec2<f32>, c: bool) -> vec4<f32> {
    if c {
        return textureSample(t, s, uv);
    }
    return vec4<f32>(0.0);
}
fn unconditional_then_select(x: f32, c: bool) -> f32 {
    let g = fwidth(x);
    if c { return 1.0; }
    return select(g, 0.0, c);
}
fn marked_uniform_if_else(x: f32) -> f32 {
    // wgsl-uniformity: uniform (u is the uniform buffer)
    if u.flag > 0.0 {
        return dpdx(x);
    } else {
        return fwidth(x);
    }
}
fn marked_uniform_loop(x: f32) -> f32 {
    var acc = 0.0;
    for (var i = 0; i < u.taps; i++) { // wgsl-uniformity: uniform
        acc += dpdx(x);
    }
    return acc;
}
fn marker_arms_only_the_next_branch(x: f32, c: bool) -> f32 {
    if u.flag > 0.0 { // wgsl-uniformity: uniform
        return 1.0;
    }
    if c {
        return dpdx(x);
    }
    return 0.0;
}
fn else_if_does_not_inherit_the_marker(x: f32, c: bool) -> f32 {
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return 1.0;
    } else if c {
        return dpdx(x);
    }
    return 0.0;
}
fn else_if_with_its_own_marker(x: f32) -> f32 {
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return 1.0;
    } else if u.other > 0.0 { // wgsl-uniformity: uniform
        return dpdx(x);
    }
    return 0.0;
}
fn block_comment_before_marker(x: f32) -> f32 {
    /* a block comment
       spanning three
       lines */
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return dpdx(x);
    }
    return 0.0;
}
fn marker_without_a_branch_is_dropped_here(x: f32) -> f32 {
    // wgsl-uniformity: uniform (nothing to arm in this function)
    return x;
}
fn branch_after_a_stale_marker(x: f32, c: bool) -> f32 {
    if c {
        return dpdx(x);
    }
    return 0.0;
}
"""
EXPECTED = {
    ('else_on_closing_line', 'inside a branch'),
    ('one_line_if_return', 'after an early return'),
    ('one_line_if_else_return', 'after an early return'),
    ('for_header_split_and_call_split', 'after an early return'),
    ('sampler_in_branch', 'inside a branch'),
    ('marker_arms_only_the_next_branch', 'inside a branch'),
    ('else_if_does_not_inherit_the_marker', 'inside a branch'),
    ('branch_after_a_stale_marker', 'inside a branch'),
}


def self_test():
    findings, _ = analyse({pathlib.Path('fixture.wgsl'): SELF_TEST_FIXTURE})
    seen = set()
    for finding in findings:
        kind = 'inside a branch' if 'inside a branch' in finding else 'after an early return'
        seen.add((finding.rsplit('`', 2)[1], kind))
    missing, extra = EXPECTED - seen, seen - EXPECTED
    for item in sorted(missing):
        print(f'self-test: MISSED {item}')
    for item in sorted(extra):
        print(f'self-test: FALSE POSITIVE {item}')
    if missing or extra:
        return 1
    print(f'wgsl-uniformity: self-test ok ({len(EXPECTED)} expected findings, no others)')
    return 0


def main():
    if sys.argv[1:] == ['--self-test']:
        return self_test()
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'crates/flui-engine/src/shaders')
    texts = {path: path.read_text() for path in sorted(root.rglob('*.wgsl'))}
    findings, derived = analyse(texts)
    for finding in findings:
        print(finding)
    if findings:
        print(f'wgsl-uniformity: {len(findings)} derivative(s) outside uniform control flow', file=sys.stderr)
        return 1
    print(f'wgsl-uniformity: {len(texts)} shader files, {derived} derivative-taking functions, all in uniform control flow')
    return 0


if __name__ == '__main__':
    sys.exit(main())
