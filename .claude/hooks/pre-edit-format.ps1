#!/usr/bin/env pwsh
# Pre-Edit Hook: Check Rust file formatting before edits
# Returns exit 0 to continue, exit 2 to block with error

param()

$input_json = $input | ConvertFrom-Json

# Only check for Rust files
$file_path = $input_json.tool_input.file_path
if (-not $file_path) {
    $file_path = $input_json.tool_input.path
}

if (-not $file_path -or -not ($file_path -match '\.rs$')) {
    # Not a Rust file, allow
    exit 0
}

# Check if cargo fmt would make changes
$result = cargo fmt -- --check 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "Warning: Some files need formatting. Running cargo fmt..." -ForegroundColor Yellow
    cargo fmt --all
}

exit 0
