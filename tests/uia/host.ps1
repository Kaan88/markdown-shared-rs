# UI Automation probe of `examples/host.rs` (the window host's a11y flow, W1 with W8).
# Build: cargo build --release -p mdshared-ui --features host,a11y --example host
# Run:   powershell -File tests/uia/host.ps1 -Exe <target>/release/examples/host.exe
# Finds the window (leaving out winit's hidden helper window), the Hello button and the field
# with their bounds, and invokes the button. Needs a desktop; not run by CI.
param([string]$Exe)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$out = Join-Path $env:TEMP 'host_probe_out.txt'
$p = Start-Process -FilePath $Exe -ArgumentList '--seconds','8' -PassThru -RedirectStandardOutput $out -RedirectStandardError (Join-Path $env:TEMP 'host_probe_err.txt')
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.NotCondition((New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Winit Thread Event Target')))))
$win = $null
$sw = [Diagnostics.Stopwatch]::StartNew()
while (-not $win -and $sw.ElapsedMilliseconds -lt 5000) {
    $win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
    if (-not $win) { Start-Sleep -Milliseconds 50 }
}
if (-not $win) { Write-Output 'FAIL no window'; exit 1 }
Write-Output ("window found after {0} ms: '{1}'" -f $sw.ElapsedMilliseconds, $win.Current.Name)
$idc = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'Hello')
$btn = $null
$sw.Restart()
while (-not $btn -and $sw.ElapsedMilliseconds -lt 3000) {
    $btn = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $idc)
    if (-not $btn) { Start-Sleep -Milliseconds 50 }
}
if (-not $btn) { Write-Output 'FAIL no Hello button' } else {
Write-Output ("button '{0}' bounds {1}" -f $btn.Current.Name, $btn.Current.BoundingRectangle)
$fc = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'Field')
$field = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $fc)
if ($field) { Write-Output ("field '{0}' bounds {1}" -f $field.Current.Name, $field.Current.BoundingRectangle) } else { Write-Output 'FAIL no field' }
$inv = $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
$inv.Invoke()
Start-Sleep -Milliseconds 400
}
$wb = $win.Current.BoundingRectangle
Write-Output ("window bounds {0}" -f $wb)
$p.WaitForExit(12000) | Out-Null
$log = Get-Content $out
$hit = $log | Select-String 'a11y action Click'
if ($hit) { Write-Output "PASS invoke reached the app: $hit" } else { Write-Output 'FAIL invoke not seen'; $log | Select-Object -Last 10 }
Get-Content (Join-Path $env:TEMP 'host_probe_err.txt') | Select-Object -First 12
