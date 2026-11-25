# A UI Automation client run against examples/purge_shape (roadmap W8 and checks C1 to C3): it
# finds the window's elements by AutomationId and drives them (Invoke, Toggle, SelectionItem,
# ScrollItem, ExpandCollapse, Value, RangeValue) the way Purge's VM rig drives WinUI
# (vm/guest/m3-e2e-ui.ps1 in Purge). Build both examples, then run it on the host:
#
#   cargo build --release -p mdshared-ui --features a11y --example purge_shape --example bare_window
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests\uia\purge_shape.ps1 -Exe <path to purge_shape.exe>
#
# It needs Windows PowerShell 5.1 (powershell.exe, on .NET Framework), not PowerShell 7 (pwsh):
# it makes the COM interop assembly for the UIA client with TypeLibConverter and saves it with
# AssemblyBuilder.Save, which .NET Framework has and .NET does not. It writes that assembly once
# to %TEMP%\mdshared-uia\Interop.UIAutomationClient.dll and reuses it in later runs (delete the
# folder to have it made again).
#
# Arguments: -Exe, the path to purge_shape.exe (needed); -BareExe, the path to bare_window.exe
# (looked for beside purge_shape.exe when not given); -FarRow, the 1-based number of the program
# the ScrollItem and Value sections use, which must lie beyond the first screenful (default
# 1500, from 40 to 2000). The Value section searches for that program's name less its last two
# digits ("Program 15" for 1500) and works out how many programs that shows.
#
# For check C1 it prints the time from Start-Process to the bare window found by its title, and
# to purge_shape's names visible (the first time UninstallButton is found by its AutomationId),
# one run each; tests\uia\first_frame.ps1 runs both examples many times for the medians. For
# check C3 it prints purge_shape.exe's size and fails when it is 10 MB or more.
#
# It also listens for UIA's focus-changed events (Purge's B1), moves the focus with SetFocus and
# then with a Tab typed into the window, and checks that each move raised one. The Tab needs the
# window in the foreground: the script asks for it (an Alt press, then SetForegroundWindow), so
# do not use the desktop while it runs.
#
# Nothing waits a fixed time: after each action the script polls for the state it expects, up to
# a deadline (WaitFor). Each section has its own try/catch, so an exception fails that section
# and the rest still run. It prints one PASS or FAIL line per check, then how many checks ran
# against how many there are, and exits with the number of failures (a section that stopped
# early, leaving checks unrun, counts as one more).

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$BareExe = '',
    [ValidateRange(40, 2000)][int]$FarRow = 1500
)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase
$AE = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$P = @{
    Toggle    = [System.Windows.Automation.TogglePattern]::Pattern
    Select    = [System.Windows.Automation.SelectionItemPattern]::Pattern
    Selection = [System.Windows.Automation.SelectionPattern]::Pattern
    Scroll    = [System.Windows.Automation.ScrollItemPattern]::Pattern
    Range     = [System.Windows.Automation.RangeValuePattern]::Pattern
    Invoke    = [System.Windows.Automation.InvokePattern]::Pattern
    Window    = [System.Windows.Automation.WindowPattern]::Pattern
    Expand    = [System.Windows.Automation.ExpandCollapsePattern]::Pattern
    Value     = [System.Windows.Automation.ValuePattern]::Pattern
}
# Every check below, counted by hand; the script says so when fewer ran.
$Expected = 50
$script:checks = 0
$script:failures = 0

function Check([string]$name, [bool]$ok, [string]$detail = '') {
    $script:checks++
    if ($ok) { Write-Output "PASS $name $detail" } else { Write-Output "FAIL $name $detail"; $script:failures++ }
}

function SectionFailed([string]$section, $err) {
    Write-Output "FAIL $section stopped: $($err.Exception.Message) at line $($err.InvocationInfo.ScriptLineNumber)"
    $script:failures++
}

# Runs $Cond until it gives something true, or $TimeoutMs has passed; returns its last answer.
# An exception inside counts as not yet (an element that went away between two calls).
function WaitFor([scriptblock]$Cond, [int]$TimeoutMs = 3000, [int]$IntervalMs = 20) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
        $r = $null
        try { $r = & $Cond } catch { $r = $null }
        if ($r) { return $r }
        if ($sw.ElapsedMilliseconds -ge $TimeoutMs) { return $r }
        Start-Sleep -Milliseconds $IntervalMs
    }
}

