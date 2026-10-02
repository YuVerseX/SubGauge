param(
    [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$ApplicationProcessId,
    [ValidateRange(1,240)][int]$Minutes = 60,
    [ValidateRange(5,60)][int]$IntervalSeconds = 5,
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
if (-not $OutputPath) {
    $repo = Split-Path $PSScriptRoot -Parent
    $OutputPath = Join-Path $repo ('release\validation\resources-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.csv')
}
if (Test-Path -LiteralPath $OutputPath) { throw 'Choose a new output file; existing observations are preserved.' }
$parentDirectory = Split-Path $OutputPath -Parent
New-Item -ItemType Directory -Force -Path $parentDirectory | Out-Null
$application = Get-Process -Id $ApplicationProcessId
if ($application.ProcessName -ne 'subgauge') { throw 'The process must be SubGauge.' }
$started = Get-Date
$deadline = $started.AddMinutes($Minutes)
$previousCpu = @{}
$previousTime = $started
$samples = 0
$logicalProcessors = [Environment]::ProcessorCount
while ((Get-Date) -le $deadline) {
    if (-not (Get-Process -Id $ApplicationProcessId -ErrorAction SilentlyContinue)) { break }
    # Follow only SubGauge descendants, including its WebView2 subprocesses.
    $processTree = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId)
    $selectedIds = [System.Collections.Generic.HashSet[int]]::new()
    [void]$selectedIds.Add($ApplicationProcessId)
    do {
        $changed = $false
        foreach ($entry in $processTree) {
            if ($selectedIds.Contains([int]$entry.ParentProcessId)) {
                $changed = $selectedIds.Add([int]$entry.ProcessId) -or $changed
            }
        }
    } while ($changed)
    $now = Get-Date
    $elapsed = ($now - $previousTime).TotalSeconds
    $processes = @($selectedIds | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    $cpuDelta = 0.0
    $nextCpu = @{}
    foreach ($process in $processes) {
        $key = [string]$process.Id
        $seconds = $process.TotalProcessorTime.TotalSeconds
        if ($previousCpu.ContainsKey($key)) { $cpuDelta += [Math]::Max(0.0,$seconds - [double]$previousCpu[$key]) }
        $nextCpu[$key] = $seconds
    }
    $root = $processes | Where-Object Id -eq $ApplicationProcessId | Select-Object -First 1
    $row = [pscustomobject]@{
        Timestamp = $now.ToString('o')
        ElapsedSeconds = [Math]::Round(($now - $started).TotalSeconds,1)
        ProcessCount = $processes.Count
        RootWorkingSetMB = [Math]::Round($root.WorkingSet64 / 1MB,2)
        RootPrivateMB = [Math]::Round($root.PrivateMemorySize64 / 1MB,2)
        TreeWorkingSetMB = [Math]::Round(($processes | Measure-Object WorkingSet64 -Sum).Sum / 1MB,2)
        TreePrivateMB = [Math]::Round(($processes | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB,2)
        CpuOneCorePercent = if ($samples -gt 0 -and $elapsed -gt 0) { [Math]::Round(100 * $cpuDelta / $elapsed,3) } else { 0 }
        CpuMachinePercent = if ($samples -gt 0 -and $elapsed -gt 0) { [Math]::Round(100 * $cpuDelta / $elapsed / $logicalProcessors,3) } else { 0 }
    }
    $row | Export-Csv -LiteralPath $OutputPath -Append -NoTypeInformation -Encoding UTF8
    $samples++
    $previousCpu = $nextCpu
    $previousTime = $now
    Start-Sleep -Seconds $IntervalSeconds
}
[pscustomobject]@{ Samples=$samples; ElapsedMinutes=[Math]::Round(((Get-Date)-$started).TotalMinutes,2); OutputPath=$OutputPath }
