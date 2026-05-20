# PreToolUse Bash hook for FLUI — denies slow / dangerous cargo invocations.
#
# Mirrors the defensive strip-wrap pattern from oven-sh/bun#30412
# (.claude/hooks/pre-bash-zig-build.js — "Claude is a sneaky fucker"). Before
# the deny check, peels off common bypass wrappings the agent uses to dodge
# naive regex deny lists: leading inline env (`RUST_LOG=debug cargo …`),
# `timeout N cargo …`, trailing pipes / redirects (`… | head`, `… 2>&1`,
# `… > log`).
#
# Contract (matches Claude Code hook spec):
#   stdin  — JSON with { tool_name, tool_input: { command, timeout }, cwd }
#   stdout — empty on allow; deny JSON on block
#   exit   — always 0 (deny is signalled via stdout JSON, not exit code)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Read-HookInput {
    $raw = [Console]::In.ReadToEnd()
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    try {
        return $raw | ConvertFrom-Json
    } catch {
        return $null
    }
}

function Deny([string]$Reason) {
    $payload = @{
        hookSpecificOutput = @{
            hookEventName            = 'PreToolUse'
            permissionDecision       = 'deny'
            permissionDecisionReason = $Reason
        }
    }
    $payload | ConvertTo-Json -Depth 6 -Compress
    exit 0
}

# Shell-style tokenizer: splits on whitespace but respects single + double
# quotes. Strips the outer quotes once tokens are produced so the deny logic
# compares against the bare command text the user actually wrote.
function Get-Tokens([string]$Command) {
    if ([string]::IsNullOrEmpty($Command)) { return @() }
    # Match a run of non-whitespace, double-quoted, or single-quoted segments.
    # Double-quoted PS string with backtick-escaped " so we can embed both
    # quote characters in the regex literal.
    $pattern = "(?:[^\s`"']+|`"[^`"]*`"|'[^']*')+"
    $matches = [regex]::Matches($Command, $pattern)
    $tokens  = @()
    foreach ($m in $matches) {
        $t = $m.Value
        if ($t.Length -ge 2) {
            $first = $t[0]
            $last  = $t[$t.Length - 1]
            if (($first -eq '"' -and $last -eq '"') -or ($first -eq "'" -and $last -eq "'")) {
                $t = $t.Substring(1, $t.Length - 2)
            }
        }
        $tokens += $t
    }
    return ,$tokens
}

# Drop leading inline env assignments (FOO=bar, RUST_LOG=debug, etc.) so
# `RUST_LOG=debug cargo test` is checked as `cargo test`. Stops at the first
# real argv0.
function Remove-InlineEnv([string[]]$Tokens) {
    $i = 0
    while ($i -lt $Tokens.Count -and
           $Tokens[$i] -match '^[A-Za-z_][A-Za-z0-9_]*=' -and
           ($Tokens[$i] -notmatch '/') -and
           ($Tokens[$i] -notmatch '\\')) {
        $i++
    }
    if ($i -ge $Tokens.Count) { return @() }
    return ,($Tokens[$i..($Tokens.Count - 1)])
}

# Unwrap a `timeout N cmd …` (or `gtimeout`) wrapper. Skips flag args
# (`-k`, `--kill-after`, `--preserve-status`, …) and the numeric duration.
# Recurses so chained wrappers like `timeout 60 timeout 30 cargo …` collapse.
function Remove-TimeoutWrapper([string[]]$Tokens) {
    if ($Tokens.Count -eq 0) { return @() }
    $argv0 = [System.IO.Path]::GetFileNameWithoutExtension($Tokens[0])
    if ($argv0 -ne 'timeout' -and $argv0 -ne 'gtimeout') { return ,$Tokens }

    $rest = if ($Tokens.Count -gt 1) { $Tokens[1..($Tokens.Count - 1)] } else { @() }
    # Skip option flags
    while ($rest.Count -gt 0 -and $rest[0].StartsWith('-')) {
        $rest = if ($rest.Count -gt 1) { $rest[1..($rest.Count - 1)] } else { @() }
    }
    # Skip duration token (e.g. 60, 1m, 0.5h)
    if ($rest.Count -gt 0 -and $rest[0] -match '^[0-9]') {
        $rest = if ($rest.Count -gt 1) { $rest[1..($rest.Count - 1)] } else { @() }
    }
    # Recurse — handle nested wrapping
    return Remove-TimeoutWrapper (Remove-InlineEnv $rest)
}

