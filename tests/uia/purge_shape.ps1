# A UI Automation client run against examples/purge_shape (roadmap W8 and check C2): it finds
# the window's elements by AutomationId and drives them the way Purge's VM rig drives WinUI
# (vm/guest/m3-e2e-ui.ps1 in Purge). Run it with Windows PowerShell on the host:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tests\uia\purge_shape.ps1 -Exe <path to purge_shape.exe>
#
# It prints one PASS or FAIL line per check and exits with the number of failures.

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$FarRow = 1500
)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$AE = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$failures = 0

function Check([string]$name, [bool]$ok, [string]$detail = '') {
    if ($ok) { Write-Output "PASS $name $detail" } else { Write-Output "FAIL $name $detail"; $script:failures++ }
}

function ById($root, [string]$id) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($AE::AutomationIdProperty, $id)
    $root.FindFirst($TS::Descendants, $cond)
}

function ByName($root, [string]$name) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $name)
    $root.FindFirst($TS::Descendants, $cond)
}

function Pattern($el, $pattern) {
    $p = $null
    if ($el.TryGetCurrentPattern($pattern, [ref]$p)) { $p } else { $null }
}

function Footer($win) {
    $f = ById $win 'FooterCount'
    if ($null -eq $f) { return '' }
    $v = Pattern $f ([System.Windows.Automation.ValuePattern]::Pattern)
    if ($v) { $v.Current.Value } else { $f.Current.Name }
}

