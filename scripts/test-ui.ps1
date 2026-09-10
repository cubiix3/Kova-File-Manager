<#
Runs against a visible, isolated Kova window on an interactive Windows desktop.
Exercises real keyboard input and Windows UI Automation. Only its UUID fixture
is modified; no clipboard, associations, user files or installed Kova profile.
Not suitable for a locked desktop or a user actively typing in another window.
#>
param([string]$Executable = 'target/debug/kova-desktop.exe')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$kovaRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$kovaExe = (Resolve-Path (Join-Path $kovaRoot $Executable)).Path
$kovaFixture = Join-Path $kovaRoot ('target/runtime/ui-test-' + [guid]::NewGuid())
$kovaFiles = Join-Path $kovaFixture 'files'
$kovaProfile = Join-Path $kovaFixture 'profile'
[void][IO.Directory]::CreateDirectory($kovaFiles)
[void][IO.Directory]::CreateDirectory((Join-Path $kovaFiles 'Nested'))
[void][IO.Directory]::CreateDirectory((Join-Path $kovaProfile 'Kova'))
[IO.File]::WriteAllText((Join-Path $kovaFiles 'Before.txt'), 'Preserve these contents.')
[IO.File]::WriteAllText((Join-Path $kovaFiles 'Nested/Needle.txt'), 'Recursive match.')
$kovaSession = @{version=1;tabs=@(@{path=$kovaFiles;search='';recursive=$false});active=0;width=1120;height=720;gallery=$false;preview=$false}
[IO.File]::WriteAllText((Join-Path $kovaProfile 'Kova/session.json'), ($kovaSession | ConvertTo-Json -Depth 5))
function Start-TestWindow {
    $kovaPrevious = $env:LOCALAPPDATA
    $env:LOCALAPPDATA = $kovaProfile
    try { Start-Process -FilePath $kovaExe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $kovaFixture 'app.log') -RedirectStandardError (Join-Path $kovaFixture 'error.log') }
    finally { $env:LOCALAPPDATA = $kovaPrevious }
}
function Wait-TestCondition([scriptblock]$Condition, [string]$Description) {
    $kovaDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $kovaDeadline)
    throw "Timed out: $Description"
}
function Find-TestElement([string]$Name, [Windows.Automation.ControlType]$Type) {
    $kovaWindow = [Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children, [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $script:kovaProcess.Id))
    if (-not $kovaWindow) { return $null }
    $kovaCondition = [Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Name), [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, $Type))
    $kovaWindow.FindFirst([Windows.Automation.TreeScope]::Descendants, $kovaCondition)
}
function Send-TestKeys([string]$Keys) {
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Keys -Text $Keys | Out-Null
}
function Close-TestWindow {
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Close | Out-Null
    if (-not $script:kovaProcess.WaitForExit(5000)) { throw 'Test window did not exit cleanly' }
}
$kovaProcess = Start-TestWindow
try {
    Wait-TestCondition { Find-TestElement 'Before.txt' ([Windows.Automation.ControlType]::ListItem) } 'initial folder enumeration and accessibility tree'
    $kovaItem = Find-TestElement 'Before.txt' ([Windows.Automation.ControlType]::ListItem)
    $kovaItem.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    Send-TestKeys '{F2}'
    Send-TestKeys '^aAfter.txt{ENTER}'
    Wait-TestCondition { Test-Path -LiteralPath (Join-Path $kovaFiles 'After.txt') } 'rename'
    Send-TestKeys '{F5}'
    Wait-TestCondition { Find-TestElement 'After.txt' ([Windows.Automation.ControlType]::ListItem) } 'refresh after rename'
    if ((Test-Path -LiteralPath (Join-Path $kovaFiles 'Before.txt')) -or [IO.File]::ReadAllText((Join-Path $kovaFiles 'After.txt')) -ne 'Preserve these contents.') { throw 'Rename changed contents or left the original name' }
    Send-TestKeys '^z'
    Wait-TestCondition { Find-TestElement 'Undo operation' ([Windows.Automation.ControlType]::Text) } 'reviewable Undo dialog'
    Send-TestKeys '{ENTER}'
    Wait-TestCondition { Test-Path -LiteralPath (Join-Path $kovaFiles 'Before.txt') } 'Undo restored original path'
    Wait-TestCondition { Find-TestElement 'Before.txt' ([Windows.Automation.ControlType]::ListItem) } 'restored selection model'
    $kovaItem = Find-TestElement 'Before.txt' ([Windows.Automation.ControlType]::ListItem)
    $kovaItem.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    $kovaSourceBounds = $kovaItem.Current.BoundingRectangle
    $kovaTargetBounds = (Find-TestElement 'Nested' ([Windows.Automation.ControlType]::ListItem)).Current.BoundingRectangle
    $kovaBounds = & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Inspect | ConvertFrom-Json
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Drag -X ([int]($kovaSourceBounds.Left + 90 - $kovaBounds.Left)) -Y ([int]($kovaSourceBounds.Top + $kovaSourceBounds.Height/2 - $kovaBounds.Top)) -EndX ([int]($kovaTargetBounds.Left + 90 - $kovaBounds.Left)) -EndY ([int]($kovaTargetBounds.Top + $kovaTargetBounds.Height/2 - $kovaBounds.Top)) -Modifier Copy -DragScreenshotPath (Join-Path $kovaFixture 'ctrl-drag.png') | Out-Null
    Wait-TestCondition { Test-Path -LiteralPath (Join-Path $kovaFiles 'Nested/Before.txt') } 'Ctrl-drag of an already selected file'
    if ([IO.File]::ReadAllText((Join-Path $kovaFiles 'Before.txt')) -ne [IO.File]::ReadAllText((Join-Path $kovaFiles 'Nested/Before.txt'))) { throw 'Ctrl-drag must preserve original and copied contents' }
    Send-TestKeys '^fNeedle'
    $kovaScope = Find-TestElement 'Include subfolders' ([Windows.Automation.ControlType]::CheckBox)
    if (-not $kovaScope) { throw 'Recursive scope lacks an accessible checkbox' }
    $kovaScope.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-TestCondition { Find-TestElement 'Needle.txt' ([Windows.Automation.ControlType]::ListItem) } 'recursive search result'
    # Global address shortcut must work while the search field retains focus.
    Send-TestKeys '^l'
    Send-TestKeys ($kovaFiles + '{ENTER}')
    Send-TestKeys '^t'
    Send-TestKeys '^l'
    Send-TestKeys ((Join-Path $kovaFiles 'Nested') + '{ENTER}')
    Wait-TestCondition { $kovaSaved=Get-Content -Raw (Join-Path $kovaProfile 'Kova/session.json') | ConvertFrom-Json; $kovaSaved.tabs.Count -eq 2 -and $kovaSaved.tabs[1].path -eq (Join-Path $kovaFiles 'Nested') } 'debounced persistence before shutdown'
    Close-TestWindow
    $kovaProcess = Start-TestWindow
    Wait-TestCondition { Find-TestElement 'Needle.txt' ([Windows.Automation.ControlType]::ListItem) } 'active tab after restart'
    Send-TestKeys '^{TAB}'
    Wait-TestCondition { Find-TestElement 'Before.txt' ([Windows.Automation.ControlType]::ListItem) } 'restored first tab'
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Screenshot -OutputPath (Join-Path $kovaFixture 'verified.png') | Out-Null
    [pscustomobject]@{Result='PASS';Flows='open/select/rename/refresh/undo/Ctrl-drag copy/recursive search/global address/tabs/debounced save/restart';Fixture=$kovaFixture} | ConvertTo-Json
} finally {
    if (-not $kovaProcess.HasExited) { Close-TestWindow }
}
