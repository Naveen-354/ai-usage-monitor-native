<#
.SYNOPSIS
  Samples CPU and memory of the app *and its WebView2 child processes* (the real footprint).
.DESCRIPTION
  Every -IntervalSeconds, prints CPU% (share of ONE core, summed over the process tree) and memory
  (working set and private bytes, summed). Ends with min/avg/max, so idle behaviour can be judged
  against the targets (idle CPU < 1 %, RAM < 150 MB).
.EXAMPLE
  pwsh scripts/perf/sample.ps1 -Seconds 60 -IntervalSeconds 2
#>
param(
  [string]$ProcessName = 'ai-usage-monitor',
  [int]$Seconds = 60,
  [int]$IntervalSeconds = 2,
  [switch]$Quiet
)

function Get-Tree([int]$RootPid) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
  $ids = New-Object System.Collections.Generic.HashSet[int]; [void]$ids.Add($RootPid)
  do { $before = $ids.Count; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId)) { [void]$ids.Add([int]$p.ProcessId) } } } while ($ids.Count -ne $before)
  $ids
}

$root = Get-Process -Name $ProcessName -ErrorAction Stop | Select-Object -First 1
$cores = [Environment]::ProcessorCount
$prevCpu = @{}; $prevAt = Get-Date
$rows = @()
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
  Start-Sleep -Seconds $IntervalSeconds
  $now = Get-Date; $dt = ($now - $prevAt).TotalSeconds; $prevAt = $now
  $cpuNow = @{}; $ws = 0L; $priv = 0L; $count = 0
  foreach ($id in (Get-Tree $root.Id)) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue; if (-not $p) { continue }
    $count++; $ws += $p.WorkingSet64; $priv += $p.PrivateMemorySize64
    $cpuNow[$id] = $p.TotalProcessorTime.TotalSeconds
  }
  $delta = 0.0
  foreach ($k in $cpuNow.Keys) { if ($prevCpu.ContainsKey($k)) { $delta += ($cpuNow[$k] - $prevCpu[$k]) } }
  $prevCpu = $cpuNow
  $row = [pscustomobject]@{ T = $now.ToString('HH:mm:ss'); CPU_pct = [math]::Round(100 * $delta / $dt, 1); WS_MB = [math]::Round($ws / 1MB, 0); Private_MB = [math]::Round($priv / 1MB, 0); Procs = $count }
  $rows += $row
  if (-not $Quiet) { $row | Format-Table -HideTableHeaders -AutoSize | Out-String -Width 120 | Write-Host -NoNewline }
}
$valid = $rows | Select-Object -Skip 1   # first interval has no CPU baseline
if ($valid) {
  $m = $valid | Measure-Object CPU_pct -Minimum -Maximum -Average
  $w = $valid | Measure-Object WS_MB -Maximum -Average
  "SUMMARY ({0} samples, {1} logical cores; CPU is % of one core): CPU avg {2:N1} / max {3:N1} · working set avg {4:N0} MB / max {5:N0} MB · processes {6}" -f $valid.Count, $cores, $m.Average, $m.Maximum, $w.Average, $w.Maximum, ($rows | Select-Object -Last 1).Procs
}
