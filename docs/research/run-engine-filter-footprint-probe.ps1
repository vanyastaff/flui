param([string]$LogPath = "docs/research/engine-filter-footprint-probe.log")
$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$source = Join-Path $workspace "crates/flui-engine/src/painter/mod.rs"
$probeFiles = @(
    "painter/mod.rs", "texture_pool.rs", "layer_offscreen.rs", "blur/mod.rs",
    "replay/flush.rs", "replay/ordered.rs", "replay/coverage.rs", "clip_mask.rs"
)
$originals = @{}
$previousJobs = $env:CARGO_BUILD_JOBS
$previousGpu = $env:FLUI_REQUIRE_GPU
$probeExit = 1
Push-Location $workspace
try {
    foreach ($probeFile in $probeFiles) {
        $relative = "crates/flui-engine/src/$probeFile"
        $dirty = git diff HEAD --name-only -- $relative
        if ($dirty) { throw "Refusing to instrument modified source: $relative" }
        $absolute = Join-Path $workspace $relative
        $originals[$absolute] = [System.IO.File]::ReadAllBytes($absolute)
    }
    foreach ($probeFile in $probeFiles | Where-Object { $_ -notin @("painter/mod.rs", "texture_pool.rs") }) {
        $absolute = Join-Path $workspace "crates/flui-engine/src/$probeFile"
        $text = [System.IO.File]::ReadAllText($absolute)
        $text = [regex]::Replace($text, '(?m)^(\s*)(let (?:mut )?\w+ = )encoder\.begin_render_pass',
            '$1#[cfg(all(test, feature = "testing"))]' + "`n" + '$1eprintln!("PROBE_PASS");' + "`n" + '$1$2encoder.begin_render_pass')
        [System.IO.File]::WriteAllText($absolute, $text)
    }
    $pool = Join-Path $workspace "crates/flui-engine/src/texture_pool.rs"
    $poolText = [System.IO.File]::ReadAllText($pool)
    $poolText = $poolText.Replace('self.inventory.total_memory_bytes += desc.size_bytes();',
        'self.inventory.total_memory_bytes += desc.size_bytes();' + "`n" +
        '#[cfg(all(test, feature = "testing"))]' + "`n" +
        'eprintln!("PROBE_POOL bytes={} width={} height={}", self.inventory.total_memory_bytes, width, height);')
    [System.IO.File]::WriteAllText($pool, $poolText)
    [System.IO.File]::AppendAllText($source, "`n#[cfg(all(test, feature = `"testing`"))]`n#[path = `"../../../../docs/research/engine-filter-footprint-probe.rs`"]`nmod foreground_research_probe;`n")
    $env:CARGO_BUILD_JOBS = "1"
    $env:FLUI_REQUIRE_GPU = "1"
    # Windows PowerShell treats redirected native stderr as an ErrorRecord.
    # Cargo's normal progress is stderr; wait for native exit before restoring sources.
    $ErrorActionPreference = "Continue"
    cargo test -p flui-engine --features testing --lib foreground_crop_research_witness --locked -- --nocapture --test-threads=1 *> $LogPath
    $probeExit = $LASTEXITCODE
} finally {
    foreach ($absolute in $originals.Keys) {
        [System.IO.File]::WriteAllBytes($absolute, $originals[$absolute])
    }
    $env:CARGO_BUILD_JOBS = $previousJobs
    $env:FLUI_REQUIRE_GPU = $previousGpu
    Pop-Location
}
Write-Output "Probe exit: $probeExit. Source restored byte for byte. Log: $LogPath"
exit $probeExit
