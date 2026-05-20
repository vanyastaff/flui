# PostToolUse Write/Edit/MultiEdit hook for FLUI — auto-formats .rs files.
#
# Mirrors the bun pattern (.claude/hooks/post-edit-zig-format.js). We run
# `cargo fmt -- <file>` instead of `rustfmt` directly: workspace-wide edition
# / config comes from the closest Cargo.toml that way, matching what CI's
# `cargo fmt --all --check` will see.
#
# The hook never fails the Edit. cargo fmt errors are printed to stderr for
# visibility but exit is always 0 so the agent's flow continues.

$ErrorActionPreference = 'Continue'

function Read-HookInput {
    $raw = [Console]::In.ReadToEnd()
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    try {
        return $raw | ConvertFrom-Json
    } catch {
        return $null
    }
}

$hookInput = Read-HookInput
if ($null -eq $hookInput) { exit 0 }

if ($hookInput.tool_name -notin @('Write', 'Edit', 'MultiEdit')) { exit 0 }

$filePath = $hookInput.tool_input.file_path
if ([string]::IsNullOrWhiteSpace($filePath)) { exit 0 }

if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) { exit 0 }

$ext = [System.IO.Path]::GetExtension($filePath).ToLowerInvariant()
if ($ext -ne '.rs') { exit 0 }

# `cargo fmt -- <file>` is wrong here — cargo fmt forwards args to *every*
# per-package rustfmt invocation, so passing one file ends up formatting the
# entire workspace. Invoke rustfmt directly with the workspace edition
# (matches `rust-version = "1.94"`, `edition = "2024"` in the workspace
# Cargo.toml). CI still runs `cargo fmt --all --check` which uses the same
# edition, so the output stays consistent.
try {
    $rustfmtOutput = & rustfmt --edition 2024 $filePath 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "rustfmt failed for $filePath`n$rustfmtOutput"
    }
} catch {
    Write-Error "post-edit-fmt: rustfmt invocation failed: $($_.Exception.Message)"
}

exit 0
