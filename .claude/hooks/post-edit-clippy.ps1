#!/usr/bin/env pwsh
# Post-Edit Hook: Run clippy after Rust file edits
# Outputs warnings to help catch issues early

param()

$input_json = $input | ConvertFrom-Json

# Only check for Rust files
$file_path = $input_json.tool_input.file_path
if (-not $file_path) {
    $file_path = $input_json.tool_input.path
}

if (-not $file_path -or -not ($file_path -match '\.rs$')) {
    # Not a Rust file, skip
    exit 0
}

# Extract crate name from path
if ($file_path -match 'crates[/\\]([^/\\]+)') {
    $crate_name = $matches[1]
    Write-Host "Running clippy on $crate_name..." -ForegroundColor Cyan
    
    $clippy_output = cargo clippy -p $crate_name --message-format=short 2>&1
    
    # Check for warnings/errors
    $warnings = $clippy_output | Select-String -Pattern "warning:|error\[" 
    if ($warnings) {
        Write-Host "`nClippy found issues:" -ForegroundColor Yellow
        $warnings | ForEach-Object { Write-Host $_.Line -ForegroundColor Yellow }
    }
}

exit 0
