$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$probeExe = (Get-ChildItem (Join-Path $workspace "target/debug/deps") -Filter "flui_engine-*.exe" | Select-Object -First 1).FullName
if (!$probeExe) { throw "First run run-engine-filter-footprint-probe.ps1 to build the witness" }
$listing = & $probeExe --list
if (!($listing -match 'foreground_crop_research_witness')) { throw "Test binary lacks research probe; run the probe builder first" }
$stdoutPath = Join-Path $PSScriptRoot "engine-filter-memory-probe.stdout.log"
$stderrPath = Join-Path $PSScriptRoot "engine-filter-memory-probe.stderr.log"
$probeProcess = Start-Process -FilePath $probeExe -ArgumentList '--exact','painter::foreground_research_probe::foreground_crop_research_witness','--nocapture','--test-threads=1' -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
$rows = @()
$elapsed = [System.Diagnostics.Stopwatch]::StartNew()
while (!$probeProcess.HasExited -and $elapsed.Elapsed.TotalSeconds -lt 20) {
    $sample = Get-Counter -Counter '\GPU Process Memory(*)\Dedicated Usage','\GPU Process Memory(*)\Shared Usage','\GPU Process Memory(*)\Total Committed' -ErrorAction SilentlyContinue
    $matching = @($sample.CounterSamples | Where-Object { $_.InstanceName -like "pid_$($probeProcess.Id)_*" })
    if ($matching.Count) {
        $rows += [pscustomobject]@{
            elapsed_ms = $elapsed.Elapsed.TotalMilliseconds
            process_id = $probeProcess.Id
            dedicated_bytes = ($matching | Where-Object Path -Like '*\dedicated usage' | Measure-Object CookedValue -Sum).Sum
            shared_bytes = ($matching | Where-Object Path -Like '*\shared usage' | Measure-Object CookedValue -Sum).Sum
            total_committed_bytes = ($matching | Where-Object Path -Like '*\total committed' | Measure-Object CookedValue -Sum).Sum
        }
    }
}
if (!$probeProcess.WaitForExit(30000)) { Stop-Process -Id $probeProcess.Id; throw "Research process exceeded the sampling deadline" }
ConvertTo-Json -InputObject @($rows) -Depth 4 | Set-Content (Join-Path $PSScriptRoot "engine-filter-process-memory-samples.json")
$rows | Format-Table
Write-Output "Samples: $($rows.Count). Expected baseline test exit: $($probeProcess.ExitCode). Sampled counters are not an exact allocation peak."
