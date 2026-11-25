# Check C1 (launch time), measured many times: runs examples/purge_shape and examples/bare_window
# with --first-frame, one after the other, -Runs times each, and prints for each the median and the
# range (least to most) of two times. Build both first, then run it on the host with Windows
# PowerShell 5.1 or PowerShell 7:
#
#   cargo build --release -p mdshared-ui --features a11y --example purge_shape --example bare_window
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests\uia\first_frame.ps1 -Exe <path to purge_shape.exe>
#
# Arguments: -Exe, the path to purge_shape.exe (needed); -BareExe, the path to bare_window.exe
# (looked for beside purge_shape.exe when not given); -Runs, how many runs of each (default 10).
#
# The two times of a run:
# - "first frame": what the example prints, from the start of its main to the first frame drawn
#   and presented into the window while it is still hidden, and the window asked to show. It
#   leaves out the process's start before main, and the moment the desktop compositor puts the
#   shown window on screen, which no call in the process waits for.
# - "launch to exit": the script's clock from starting the process to its end, the first frame
#   and the teardown after it included. An upper bound, for comparing the two examples.
# purge_shape builds its whole accessibility tree (2,000 rows) and makes the UIA adapter before its
# first frame; no UIA client is connected during these runs.
#
# The runs alternate (bare, purge, bare, purge, ...) so that a change of the machine's load while
# it runs falls on both. A run that prints no time fails the script (exit 1); otherwise it exits 0:
# C1 has no pass mark here, only numbers (see docs/ROADMAP.md).

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$BareExe = '',
    [ValidateRange(1, 1000)][int]$Runs = 10
)

$ErrorActionPreference = 'Stop'
if ($BareExe -eq '') { $BareExe = Join-Path (Split-Path $Exe) 'bare_window.exe' }
foreach ($e in @($Exe, $BareExe)) {
    if (-not (Test-Path $e)) { Write-Output "FAIL not built: $e"; exit 1 }
}

# One run of $path with --first-frame: its printed first-frame time and the launch-to-exit time,
# both in ms, or $null for the first when it printed none.
function RunOnce([string]$path) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $path --first-frame 2>&1 | Out-String
    $wall = $sw.Elapsed.TotalMilliseconds
    $frame = $null
    $m = [regex]::Match($out, 'first frame after ([0-9]+(\.[0-9]+)?) ms')
    if ($m.Success) { $frame = [double]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture) }
    @{ Frame = $frame; Wall = $wall }
}

function Median([double[]]$xs) {
    $s = @($xs | Sort-Object)
    $n = $s.Count
    if ($n % 2 -eq 1) { $s[($n - 1) / 2] } else { ($s[$n / 2 - 1] + $s[$n / 2]) / 2 }
}

function Summary([string]$what, [double[]]$xs) {
    $s = @($xs | Sort-Object)
    '{0,-28} median {1,7:N1} ms, range {2,7:N1} to {3,7:N1} ms' -f $what, (Median $xs), $s[0], $s[-1]
}

$times = @{ bare = @{ Frame = @(); Wall = @() }; purge = @{ Frame = @(); Wall = @() } }
$missing = 0
# One untimed run of each first, so that the files are in the system's cache for every timed run.
$null = RunOnce $BareExe
$null = RunOnce $Exe
for ($i = 1; $i -le $Runs; $i++) {
    foreach ($k in @('bare', 'purge')) {
        $path = if ($k -eq 'bare') { $BareExe } else { $Exe }
        $r = RunOnce $path
        if ($null -eq $r.Frame) { $missing++; Write-Output "FAIL run $i of $k printed no first-frame time" }
        else { $times[$k].Frame += $r.Frame }
        $times[$k].Wall += $r.Wall
        Write-Output ('run {0,3} {1,-5} first frame {2,7} ms, launch to exit {3,7:N1} ms' -f $i, $k, $(if ($null -ne $r.Frame) { '{0:N1}' -f $r.Frame } else { '-' }), $r.Wall)
    }
}
Write-Output "C1 over $Runs runs of each, alternately:"
foreach ($k in @('bare', 'purge')) {
    $name = if ($k -eq 'bare') { 'bare_window' } else { 'purge_shape' }
    if ($times[$k].Frame.Count -gt 0) { Write-Output (Summary "$name first frame" $times[$k].Frame) }
    Write-Output (Summary "$name launch to exit" $times[$k].Wall)
}
if ($missing -gt 0) { exit 1 }
exit 0
