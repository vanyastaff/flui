#!/usr/bin/env pwsh
# Stop Hook: Log session activity and run final checks
# Provides session summary when Claude finishes responding

param()

$timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
$log_file = ".claude/session-log.txt"

# Ensure log directory exists
$log_dir = Split-Path $log_file -Parent
if (-not (Test-Path $log_dir)) {
    New-Item -ItemType Directory -Path $log_dir -Force | Out-Null
}

# Log session activity
Add-Content -Path $log_file -Value "[$timestamp] Session response completed"

# Quick workspace check
$check_result = cargo check --workspace 2>&1 | Select-String -Pattern "error\[E"
if ($check_result) {
    Write-Host "`nWorkspace has compilation errors:" -ForegroundColor Red
    $check_result | Select-Object -First 5 | ForEach-Object { Write-Host $_.Line -ForegroundColor Red }
}

exit 0