function ById($root, [string]$id) {
    if ($null -eq $root) { return $null }
    $cond = New-Object System.Windows.Automation.PropertyCondition($AE::AutomationIdProperty, $id)
    $root.FindFirst($TS::Descendants, $cond)
}

function ByName($root, [string]$name) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $name)
    $root.FindFirst($TS::Descendants, $cond)
}

function Pattern($el, $pattern) {
    $p = $null
    if ($null -ne $el -and $el.TryGetCurrentPattern($pattern, [ref]$p)) { $p } else { $null }
}

# A top-level window of process $procId with this title. A winit process has a second top-level
# window, the hidden helper of class 'Winit Thread Event Target' with no name; the title already
# leaves it out, and the class is left out as well, so that a rig that drops the title (or a
# window whose title is empty) still never takes the helper for the window.
$HelperClass = 'Winit Thread Event Target'
function FindWindow([int]$procId, [string]$title) {
    $cond = New-Object System.Windows.Automation.AndCondition(
        (New-Object System.Windows.Automation.PropertyCondition($AE::ProcessIdProperty, $procId)),
        (New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $title)),
        (New-Object System.Windows.Automation.NotCondition(
            (New-Object System.Windows.Automation.PropertyCondition($AE::ClassNameProperty, $HelperClass)))))
    $AE::RootElement.FindFirst($TS::Children, $cond)
}

# Every top-level window of process $procId, the helper included: for the check that the helper
# is there and is not what FindWindow gives.
function ProcessWindows([int]$procId) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($AE::ProcessIdProperty, $procId)
    @($AE::RootElement.FindAll($TS::Children, $cond))
}

# The footer is a Text element (AccessKit's Label): its text is its Name, with no Value pattern.
function Footer($win) {
    $f = ById $win 'FooterCount'
    if ($null -eq $f) { '' } else { $f.Current.Name }
}

function IsSelected($list, [string]$id) {
    $si = Pattern (ById $list $id) $P.Select
    ($null -ne $si) -and $si.Current.IsSelected
}

# Whether element $el lies within the list's viewport. A row with no bounds (scrolled far away)
# has an empty rectangle, which counts as out.
function InView($el, $list) {
    $rb = $el.Current.BoundingRectangle
    $lb = $list.Current.BoundingRectangle
    if ($rb.IsEmpty -or $rb.Width -le 0 -or $rb.Height -le 0) { return $false }
    ($rb.Top -ge $lb.Top - 1) -and ($rb.Bottom -le $lb.Bottom + 1)
}

# UIA_PositionInSetPropertyId, UIA_SizeOfSetPropertyId (Windows 10 1809) and
# UIA_FullDescriptionPropertyId (1703) are unknown to the managed client, so they are read through
# the COM client. Its interop assembly is made once from the type library inside UIAutomationCore.dll
# (what tlbimp does) and kept in %TEMP%.
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
public static class UiaCom {
    // The properties `ids` of the element with this AutomationId, or null when it is not found.
    public static object[] Get(IntPtr hwnd, string id, int[] ids) {
        var uia = new CUIAutomation();
        var root = uia.ElementFromHandle(hwnd);
        var el = root.FindFirst(TreeScope.TreeScope_Descendants, uia.CreatePropertyCondition(30011, id));
        if (el == null) return null;
        var r = new object[ids.Length];
        for (int i = 0; i < ids.Length; i++) r[i] = el.GetCurrentPropertyValue(ids[i]);
        return r;
    }
}
'@
[Reflection.Assembly]::LoadFrom($interop) | Out-Null
function ComSet($hwnd, [string]$id) {
    $r = [UiaCom]::Get($hwnd, $id, [int[]](30152, 30153))
    if ($null -eq $r) { return $null }
    @{ Position = [int]$r[0]; Size = [int]$r[1] }
}
function ComDescription($hwnd, [string]$id) {
    $r = [UiaCom]::Get($hwnd, $id, [int[]](30159))
    if ($null -eq $r) { $null } else { [string]$r[0] }
}

