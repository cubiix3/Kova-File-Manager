<# Captures the real packaged UI using disposable demonstration files and profile. #>
param([string]$Executable = 'target/release/kova-desktop.exe')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms
$kovaRoot=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$kovaExe=(Resolve-Path (Join-Path $kovaRoot $Executable)).Path
$kovaDemo=Join-Path $kovaRoot ('target/runtime/demo-'+[guid]::NewGuid())
foreach($kovaFolder in @('Project/Assets','Project/Notes','Inbox','Archive','profile/Kova','screenshots')) { [void][IO.Directory]::CreateDirectory((Join-Path $kovaDemo $kovaFolder)) }
$kovaDrive=@('K','L','M','N','P') | Where-Object { [IO.Directory]::GetLogicalDrives() -notcontains ($_+':\') } | Select-Object -First 1
if(-not $kovaDrive){throw 'No free demonstration drive letter'}
& subst ($kovaDrive+':') $kovaDemo
if($LASTEXITCODE){throw 'Cannot create demonstration drive'}
$kovaProject=$kovaDrive+':\Project'
$kovaInbox=$kovaDrive+':\Inbox'
Copy-Item -LiteralPath (Join-Path $kovaRoot 'apps/kova-desktop/assets/kova.png') -Destination (Join-Path $kovaProject 'Assets/Kova.png')
Copy-Item -LiteralPath (Join-Path $kovaRoot 'docs/images/made-with-slint.png') -Destination (Join-Path $kovaProject 'Assets/Slint.png')
[IO.File]::WriteAllText((Join-Path $kovaProject 'README.md'),"# Project workspace`r`n`r`nKeep source files, assets and notes together.`r`n`r`nKova restores this workspace after restart.`r`nSearch here, or include subfolders.`r`n")
[IO.File]::WriteAllText((Join-Path $kovaProject 'Image 2.txt'),'Natural sorting places 2 before 10.')
[IO.File]::WriteAllText((Join-Path $kovaProject 'Image 10.txt'),'The second numbered example.')
[IO.File]::WriteAllText((Join-Path $kovaProject 'Notes/Release checklist.txt'),"Review changes`r`nRun tests`r`nVerify Windows setup`r`n")
[IO.File]::WriteAllText((Join-Path $kovaInbox 'README.md'),'Earlier notes. Keep or replace explicitly.')
$kovaSession=@{version=1;tabs=@(@{path=$kovaProject;search=''},@{path=$kovaInbox;search=''});active=0;width=1400;height=820;gallery=$false;preview=$false;recent=@($kovaProject,$kovaInbox,($kovaDrive+':\Archive'))}
$kovaProfile=Join-Path $kovaDemo 'profile'
[IO.File]::WriteAllText((Join-Path $kovaProfile 'Kova/session.json'),($kovaSession|ConvertTo-Json -Depth 5))
[IO.File]::WriteAllText((Join-Path $kovaProfile 'Kova/library.json'),(@{pins=@($kovaProject,$kovaInbox);tags=@{};collections=@{}}|ConvertTo-Json -Depth 5))
function Find-DemoElement([string]$Name,[Windows.Automation.ControlType]$Type) {
    $kovaWindow=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$script:kovaProcess.Id))
    if(-not $kovaWindow){return $null}
    $kovaCondition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$Name),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $kovaWindow.FindFirst([Windows.Automation.TreeScope]::Descendants,$kovaCondition)
}
function Wait-DemoElement([string]$Name,[Windows.Automation.ControlType]$Type) {
    $kovaDeadline=[DateTime]::UtcNow.AddSeconds(15)
    do { $kovaElement=Find-DemoElement $Name $Type;if($kovaElement){return $kovaElement};Start-Sleep -Milliseconds 100 } while([DateTime]::UtcNow -lt $kovaDeadline)
    throw "Missing UI element: $Name"
}
function Select-DemoFile([string]$Name) {
    $kovaItem=Wait-DemoElement $Name ([Windows.Automation.ControlType]::ListItem)
    $kovaItem.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    $kovaItemBounds=$kovaItem.Current.BoundingRectangle
    $kovaWindowBounds=& "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Inspect | ConvertFrom-Json
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Click -X ([int]($kovaItemBounds.Left+90-$kovaWindowBounds.Left)) -Y ([int]($kovaItemBounds.Top+$kovaItemBounds.Height/2-$kovaWindowBounds.Top)) | Out-Null
}
function Invoke-DemoButton([string]$Name) { (Wait-DemoElement $Name ([Windows.Automation.ControlType]::Button)).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 300 }
function Send-DemoKeys([string]$Keys) { & "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Keys -Text $Keys | Out-Null }
function Save-Demo([string]$Name) { Start-Sleep -Milliseconds 700;& "$PSScriptRoot/runtime-window.ps1" -ProcessId $script:kovaProcess.Id -Action Screenshot -OutputPath (Join-Path $kovaDemo "screenshots/daily-driver-$Name.png") | Out-Null }
$kovaPrevious=$env:LOCALAPPDATA
$env:LOCALAPPDATA=$kovaProfile
try {$kovaProcess=Start-Process -FilePath $kovaExe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $kovaDemo 'app.log') -RedirectStandardError (Join-Path $kovaDemo 'error.log')}
finally {$env:LOCALAPPDATA=$kovaPrevious}
try {
    $null=Wait-DemoElement 'README.md' ([Windows.Automation.ControlType]::ListItem)
    $kovaScreen=[Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Resize -PositionX ($kovaScreen.Left+20) -PositionY ($kovaScreen.Top+20) -X ([math]::Min(1750,$kovaScreen.Width-40)) -Y ([math]::Min(1025,$kovaScreen.Height-40)) | Out-Null
    Select-DemoFile 'README.md'
    Save-Demo 'details'
    Send-DemoKeys ' '
    Save-Demo 'inspector'
    Send-DemoKeys ('^l'+$kovaProject+'\Assets{ENTER}')
    Select-DemoFile 'Kova.png'
    Send-DemoKeys '^2'
    $null=Wait-DemoElement 'Gallery' ([Windows.Automation.ControlType]::Text)
    Save-Demo 'gallery'
    Send-DemoKeys '^1'
    if(Find-DemoElement 'Gallery' ([Windows.Automation.ControlType]::Text)){throw 'Ctrl+1 did not restore Details view'}
    Send-DemoKeys '^t'
    $null=Wait-DemoElement 'Recent folders' ([Windows.Automation.ControlType]::Text)
    Save-Demo 'home'
    Send-DemoKeys ('^l'+$kovaProject+'{ENTER}')
    Invoke-DemoButton 'Details'
    Select-DemoFile 'README.md'
    Invoke-DemoButton 'Close inspector'
    Select-DemoFile 'README.md'
    Send-DemoKeys '+{F10}'
    $kovaMenuDeadline=[DateTime]::UtcNow.AddSeconds(10)
    while([KovaWindowTest]::NativeMenuWindow($kovaProcess.Id) -eq [IntPtr]::Zero){
        if([DateTime]::UtcNow -gt $kovaMenuDeadline){throw 'Shift+F10 did not open a native Windows menu'}
        Start-Sleep -Milliseconds 100
    }
    Save-Demo 'native'
    Send-DemoKeys '{ESC}'
    Select-DemoFile 'README.md'
    Send-DemoKeys '^c'
    Send-DemoKeys ('^l'+$kovaInbox+'{ENTER}')
    Send-DemoKeys '^v'
    $null=Wait-DemoElement 'Keep Both' ([Windows.Automation.ControlType]::Button)
    Save-Demo 'conflict'
    Invoke-DemoButton 'Keep Both'
    $null=Wait-DemoElement 'README (1).md' ([Windows.Automation.ControlType]::ListItem)
    if([IO.File]::ReadAllText((Join-Path $kovaInbox 'README (1).md')) -ne [IO.File]::ReadAllText((Join-Path $kovaProject 'README.md'))){throw 'Keep Both contents mismatch'}
    [pscustomobject]@{Result='PASS';Directory=$kovaDemo;Screenshots=@(Get-ChildItem (Join-Path $kovaDemo 'screenshots') | ForEach-Object Name)} | ConvertTo-Json
} finally {
    if(-not $kovaProcess.HasExited){& "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Close | Out-Null;if(-not $kovaProcess.WaitForExit(5000)){throw 'Demonstration window did not close cleanly'}}
    if((subst) -match ('(?m)^'+$kovaDrive+':\\: => '+[regex]::Escape($kovaDemo)+'$')){& subst ($kovaDrive+':') /D}
}
