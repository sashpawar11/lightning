# Performance guard. Run after every change:
#
#   scripts\bench.ps1                 # compare against the saved baseline, fail on regressions
#   scripts\bench.ps1 -SaveBaseline   # accept current numbers as the new baseline
#
# Between separate runs, identical code can differ by ~15% on the
# multi-threaded file benchmarks (thread scheduling on a tiny corpus), more
# than criterion's within-run confidence interval reflects. So:
#   * the baseline is the per-benchmark median of 3 runs, and
#   * a benchmark fails only if it is slower than baseline by more than
#     -Threshold (lower bound of the 95% CI) in its first run AND in
#     -Retries fresh re-runs. Real regressions reproduce; noise doesn't.
# The memory check indexes the bench corpus and fails if private bytes exceed
# the recorded baseline by more than -Threshold.
param(
    [switch]$SaveBaseline,
    [double]$Threshold = 0.10,
    [int]$Retries = 2
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)
$budgetFile = 'scripts\memory-baseline.json'
$corpus = Join-Path $env:TEMP 'lightning-bench-corpus-v1'
$criterion = 'target\criterion'

function Bench([string[]]$extra) {
    cargo bench -p lightning-core --bench search -- @extra
    if ($LASTEXITCODE) { throw "cargo bench failed (exit $LASTEXITCODE)" }
}

# Every benchmark directory, with its criterion id ("files_20k/report").
function Benchmarks {
    Get-ChildItem $criterion -Recurse -Filter benchmark.json |
        Where-Object { $_.Directory.Name -eq 'new' } |
        ForEach-Object {
            $meta = Get-Content $_.FullName -Raw | ConvertFrom-Json
            [pscustomobject]@{ Id = $meta.full_id; Dir = $_.Directory.Parent.FullName }
        }
}

function Mean($dir, $name) {
    $f = Join-Path $dir "$name\estimates.json"
    if (Test-Path $f) { (Get-Content $f -Raw | ConvertFrom-Json).mean.point_estimate }
}

function Measure-IndexMemory {
    $out = cargo run --release -q -p lightning-core --example probe -- $corpus 2>&1 | Out-String
    if ($out -notmatch 'files: (\d+) in .*private ([\d.]+) MB') { throw "probe output not understood:`n$out" }
    [pscustomobject]@{ files = [int]$Matches[1]; private_mb = [double]$Matches[2] }
}

if ($SaveBaseline) {
    foreach ($i in 1..3) { Bench @('--save-baseline', "run$i") }
    # Per benchmark, keep the run with the median mean as "main".
    foreach ($b in Benchmarks) {
        $runs = 1..3 | ForEach-Object { [pscustomobject]@{ Run = "run$_"; Mean = (Mean $b.Dir "run$_") } } |
            Where-Object Mean | Sort-Object Mean
        $median = $runs[[int][math]::Floor(($runs.Count - 1) / 2)].Run
        $main = Join-Path $b.Dir 'main'
        if (Test-Path $main) { Remove-Item $main -Recurse -Force }
        Copy-Item (Join-Path $b.Dir $median) $main -Recurse
        1..3 | ForEach-Object { Remove-Item (Join-Path $b.Dir "run$_") -Recurse -Force -ErrorAction SilentlyContinue }
    }
    Measure-IndexMemory | ConvertTo-Json | Set-Content -Encoding ascii $budgetFile
    Write-Host "Baseline saved (median of 3 runs)." -ForegroundColor Green
    exit 0
}

# Lower bound of the CI for the change vs. baseline, per benchmark id.
function Regressions([string[]]$ids) {
    foreach ($b in Benchmarks | Where-Object { -not $ids -or $ids -contains $_.Id }) {
        $f = Join-Path $b.Dir 'change\estimates.json'
        if (-not (Test-Path $f)) { continue }
        $j = Get-Content $f -Raw | ConvertFrom-Json
        [pscustomobject]@{
            Id = $b.Id
            Mean = $j.mean.point_estimate
            Low = $j.mean.confidence_interval.lower_bound
        }
    }
}

Bench @('--baseline-lenient', 'main')
$results = @(Regressions)
foreach ($r in $results | Sort-Object Id) { Write-Host ('{0,-48} {1,8:P1}' -f $r.Id, $r.Mean) }
$failing = @($results | Where-Object { $_.Low -gt $Threshold } | ForEach-Object Id)

for ($attempt = 1; $failing.Count -and $attempt -le $Retries; $attempt++) {
    Write-Host "`nRe-running $($failing.Count) over-threshold benchmark(s), attempt $attempt of ${Retries}: $($failing -join ', ')" -ForegroundColor Yellow
    # Escape only regex metacharacters (Rust's regex rejects some .NET escapes, e.g. "\ ").
    $esc = [System.Text.RegularExpressions.MatchEvaluator] { param($m) '\' + $m.Value }
    $filter = '^(' + (($failing | ForEach-Object { [regex]::Replace($_, '[.+*?()|\[\]{}^$]', $esc) }) -join '|') + ')$'
    Bench @('--baseline-lenient', 'main', $filter)
    $again = @(Regressions $failing)
    foreach ($r in $again) { Write-Host ('{0,-48} {1,8:P1}' -f $r.Id, $r.Mean) }
    $failing = @($again | Where-Object { $_.Low -gt $Threshold } | ForEach-Object Id)
}

$mem = Measure-IndexMemory
$memFail = $false
if (Test-Path $budgetFile) {
    $base = Get-Content $budgetFile -Raw | ConvertFrom-Json
    $limit = $base.private_mb * (1 + $Threshold)
    $line = 'memory: {0:N1} MB for {1} files (baseline {2:N1} MB, limit {3:N1} MB)' -f $mem.private_mb, $mem.files, $base.private_mb, $limit
    $memFail = $mem.private_mb -gt $limit
    Write-Host $line -ForegroundColor $(if ($memFail) { 'Red' } else { 'Gray' })
}

if ($failing.Count -or $memFail) {
    foreach ($id in $failing) { Write-Host "REGRESSION: $id (slower than +$($Threshold.ToString('P0')) on every attempt)" -ForegroundColor Red }
    exit 1
}
Write-Host "`nNo regressions." -ForegroundColor Green
