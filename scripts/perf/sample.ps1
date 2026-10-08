<#
.SYNOPSIS
  Samples CPU and memory of the running app (a single process).
.DESCRIPTION
  Every -IntervalSeconds, prints CPU% (share of ONE core) and memory (working set and private bytes).
  Ends with min/avg/max, so idle behaviour can be judged against the targets (idle CPU < 1 %, RAM < 150 MB).
.EXAMPLE
  pwsh scripts/perf/sample.ps1 -Seconds 60 -IntervalSeconds 2
#>
param(
  [string]$ProcessName = 'ai-usage-monitor-native',
  [int]$Seconds = 60,
  [int]$IntervalSeconds = 2,
  [switch]$Quiet
)

$proc = Get-Process -Name $ProcessName -ErrorAction Stop | Select-Object -First 1
$cores = [Environment]::ProcessorCount
$prevCpu = $null; $prevAt = Get-Date
$rows = @()
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
  Start-Sleep -Seconds $IntervalSeconds
  $p = Get-Process -Id $proc.Id -ErrorAction Stop
  $now = Get-Date; $dt = ($now - $prevAt).TotalSeconds; $prevAt = $now
  $cpuNow = $p.TotalProcessorTime.TotalSeconds
  $delta = if ($null -ne $prevCpu) { $cpuNow - $prevCpu } else { 0.0 }
  $prevCpu = $cpuNow
  $row = [pscustomobject]@{ T = $now.ToString('HH:mm:ss'); CPU_pct = [math]::Round(100 * $delta / $dt, 1); WS_MB = [math]::Round($p.WorkingSet64 / 1MB, 0); Private_MB = [math]::Round($p.PrivateMemorySize64 / 1MB, 0) }
  $rows += $row
  if (-not $Quiet) { $row | Format-Table -HideTableHeaders -AutoSize | Out-String -Width 120 | Write-Host -NoNewline }
}
$valid = $rows | Select-Object -Skip 1   # first interval has no CPU baseline
if ($valid) {
  $m = $valid | Measure-Object CPU_pct -Minimum -Maximum -Average
  $w = $valid | Measure-Object WS_MB -Maximum -Average
  "SUMMARY ({0} samples, {1} logical cores; CPU is % of one core): CPU avg {2:N1} / max {3:N1} · working set avg {4:N0} MB / max {5:N0} MB" -f $valid.Count, $cores, $m.Average, $m.Maximum, $w.Average, $w.Maximum
}