# UIA focus-changed events (Purge's B1: "focus changes raise UIA focus events"). UIA calls the
# handler on its own thread, where a PowerShell scriptblock is not safe, so the handler is C#: it
# queues "pid:AutomationId" of each element that takes the focus. The same class brings a window
# to the foreground and types keys into it with SendInput.
$refs = @(
    [System.Windows.Automation.Automation].Assembly.Location,
    [System.Windows.Automation.AutomationElement].Assembly.Location,
    [System.Windows.Automation.TreeScope].Assembly.Location,
    [System.Windows.Point].Assembly.Location
) | Select-Object -Unique
Add-Type -ReferencedAssemblies $refs -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Windows.Automation;
public static class UiaFocus {
    static readonly ConcurrentQueue<string> seen = new ConcurrentQueue<string>();
    static AutomationFocusChangedEventHandler handler;
    public static void Start() {
        handler = (sender, e) => {
            var el = sender as AutomationElement;
            string s = "?";
            try { s = el.Current.ProcessId + ":" + el.Current.AutomationId; } catch (Exception) { }
            seen.Enqueue(s);
        };
        Automation.AddAutomationFocusChangedEventHandler(handler);
    }
    public static void Stop() {
        if (handler != null) Automation.RemoveAutomationFocusChangedEventHandler(handler);
        handler = null;
    }
    public static string[] Seen() { return seen.ToArray(); }
    public static void Clear() { string s; while (seen.TryDequeue(out s)) { } }

    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    // INPUT is a type and a union whose largest member (MOUSEINPUT) is 32 bytes on x64; the
    // padding keeps the size SendInput checks.
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public KEYBDINPUT ki; public long pad; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    const uint KEYUP = 2;

