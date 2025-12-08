#!/usr/bin/env pwsh
# Pre-Edit Hook: Validate Cargo.toml changes
# Ensures workspace dependencies are properly configured

param()

$input_json = $input | ConvertFrom-Json

$file_path = $input_json.tool_input.file_path
if (-not $file_path) {
    $file_path = $input_json.tool_input.path
}

if (-not $file_path -or -not ($file_path -match 'Cargo\.toml$')) {
    exit 0
}

# Run cargo check to validate Cargo.toml
$result = cargo check --workspace 2>&1 | Select-String -Pattern "error"
if ($result) {
    $output = @{
        decision = "block"
        reason = "Cargo.toml validation failed. Please check workspace dependencies."
    } | ConvertTo-Json
    Write-Output $output
    exit 0
}

exit 0
