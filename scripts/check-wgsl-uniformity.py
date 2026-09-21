#!/usr/bin/env python3
"""Refuse a WGSL derivative taken inside a branch.

WebGPU's uniformity analysis (Tint, in every browser) admits `dpdx`/`dpdy`/
`fwidth` and their coarse/fine variants only in uniform control flow. Native
wgpu goes through naga, whose analysis is weaker: on 2026-09-21 every
clip-capable pipeline compiled natively and failed module creation in the
browser because `sdfToAlpha` (which takes derivatives) was called under an
`if` on per-instance clip data, and the arc shader took its angular gradient
after a per-instance early `return`. naga's validator with every flag on
accepted both, so there is no host-side oracle for this class; this check is
the structural stand-in.

The rule is syntactic and deliberately conservative: inside every function,
a call to a derivative builtin — or to any function that transitively calls
one — must not sit inside an `if`, `else`, `switch`, `loop`, `for` or `while`
block, and must not follow an early `return` in the same function. A branch
on a genuinely uniform value would be legal for Tint but is refused here too;
compute the derivative unconditionally and `select` afterwards, which is
also what the shaders do today.

Usage: check-wgsl-uniformity.py [ROOT]   (default: crates/flui-engine/src/shaders)
Exit 1 with one line per finding; 0 when clean.
"""
import pathlib
import re
import sys

DERIVATIVES = {'dpdx', 'dpdy', 'fwidth', 'dpdxCoarse', 'dpdyCoarse', 'fwidthCoarse',
               'dpdxFine', 'dpdyFine', 'fwidthFine'}
FN = re.compile(r'\bfn\s+([A-Za-z_]\w*)\s*\(')
CALL = re.compile(r'\b([A-Za-z_]\w*)\s*\(')
BRANCH = re.compile(r'^\s*(if|else|switch|loop|for|while)\b')
RETURN = re.compile(r'^\s*return\b')


def strip_comments(text):
    text = re.sub(r'/\*.*?\*/', lambda m: ' ' * len(m.group(0)), text, flags=re.S)
    return '\n'.join(line.split('//', 1)[0] for line in text.splitlines())


def functions(text):
    """Yield (name, body_lines_with_numbers) for every fn in `text`."""
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        match = FN.search(lines[i])
        if not match:
            i += 1
            continue
        name = match.group(1)
        # Find the body's opening brace, then its matching close.
        depth, started, body = 0, False, []
        j = i
        while j < len(lines):
            line = lines[j]
            for ch in line:
                if ch == '{':
                    depth += 1
                    started = True
                elif ch == '}':
                    depth -= 1
            body.append((j + 1, line))
            if started and depth == 0:
                break
            j += 1
        yield name, body
        i = j + 1


def calls_in(body):
    names = set()
    for _, line in body:
        names.update(CALL.findall(line))
    return names


def main():
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'crates/flui-engine/src/shaders')
    sources = {path: strip_comments(path.read_text()) for path in sorted(root.rglob('*.wgsl'))}
    # Transitive closure of "takes a derivative", across every file: the
    # helpers live in common/*.wgsl and are concatenated into the modules.
    fn_calls = {}
    for text in sources.values():
        for name, body in functions(text):
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
    for path, text in sources.items():
        for name, body in functions(text):
            depth = 0          # brace depth relative to the body
            branch_depth = []  # brace depths at which a branch block opened
            returned = False
            for number, line in body[1:]:
                if BRANCH.match(line):
                    branch_depth.append(depth)
                if RETURN.match(line) and branch_depth:
                    # A return inside a conditional block: whether the code
                    # after the block runs depends on the condition.
                    returned = True
                for called in CALL.findall(line):
                    if called in derives and called != name:
                        if branch_depth:
                            findings.append(f'{path}:{number}: `{called}` takes a derivative inside a branch in `{name}`')
                        elif returned:
                            findings.append(f'{path}:{number}: `{called}` takes a derivative after an early return in `{name}`')
                for ch in line:
                    if ch == '{':
                        depth += 1
                    elif ch == '}':
                        depth -= 1
                        if branch_depth and depth <= branch_depth[-1]:
                            branch_depth.pop()
    for finding in findings:
        print(finding)
    if findings:
        print(f'wgsl-uniformity: {len(findings)} derivative(s) outside uniform control flow', file=sys.stderr)
        return 1
    print(f'wgsl-uniformity: {len(sources)} shader files, {len(derives) - len(DERIVATIVES)} derivative-taking functions, all in uniform control flow')
    return 0


if __name__ == '__main__':
    sys.exit(main())
