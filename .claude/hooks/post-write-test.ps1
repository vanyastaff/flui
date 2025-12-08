#!/usr/bin/env pwsh
# Post-Write Hook: Run tests for modified crate after file creation
# Helps catch issues early by running targeted tests

param()

$input_json = $input | ConvertFrom-Json

$file_path = $input_json.tool_input.file_path
if (-not $file_path) {
    $file_path = $input_json.tool_input.path
}

if (-not $file_path -or -not ($file_path -match '\.rs$')) {
    exit 0
}

# Extract crate name from path
if ($file_path -match 'crates[/\\]([^/\\]+)') {
    $crate_name = $matches[1]
    
    # Only run tests if it's a test file or has test modules
    $content = Get-Content $file_path -Raw -ErrorAction SilentlyContinue
    if ($content -match '#\[cfg\(test\)\]|#\[test\]') {
        Write-Host "Running tests for $crate_name..." -ForegroundColor Cyan
        cargo test -p $crate_name --no-fail-fast 2>&1 | Select-Object -Last 20
    }
}

exit 0