    // Asks for the window to come to the front. Windows lets a process take the foreground only
    // in some cases; a press of Alt first is the usual way for a script to qualify. Answers
    // whether the window is in front afterwards.
    public static bool Front(IntPtr hwnd) {
        if (GetForegroundWindow() == hwnd) return true;
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYUP, UIntPtr.Zero);
        SetForegroundWindow(hwnd);
        System.Threading.Thread.Sleep(50);
        return GetForegroundWindow() == hwnd;
    }
    public static bool IsFront(IntPtr hwnd) { return GetForegroundWindow() == hwnd; }

    // A press and release of the virtual key `vk`; answers how many inputs Windows took (2).
    public static uint Key(ushort vk) {
        var down = new INPUT { type = 1, ki = new KEYBDINPUT { wVk = vk } };
        var up = new INPUT { type = 1, ki = new KEYBDINPUT { wVk = vk, dwFlags = KEYUP } };
        return SendInput(2, new[] { down, up }, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

$ErrorActionPreference = 'Stop'

# --- C3: the size of the release exe, under 10 MB.
try {
    $bytes = (Get-Item $Exe).Length
    $mb = $bytes / 1MB
    Check 'C3 purge_shape.exe under 10 MB' ($bytes -lt 10MB) ("{0:N0} bytes ({1:N2} MB)" -f $bytes, $mb)
}
catch { SectionFailed 'C3 size' $_ }

# The first UIA call of a process loads and starts the client; done here so the timings below
# measure the window, not the client.
$null = $AE::RootElement.FindFirst($TS::Children, [System.Windows.Automation.Condition]::FalseCondition)

# --- C1 reference: the bare window (winit and softbuffer only), found by its title.
if ($BareExe -eq '') { $BareExe = Join-Path (Split-Path $Exe) 'bare_window.exe' }
try {
    $exists = Test-Path $BareExe
    $bare = $null
    $ms = -1
    if ($exists) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $bp = Start-Process -FilePath $BareExe -PassThru
        try {
            $bare = WaitFor { FindWindow $bp.Id 'Bare window' } -TimeoutMs 10000 -IntervalMs 2
            $ms = $sw.ElapsedMilliseconds
        }
        finally { if (-not $bp.HasExited) { $bp.Kill() } }
    }
    Check 'bare window found' ($null -ne $bare) "$BareExe$(if (-not $exists) { ' (not built)' })"
    if ($bare) { Write-Output "bare window visible after $ms ms" }
}
catch { SectionFailed 'bare window' $_ }

$sw = [Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $Exe -PassThru
try {
    # --- C1: from Start-Process to the names visible. The window by its process and title: the
    # example is a console program, so the process's MainWindowHandle is at first its console
    # window, not this one.
    $win = $null
    $hwnd = [IntPtr]::Zero
    $button = $null
    try {
        $button = WaitFor { ById (FindWindow $proc.Id 'Purge shape') 'UninstallButton' } -TimeoutMs 10000 -IntervalMs 2
        $ms = $sw.ElapsedMilliseconds
        $win = FindWindow $proc.Id 'Purge shape'
        if ($win) { $hwnd = [IntPtr]$win.Current.NativeWindowHandle }
        Check 'window' ($null -ne $win) "pid $($proc.Id)"
        $all = ProcessWindows $proc.Id
        $classes = @($all | ForEach-Object { "'$($_.Current.ClassName)' named '$($_.Current.Name)'" })
        $helper = @($all | Where-Object { $_.Current.ClassName -eq $HelperClass })
        Check 'the window, not the helper' ($null -ne $win -and $win.Current.ClassName -ne $HelperClass) "top-level windows of the process: $($classes -join '; '); helpers: $($helper.Count)"
        Check 'find by AutomationId' ($null -ne $button) 'UninstallButton'
        if ($button) { Write-Output "names visible after $ms ms" }
        Check 'find by name' ($null -ne (ByName $win 'Show components')) 'Show components'
        $hint = (ById $win 'SearchBox').Current.HelpText
        Check 'search field says its hint' ($hint -eq 'Search (Ctrl+F)') "HelpText '$hint'"
        $cols = ById $win 'ProgramColumns'
        $names = @('NameColumn', 'SizeColumn') | ForEach-Object { (ById $cols $_).Current.Name }
        Check 'column headers' (($names -join ',') -eq 'Name,Size') "'$($names -join "', '")'"
    }
    catch { SectionFailed 'launch' $_ }
    if ($null -eq $win) {
        $state = if ($proc.HasExited) { "exited with code $($proc.ExitCode)" } else { 'still running' }
        Write-Output "FAIL no window after $($sw.ElapsedMilliseconds) ms; the process $state"
        Write-Output "checks: $script:checks of $Expected run; failures: $($script:failures + 1)"
        exit ($script:failures + 1)
    }

    # --- Toggle.
    try {
        $tog = Pattern (ById $win 'ShowComponents') $P.Toggle
        Check 'Toggle pattern' ($null -ne $tog)
        $before = $tog.Current.ToggleState
        $tog.Toggle()
        $after = WaitFor { $s = (Pattern (ById $win 'ShowComponents') $P.Toggle).Current.ToggleState; if ($s -eq 'On') { $s } }
        Check 'Toggle acted' ($before -eq 'Off' -and $after -eq 'On') "$before -> $after"
    }
    catch { SectionFailed 'Toggle' $_ }

    # --- The list: every row is in the tree, and each says where it is in all 2,000 (UIA has the
    # count on the items, PositionInSet and SizeOfSet).
    $list = $null
    try {
        $list = ById $win 'ProgramList'
        Check 'list found' ($null -ne $list)
        $kids = $list.FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
        Check 'every row in the tree' ($kids.Count -eq 2000) "$($kids.Count)"
        $sp = Pattern $list $P.Selection
        Check 'Selection pattern on list, multiple' ($null -ne $sp -and $sp.Current.CanSelectMultiple) "CanSelectMultiple=$(if ($sp) { $sp.Current.CanSelectMultiple })"
        $row5 = ById $list 'Row4'
        $rs = ComSet $hwnd 'Row4'
        Check 'row position of size' ($rs.Position -eq 5 -and $rs.Size -eq 2000) "row 5: $($rs.Position) of $($rs.Size), name '$($row5.Current.Name)'"
        # The row's name is its two columns: the program and its size (5 * 37 % 900 + 12 for
        # program 5, whose index is 4).
        Check 'a row is named with its size' ($row5.Current.Name -eq 'Program 0005, 160 MB') "'$($row5.Current.Name)'"
        $si = Pattern $row5 $P.Select
        Check 'SelectionItem pattern' ($null -ne $si)
        $si.Select()
        $sel = WaitFor { IsSelected $list 'Row4' }
        Check 'SelectionItem acted' $sel "footer: $(Footer $win)"

        # Select on a selected row keeps it: AccessKit sends nothing then. RemoveFromSelection
        # comes to the window as the same click as Select, which the window ignores on a selected
        # row (AddToSelection and RemoveFromSelection are not supported). The toggle after them
        # is a barrier: the window handles requests in order, so once it acted, so had they.
        (Pattern (ById $list 'Row4') $P.Select).Select()
        (Pattern (ById $list 'Row4') $P.Select).RemoveFromSelection()
        (Pattern (ById $win 'ShowComponents') $P.Toggle).Toggle()
        $off = WaitFor { (Footer $win) -match 'components off' }
        $f = Footer $win
        Check 'Select on a selected row keeps it' ($off -and (IsSelected $list 'Row4') -and $f -match '^1 selected') "footer: $f"
    }
    catch { SectionFailed 'list' $_ }

    # --- ScrollItem: a far row is found directly, scrolled into view and selected.
    try {
        $target = "Row$($FarRow - 1)"
        $far = ById $list $target
        Check 'far row found' ($null -ne $far) $target
        $sc = Pattern $far $P.Scroll
        Check 'ScrollItem pattern' ($null -ne $sc)
        $rb0 = $far.Current.BoundingRectangle
        $wasOut = -not (InView $far $list)
        $sc.ScrollIntoView()
        $inView = WaitFor { InView (ById $list $target) $list }
        $rb = (ById $list $target).Current.BoundingRectangle
        $lb = $list.Current.BoundingRectangle
        Check 'far row scrolled into view' ($wasOut -and $inView) "out before: $wasOut ($rb0); row $($rb.Top)-$($rb.Bottom), list $($lb.Top)-$($lb.Bottom)"
        # A row scrolled away above the list has no bounds, so the toolbar keeps its hits.
        $bb = (ById $win 'UninstallButton').Current.BoundingRectangle
        $pt = New-Object System.Windows.Point(($bb.Left + $bb.Width / 2), ($bb.Top + $bb.Height / 2))
        $hit = $AE::FromPoint($pt)
        Check 'the button, not a row, is at its own point' ($hit.Current.AutomationId -eq 'UninstallButton') "at $pt`: '$($hit.Current.AutomationId)' $($hit.Current.ControlType.ProgrammaticName)"
        (Pattern (ById $list $target) $P.Select).Select()
        $sel = WaitFor { IsSelected $list $target }
        $f = Footer $win
        Check 'far row selected, only it' ($sel -and -not (IsSelected $list 'Row4') -and $f -match '^1 selected') "footer: $f"
    }
    catch { SectionFailed 'ScrollItem' $_ }

    # --- RangeValue: one program of 2,000 selected is 0.05 percent.
    try {
        $rv = Pattern (ById $win 'SelectedProgress') $P.Range
        Check 'RangeValue pattern' ($null -ne $rv)
        $v = $rv.Current.Value
        Check 'RangeValue value after selecting' ([math]::Abs($v - 0.05) -lt 1e-6 -and $rv.Current.Maximum -eq 100) "value=$v of $($rv.Current.Maximum)"
    }
    catch { SectionFailed 'RangeValue' $_ }

    # --- Invoke opens the modal dialog; Cancel closes it, Uninstall confirms.
    try {
        $inv = Pattern (ById $win 'UninstallButton') $P.Invoke
        Check 'Invoke pattern' ($null -ne $inv)
        $inv.Invoke()
        $dlg = WaitFor { ById $win 'ConfirmDialog' }
        Check 'Invoke opened the dialog' ($null -ne $dlg) "footer: $(Footer $win)"
        $wp = Pattern $dlg $P.Window
        $modal = if ($wp) { $wp.Current.IsModal } else { 'no Window pattern' }
        Check 'dialog is a modal window' ($modal -eq $true) "$($dlg.Current.ControlType.ProgrammaticName), modal: $modal"
        $desc = ComDescription $hwnd 'ConfirmDialog'
        $body = ById $dlg 'ConfirmBody'
        $bodyText = if ($body) { $body.Current.Name } else { '(not found)' }
        Check 'dialog has its body as text, and as its description' ($bodyText -eq '1 programs will be removed.' -and $desc -eq $bodyText) "ConfirmBody '$bodyText', FullDescription '$desc'"
        # The rest of the window is hidden while the dialog is open: AccessKit leaves hidden nodes
        # out of what UIA sees, so they are not found at all.
        $behind = ById $win 'UninstallButton'
        $how = if ($null -eq $behind) { 'not found' } else { "found, offscreen: $($behind.Current.IsOffscreen)" }
        Check 'the window behind the dialog is hidden' ($null -eq $behind -or $behind.Current.IsOffscreen) "UninstallButton $how; footer found: $($null -ne (ById $win 'FooterCount'))"
        (Pattern (ById $dlg 'ConfirmNo') $P.Invoke).Invoke()
        $closed = WaitFor { $null -eq (ById $win 'ConfirmDialog') }
        Check 'Cancel closed the dialog' $closed "footer: $(Footer $win)"
        Check 'the window is back after the dialog' ($null -ne (ById $win 'UninstallButton'))
        (Pattern (ById $win 'UninstallButton') $P.Invoke).Invoke()
        $yes = WaitFor { ById $win 'ConfirmYes' }
        (Pattern $yes $P.Invoke).Invoke()
        $f = WaitFor { $t = Footer $win; if ($t -match 'uninstalled 1,') { $t } }
        Check 'Uninstall confirmed' ($f -match 'uninstalled 1,' -and $f -match 'pressed 2 times') "footer: $(Footer $win)"
    }
    catch { SectionFailed 'dialog' $_ }

    # --- ExpandCollapse on a group header of the grouped list; the items' place in their group.
    try {
        $ec = Pattern (ById $win 'GroupHeader1') $P.Expand
        Check 'ExpandCollapse pattern' ($null -ne $ec) "state: $(if ($ec) { $ec.Current.ExpandCollapseState })"
        Check 'group items shown while open' ($null -ne (ById $win 'Leftover1_0'))
        $set = ComSet $hwnd 'Leftover1_0'
        Check 'leftover item position of size' ($set.Position -eq 1 -and $set.Size -eq 6) "Leftover1_0: $($set.Position) of $($set.Size)"
        $set = ComSet $hwnd 'Leftover1_5'
        Check 'last leftover item position of size' ($set.Position -eq 6 -and $set.Size -eq 6) "Leftover1_5: $($set.Position) of $($set.Size)"
        $ec.Collapse()
        $st = WaitFor { $s = (Pattern (ById $win 'GroupHeader1') $P.Expand).Current.ExpandCollapseState; if ($s -eq 'Collapsed' -and $null -eq (ById $win 'Leftover1_0')) { $s } }
        Check 'Collapse acted' ($st -eq 'Collapsed') "state: $((Pattern (ById $win 'GroupHeader1') $P.Expand).Current.ExpandCollapseState)"
        (Pattern (ById $win 'GroupHeader1') $P.Expand).Expand()
        $st = WaitFor { $s = (Pattern (ById $win 'GroupHeader1') $P.Expand).Current.ExpandCollapseState; if ($s -eq 'Expanded' -and $null -ne (ById $win 'Leftover1_0')) { $s } }
        Check 'Expand acted' ($st -eq 'Expanded') "state: $((Pattern (ById $win 'GroupHeader1') $P.Expand).Current.ExpandCollapseState)"
        # The last header is below the pane at first, so it has no bounds; focusing it (as Tab
        # does) scrolls the pane to it.
        $pane = ById $win 'LeftoverPane'
        $wasOut = -not (InView (ById $win 'GroupHeader3') $pane)
        (ById $win 'GroupHeader3').SetFocus()
        $in = WaitFor { InView (ById $win 'GroupHeader3') $pane }
        Check 'focus scrolls a header into the pane' ($wasOut -and $in) "out before: $wasOut; header $((ById $win 'GroupHeader3').Current.BoundingRectangle), pane $($pane.Current.BoundingRectangle)"
    }
    catch { SectionFailed 'ExpandCollapse' $_ }

    # --- Value: the search field filters the list; a row keeps its program, and the selection
    # stays on its program.
    try {
        $farName = (ById $list "Row$($FarRow - 1)").Current.Name
        # The far program's name less its last two digits ("Program 15" for program 1500) shows
        # the programs numbered from $lo to $hi, the far one at $lo's distance from the first.
        $prefix = [int][math]::Floor($FarRow / 100)
        $query = 'Program {0:D2}' -f $prefix
        $lo = [math]::Max(1, $prefix * 100)
        $hi = [math]::Min(2000, $prefix * 100 + 99)
        $count = $hi - $lo + 1
        $farAt = $FarRow - $lo + 1
        # A program the search hides: program 5, or program 2000 when the search shows 5.
        $hidden = if ($prefix -eq 0) { 'Row1999' } else { 'Row4' }
        $val = Pattern (ById $win 'SearchBox') $P.Value
        Check 'Value pattern' ($null -ne $val)
        $val.SetValue($query)
        $f = WaitFor { $t = Footer $win; if ($t -match " $count shown") { $t } }
        $got = (Pattern (ById $win 'SearchBox') $P.Value).Current.Value
        Check 'search filtered' ($got -eq $query -and $f -match " $count shown") "'$query' should show $lo to $hi; value '$got', footer: $(Footer $win)"
        $kids = (ById $win 'ProgramList').FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
        Check 'list has the filtered rows' ($kids.Count -eq $count) "$($kids.Count) of $count"
        $row = ById $win "Row$($FarRow - 1)"
        $rs = ComSet $hwnd "Row$($FarRow - 1)"
        Check 'a row keeps its program through the filter' ($null -ne $row -and $row.Current.Name -eq $farName -and $rs.Position -eq $farAt -and $rs.Size -eq $count) "'$($row.Current.Name)' (was '$farName'), $($rs.Position) of $($rs.Size), want $farAt of $count"
        Check 'the selection stays on its program' ((IsSelected $win "Row$($FarRow - 1)") -and (Footer $win) -match '^1 selected') "footer: $(Footer $win)"
        Check 'a hidden program is not in the tree' ($null -eq (ById $win $hidden)) $hidden
        $val.SetValue('')
        $f = WaitFor { $t = Footer $win; if ($t -match ' 2000 shown') { $t } }
        Check 'search cleared' ($f -match ' 2000 shown') "footer: $(Footer $win)"
    }
    catch { SectionFailed 'Value' $_ }

    # --- Focus events: a focus moved by UIA, then one moved by the keyboard, raises UIA's
    # focus-changed event for the element that took it.
    try {
        $mine = { @([UiaFocus]::Seen() | Where-Object { $_ -like "$($proc.Id):*" } | ForEach-Object { $_.Substring($_.IndexOf(':') + 1) }) }
        [UiaFocus]::Start()
        # Row4 is out of view now (the list shows the far row); SetFocus on it must scroll it
        # into view, as the keyboard would, and its rectangle stop being empty.
        $list = ById $win 'ProgramList'
        $rowWasOut = -not (InView (ById $list 'Row4') $list)
        $want = @('Row4', 'SearchBox', 'GroupHeader2')
        foreach ($id in $want) {
            (ById $win $id).SetFocus()
            $null = WaitFor { (& $mine) -contains $id }
            if ($id -eq 'Row4') {
                $rowIn = WaitFor { InView (ById $list 'Row4') $list }
                Check 'focus scrolls an off-view row into view' ($rowWasOut -and $rowIn) "out before: $rowWasOut; row $((ById $list 'Row4').Current.BoundingRectangle), list $($list.Current.BoundingRectangle)"
            }
        }
        # Each wanted id in order, whatever else came between (the window itself, say).
        $got = & $mine
        $at = 0
        foreach ($g in $got) { if ($at -lt $want.Count -and $g -eq $want[$at]) { $at++ } }
        Check 'SetFocus raises focus events, in order' ($at -eq $want.Count) "seen: $($got -join ', ')"

        # The keyboard: Tab from GroupHeader2 goes to GroupHeader3 in the window's ring.
        [UiaFocus]::Clear()
        $front = WaitFor { [UiaFocus]::Front($hwnd) } -TimeoutMs 2000 -IntervalMs 100
        $sent = 0
        if ($front) { $sent = [UiaFocus]::Key(0x09) }
        $tab = WaitFor { (& $mine) -contains 'GroupHeader3' }
        Check 'Tab raises a focus event for the next element' ($tab) "foreground: $front, inputs sent: $sent, seen: $((& $mine) -join ', ')"
    }
    catch { SectionFailed 'focus events' $_ }
    finally { [UiaFocus]::Stop() }
}
finally {
    if (-not $proc.HasExited) { $proc.Kill() }
}
if ($script:checks -ne $Expected) {
    Write-Output "FAIL check count: $script:checks of $Expected ran"
    $script:failures++
}
Write-Output "checks: $script:checks of $Expected run; failures: $script:failures"
exit $script:failures
