param(
    [Parameter(Mandatory=$true)][int]$ProcessId,
    [ValidateSet('Inspect','Screenshot','Keys','Click','Drag','Resize','Close','Wheel','Restore','RightClick')][string]$Action = 'Inspect',
    [string]$Text,
    [string]$OutputPath,
    [int]$X, [int]$Y, [int]$EndX, [int]$EndY,
    [long]$WindowHandle,
    [int]$PositionX = [int]::MinValue, [int]$PositionY = [int]::MinValue,
    [ValidateSet('None','Copy','Move')][string]$Modifier = 'None',
    [string]$DragScreenshotPath,
    [int]$WheelDelta = -120, [int]$RepeatCount = 1, [int]$IntervalMilliseconds = 16
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KovaWindowTest {
    public delegate bool EnumWindow(IntPtr hwnd,IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindow callback,IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd,uint command);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hwnd);
    public static IntPtr MainWindow(int pid) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((hwnd,parameter)=>{uint owner;GetWindowThreadProcessId(hwnd,out owner);if(owner==pid && IsWindowVisible(hwnd) && GetWindow(hwnd,4)==IntPtr.Zero && GetWindowTextLength(hwnd)>0){found=hwnd;return false;}return true;},IntPtr.Zero);
        return found;
    }
    public static bool OwnsForeground(int pid) {uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);return owner==pid;}
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a,uint b,bool attach);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    public static void FocusWindow(IntPtr hwnd) {
        uint owner;uint foreground=GetWindowThreadProcessId(GetForegroundWindow(),out owner);uint current=GetCurrentThreadId();
        bool attached=foreground!=current && AttachThreadInput(current,foreground,true);
        try { ShowWindow(hwnd,IsIconic(hwnd)?9:5);BringWindowToTop(hwnd);SetForegroundWindow(hwnd); }
        finally {if(attached)AttachThreadInput(current,foreground,false);}
    }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out Rect rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd,int command);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hwnd,int x,int y,int width,int height,bool repaint);
    [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr hwnd,uint message,IntPtr wp,IntPtr lp);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
[void][KovaWindowTest]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$kovaProcess = Get-Process -Id $ProcessId
$kovaHandle = if ($WindowHandle) { [IntPtr]::new($WindowHandle) } else { [KovaWindowTest]::MainWindow($ProcessId) }
$kovaOwner=0
[void][KovaWindowTest]::GetWindowThreadProcessId($kovaHandle,[ref]$kovaOwner)
if ($kovaOwner -ne $ProcessId) { throw "Window does not belong to the specified test process." }
if ($kovaHandle -eq [IntPtr]::Zero) { throw 'The specified process has no visible window.' }
$kovaBounds = New-Object KovaWindowTest+Rect
[void][KovaWindowTest]::GetWindowRect($kovaHandle,[ref]$kovaBounds)
if ($Action -in @('Keys','Click','Drag','Resize','Wheel','Restore','RightClick')) {
    if (-not [KovaWindowTest]::OwnsForeground($ProcessId)) {
        [KovaWindowTest]::FocusWindow($kovaHandle)
        Start-Sleep -Milliseconds 150
    }
    if (-not [KovaWindowTest]::OwnsForeground($ProcessId)) { throw 'Refusing input: the test application is not the foreground window.' }
}
function Save-KovaSnapshot([string]$Path) {
    $kovaImage=New-Object System.Drawing.Bitmap ($kovaBounds.Right-$kovaBounds.Left),($kovaBounds.Bottom-$kovaBounds.Top)
    $kovaGraphics=[System.Drawing.Graphics]::FromImage($kovaImage)
    try { $kovaGraphics.CopyFromScreen($kovaBounds.Left,$kovaBounds.Top,0,0,$kovaImage.Size);$kovaImage.Save($Path,[System.Drawing.Imaging.ImageFormat]::Png) }
    finally { $kovaGraphics.Dispose();$kovaImage.Dispose() }
}
switch ($Action) {
    'Keys' { [System.Windows.Forms.SendKeys]::SendWait($Text) }
    'Click' {
        [void][KovaWindowTest]::SetCursorPos($kovaBounds.Left+$X,$kovaBounds.Top+$Y)
        Start-Sleep -Milliseconds 40
        [KovaWindowTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 35
        [KovaWindowTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    }
    'Restore' { [void][KovaWindowTest]::ShowWindow($kovaHandle,9) }
    'Wheel' { [void][KovaWindowTest]::SetCursorPos($kovaBounds.Left+$X,$kovaBounds.Top+$Y); for($kovaRepeat=0;$kovaRepeat -lt $RepeatCount;$kovaRepeat++){[KovaWindowTest]::mouse_event(0x0800,0,0,([BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$WheelDelta),0)),[UIntPtr]::Zero);Start-Sleep -Milliseconds $IntervalMilliseconds} }
    'RightClick' { [void][KovaWindowTest]::SetCursorPos($kovaBounds.Left+$X,$kovaBounds.Top+$Y); [KovaWindowTest]::mouse_event(8,0,0,0,[UIntPtr]::Zero); [KovaWindowTest]::mouse_event(16,0,0,0,[UIntPtr]::Zero) }
    'Drag' {
        $kovaModifierKey = if ($Modifier -eq 'Copy') {17} elseif ($Modifier -eq 'Move') {16} else {0}
        if ($kovaModifierKey) { [KovaWindowTest]::keybd_event($kovaModifierKey,0,0,[UIntPtr]::Zero) }
        try {
        [void][KovaWindowTest]::SetCursorPos($kovaBounds.Left+$X,$kovaBounds.Top+$Y)
        Start-Sleep -Milliseconds 40
        [KovaWindowTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        for ($kovaStep=1;$kovaStep -le 20;$kovaStep++) {
            [void][KovaWindowTest]::SetCursorPos($kovaBounds.Left+$X+($EndX-$X)*$kovaStep/20,$kovaBounds.Top+$Y+($EndY-$Y)*$kovaStep/20)
            Start-Sleep -Milliseconds 30
        }
        Start-Sleep -Milliseconds 250
        if ($DragScreenshotPath) { Save-KovaSnapshot $DragScreenshotPath }
        } finally {
            [KovaWindowTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
            if ($kovaModifierKey) { [KovaWindowTest]::keybd_event($kovaModifierKey,0,2,[UIntPtr]::Zero) }
        }
    }
    'Resize' { $kovaLeft=if($PositionX -eq [int]::MinValue){$kovaBounds.Left}else{$PositionX};$kovaTop=if($PositionY -eq [int]::MinValue){$kovaBounds.Top}else{$PositionY};[void][KovaWindowTest]::MoveWindow($kovaHandle,$kovaLeft,$kovaTop,$X,$Y,$true) }
    'Close' { [void][KovaWindowTest]::PostMessage($kovaHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero) }
    'Screenshot' {
        if (-not $OutputPath) { throw 'OutputPath is required.' }
        Save-KovaSnapshot $OutputPath
    }
}
if ($Action -in @('Keys','Click','Drag','Resize','Wheel','Restore','RightClick')) { Start-Sleep -Milliseconds 250 }
[pscustomobject]@{ProcessId=$ProcessId;Title=$kovaProcess.MainWindowTitle;Left=$kovaBounds.Left;Top=$kovaBounds.Top;Width=$kovaBounds.Right-$kovaBounds.Left;Height=$kovaBounds.Bottom-$kovaBounds.Top;Dpi=[KovaWindowTest]::GetDpiForWindow($kovaHandle);WorkingSetMiB=[math]::Round($kovaProcess.WorkingSet64/1MB,1)} | ConvertTo-Json -Compress