# Cut everything from the first pipe (`|`) onwards. Then strip standalone
# redirect tokens (`>`, `>>`, `2>`, `&>`, `2>&1`, `1>&2`, `<`) and their
# adjacent filename argument so the remaining tokens describe the inner
# program + its arguments only.
function Remove-RedirectsAndPipes([string[]]$Tokens) {
    # Truncate at first standalone pipe
    $pipeIdx = -1
    for ($i = 0; $i -lt $Tokens.Count; $i++) {
        if ($Tokens[$i] -eq '|') { $pipeIdx = $i; break }
    }
    if ($pipeIdx -ge 0) {
        if ($pipeIdx -eq 0) { return @() }
        $Tokens = $Tokens[0..($pipeIdx - 1)]
    }

    $out = @()
    $skipNext = $false
    for ($i = 0; $i -lt $Tokens.Count; $i++) {
        if ($skipNext) { $skipNext = $false; continue }
        $t = $Tokens[$i]
        if ($t -in @('2>&1', '1>&2', '&>', '2>', '<')) { continue }
        if ($t -eq '>' -or $t -eq '>>') { $skipNext = $true; continue }
        # Inline forms: `>file`, `>>file`, `2>file`, `&>file`
        if ($t -match '^(>{1,2}|2>|&>)[^>]') { continue }
        $out += $t
    }
    return ,$out
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

$hookInput = Read-HookInput
if ($null -eq $hookInput) { exit 0 }

if ($hookInput.tool_name -ne 'Bash') { exit 0 }

$command = $hookInput.tool_input.command
if ([string]::IsNullOrWhiteSpace($command)) { exit 0 }

$tokens = Get-Tokens $command
if ($tokens.Count -eq 0) { exit 0 }

# Peel wrappings in the same order Claude tends to apply them.
$tokens = Remove-InlineEnv $tokens
$tokens = Remove-TimeoutWrapper $tokens
$tokens = Remove-RedirectsAndPipes $tokens
if ($tokens.Count -eq 0) { exit 0 }

$argv0 = [System.IO.Path]::GetFileNameWithoutExtension($tokens[0])
if ($argv0 -ne 'cargo') { exit 0 }

$rest        = if ($tokens.Count -gt 1) { $tokens[1..($tokens.Count - 1)] } else { @() }
$positionals = @($rest | Where-Object { -not $_.StartsWith('-') })
$flags       = @($rest | Where-Object { $_.StartsWith('-') })

$subcommand = if ($positionals.Count -gt 0) { $positionals[0] } else { '' }

# Rule 1: workspace-wide test without -p / --package
if ($subcommand -eq 'test') {
    $hasWorkspace = $flags -contains '--workspace' -or $flags -contains '--all'
    $hasPackage   = ($flags -contains '-p' -or $flags -contains '--package' -or
                     ($flags | Where-Object { $_ -like '--package=*' -or $_ -like '-p=*' }).Count -gt 0)
    if ($hasWorkspace -and -not $hasPackage) {
        Deny ("error: `cargo test --workspace` is slow on FLUI. Pass `-p <crate>` " +
              "or run a specific test target. To force-run the full suite, set " +
              "FLUI_HOOK_OFF=1 for this command.")
    }
}

# Rule 2: release build without an explicit opt-in env
if ($subcommand -eq 'build' -or $subcommand -eq 'rustc') {
    $isRelease = $flags -contains '--release' -or
                 (($flags | Where-Object { $_ -like '--profile=release' -or $_ -eq '--profile' }).Count -gt 0)
    if ($isRelease -and $env:FLUI_HOOK_OFF -ne '1') {
        Deny ("error: `cargo build --release` is slow during iteration. Use the " +
              "default dev profile, or set FLUI_HOOK_OFF=1 if you really need the " +
              "release binary right now.")
    }
}

# Rule 3: install / publish / yank — never appropriate for an automated agent.
if ($subcommand -in @('install', 'publish', 'yank', 'owner', 'login', 'logout')) {
    Deny "error: `cargo $subcommand` mutates the user's environment or crates.io. Run it yourself if you need it."
}

# Everything else is allowed.
exit 0