# UIA_PositionInSetPropertyId and UIA_SizeOfSetPropertyId (Windows 10 1809) are unknown to the
# managed client, so they are read through the COM client. Its interop assembly is made once from
# the type library inside UIAutomationCore.dll (what tlbimp does) and kept in %TEMP%.
$interop = Join-Path $env:TEMP 'mdshared-uia\Interop.UIAutomationClient.dll'
if (-not (Test-Path $interop)) {
    New-Item -ItemType Directory -Force (Split-Path $interop) | Out-Null
    Add-Type -TypeDefinition @'
using System;
using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.InteropServices;
public static class UiaTlb {
    [DllImport("oleaut32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    static extern void LoadTypeLibEx(string file, int kind, [MarshalAs(UnmanagedType.Interface)] out object lib);
    class Sink : ITypeLibImporterNotifySink {
        public void ReportEvent(ImporterEventKind k, int c, string m) {}
        public Assembly ResolveRef(object t) { return null; }
    }
    public static void Make(string path) {
        object lib;
        LoadTypeLibEx(Environment.SystemDirectory + "\\UIAutomationCore.dll", 2, out lib);
        var asm = new TypeLibConverter().ConvertTypeLibToAssembly(lib, path, TypeLibImporterFlags.None, new Sink(), null, null, "Interop.UIAutomationClient", null);
        asm.Save(System.IO.Path.GetFileName(path));
    }
}
'@
    Push-Location (Split-Path $interop); try { [UiaTlb]::Make($interop) } finally { Pop-Location }
}
Add-Type -ReferencedAssemblies $interop -TypeDefinition @'
using System;
using Interop.UIAutomationClient;
public static class UiaSet {
    // { PositionInSet, SizeOfSet } of the element with this AutomationId, or null.
    public static int[] Get(IntPtr hwnd, string id) {
        var uia = new CUIAutomation();
        var root = uia.ElementFromHandle(hwnd);
        var el = root.FindFirst(TreeScope.TreeScope_Descendants, uia.CreatePropertyCondition(30011, id));
        if (el == null) return null;
        return new int[] { (int)el.GetCurrentPropertyValue(30152), (int)el.GetCurrentPropertyValue(30153) };
    }
}
'@
[Reflection.Assembly]::LoadFrom($interop) | Out-Null
function ComSet($hwnd, [string]$id) {
    $r = [UiaSet]::Get($hwnd, $id)
    if ($null -eq $r) { return $null }
    @{ Position = $r[0]; Size = $r[1] }
}

$ErrorActionPreference = 'Stop'
$proc = Start-Process -FilePath $Exe -PassThru
try {
    # The window by its process and title: the example is a console program, so the process's
    # MainWindowHandle is at first its console window, not this one.
    $cond = New-Object System.Windows.Automation.AndCondition(
        (New-Object System.Windows.Automation.PropertyCondition($AE::ProcessIdProperty, $proc.Id)),
        (New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, 'Purge shape')))
    $win = $null
    for ($i = 0; $i -lt 100 -and $null -eq $win; $i++) {
        Start-Sleep -Milliseconds 100
        $win = $AE::RootElement.FindFirst($TS::Children, $cond)
    }
    $hwnd = if ($win) { [IntPtr]$win.Current.NativeWindowHandle } else { [IntPtr]::Zero }
    Check 'window' ($null -ne $win) "pid $($proc.Id)"
    if ($null -eq $win) { exit 1 }

    # Find by AutomationId and by name.
    $button = ById $win 'UninstallButton'
    Check 'find by AutomationId' ($null -ne $button) 'UninstallButton'
    Check 'find by name' ($null -ne (ByName $win 'Show components')) 'Show components'

    # Invoke.
    $inv = Pattern $button ([System.Windows.Automation.InvokePattern]::Pattern)
    Check 'Invoke pattern' ($null -ne $inv)
    if ($inv) { $inv.Invoke(); Start-Sleep -Milliseconds 300 }
    $f = Footer $win
    Check 'Invoke acted' ($f -match 'pressed 1 times') "footer: $f"

    # Toggle.
    $check = ById $win 'ShowComponents'
    $tog = Pattern $check ([System.Windows.Automation.TogglePattern]::Pattern)
    Check 'Toggle pattern' ($null -ne $tog)
    if ($tog) {
        $before = $tog.Current.ToggleState
        $tog.Toggle(); Start-Sleep -Milliseconds 300
        $after = (Pattern (ById $win 'ShowComponents') ([System.Windows.Automation.TogglePattern]::Pattern)).Current.ToggleState
        Check 'Toggle acted' ($before -eq 'Off' -and $after -eq 'On') "$before -> $after"
    }

    # The list: only some rows are realized, and each says where it is in all 2,000 (UIA has the
    # count on the items, PositionInSet and SizeOfSet, not on the list).
    $list = ById $win 'ProgramList'
    Check 'list found' ($null -ne $list)
    $kids = $list.FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
    Check 'only some rows realized' ($kids.Count -gt 0 -and $kids.Count -lt 2000) "$($kids.Count)"
    $row5 = ById $list 'Row4'
    if ($row5) {
        $rs = ComSet $hwnd 'Row4'
        Check 'row position of size' ($rs.Position -eq 5 -and $rs.Size -eq 2000) "row 5: $($rs.Position) of $($rs.Size), name '$($row5.Current.Name)'"
        # SelectionItem.
        $si = Pattern $row5 ([System.Windows.Automation.SelectionItemPattern]::Pattern)
        Check 'SelectionItem pattern' ($null -ne $si)
        if ($si) {
            $si.Select(); Start-Sleep -Milliseconds 300
            $sel = (Pattern (ById $list 'Row4') ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected
            Check 'SelectionItem acted' $sel "footer: $(Footer $win)"
        }
        $sp = Pattern $list ([System.Windows.Automation.SelectionPattern]::Pattern)
        Check 'Selection pattern on list' ($null -ne $sp) "CanSelectMultiple=$(if ($sp) { $sp.Current.CanSelectMultiple })"
    } else {
        Check 'row 5 realized' $false
    }

    # ScrollItem: walk to a far row by scrolling the last realized row into view, then scroll the
    # far row itself into view and select it.
    $target = "Row$($FarRow - 1)"
    $far = ById $list $target
    $steps = 0
    while ($null -eq $far -and $steps -lt 200) {
        $kids = $list.FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
        $last = $kids[$kids.Count - 1]
        $sc = Pattern $last ([System.Windows.Automation.ScrollItemPattern]::Pattern)
        if ($null -eq $sc) { break }
        $sc.ScrollIntoView(); Start-Sleep -Milliseconds 150
        $steps++
        $far = ById $list $target
    }
    Check 'far row realized by ScrollItem steps' ($null -ne $far) "$target after $steps steps"
    if ($far) {
        $sc = Pattern $far ([System.Windows.Automation.ScrollItemPattern]::Pattern)
        Check 'ScrollItem pattern' ($null -ne $sc)
        $sc.ScrollIntoView(); Start-Sleep -Milliseconds 300
        $far = ById $list $target
        $lb = $list.Current.BoundingRectangle
        $rb = $far.Current.BoundingRectangle
        $inView = ($rb.Top -ge $lb.Top - 1) -and ($rb.Bottom -le $lb.Bottom + 1)
        Check 'far row scrolled into view' $inView "row $($rb.Top)-$($rb.Bottom), list $($lb.Top)-$($lb.Bottom)"
        $si = Pattern $far ([System.Windows.Automation.SelectionItemPattern]::Pattern)
        if ($si) { $si.Select(); Start-Sleep -Milliseconds 300 }
        $sel = (Pattern (ById $list $target) ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected
        Check 'far row selected' $sel "footer: $(Footer $win)"
    }

    # RangeValue.
    $prog = ById $win 'SelectedProgress'
    $rv = Pattern $prog ([System.Windows.Automation.RangeValuePattern]::Pattern)
    Check 'RangeValue pattern' ($null -ne $rv) "value=$(if ($rv) { $rv.Current.Value }) of $(if ($rv) { $rv.Current.Maximum })"
}
catch {
    Write-Output "FAIL script error: $($_.Exception.Message) at line $($_.InvocationInfo.ScriptLineNumber)"
    $failures++
}
finally {
    if (-not $proc.HasExited) { $proc.Kill() }
}
Write-Output "failures: $failures"
exit $failures
