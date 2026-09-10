param([string]$Executable = 'target/release/kova-desktop.exe', [switch]$PrepareFixtures)
$ErrorActionPreference = 'Stop'
$kovaRoot=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$kovaExecutable=(Resolve-Path (Join-Path $kovaRoot $Executable)).Path
$kovaOutput=Join-Path $kovaRoot 'target/runtime/ui-benchmark'
[void][IO.Directory]::CreateDirectory($kovaOutput)
[pscustomobject]@{
    DateUtc=[DateTime]::UtcNow.ToString('o')
    OperatingSystem=(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber)
    Processor=(Get-CimInstance Win32_Processor | Select-Object -First 1 Name,NumberOfLogicalProcessors)
    RamBytes=(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    ExecutableVersion=[Diagnostics.FileVersionInfo]::GetVersionInfo($kovaExecutable).FileVersion
    ExecutableSha256=(Get-FileHash -LiteralPath $kovaExecutable -Algorithm SHA256).Hash
} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $kovaOutput 'environment.json')
$kovaMeasurements=@()
foreach($kovaCount in @(1000,10000,100000)) {
    $kovaFolder=Join-Path $kovaRoot ('target/runtime/large'+$(if($kovaCount -eq 100000){'100k'}else{"$kovaCount"}))
    if ($PrepareFixtures) {
        [void][IO.Directory]::CreateDirectory($kovaFolder)
        for ($kovaIndex=0; $kovaIndex -lt $kovaCount; $kovaIndex++) {
            $kovaFile=Join-Path $kovaFolder "Image $kovaIndex.txt"
            if (-not [IO.File]::Exists($kovaFile)) {
                $kovaStream=[IO.File]::Open($kovaFile,[IO.FileMode]::CreateNew)
                $kovaStream.Dispose()
            }
        }
    }
    if(-not (Test-Path -LiteralPath $kovaFolder)){throw "Missing fixture: $kovaFolder"}
    $kovaProfile=Join-Path $kovaOutput "profile-$kovaCount"
    [void][IO.Directory]::CreateDirectory((Join-Path $kovaProfile 'Kova'))
    $kovaSession=@{version=1;tabs=@(@{path=$kovaFolder;recursive=$false;search='';sort=0;descending=$false});active=0;width=1120;height=720;maximized=$false;gallery=$false;preview=$false}
    [IO.File]::WriteAllText((Join-Path $kovaProfile 'Kova/session.json'),($kovaSession|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
    $kovaLog=Join-Path $kovaOutput "$kovaCount.log"
    $kovaPreviousProfile=$env:LOCALAPPDATA
    $kovaPreviousPerf=$env:KOVA_PERF
    $env:LOCALAPPDATA=$kovaProfile;$env:KOVA_PERF='1'
    try {$kovaProcess=Start-Process -FilePath $kovaExecutable -WindowStyle Hidden -PassThru -RedirectStandardOutput $kovaLog -RedirectStandardError (Join-Path $kovaOutput "$kovaCount-error.log")}
    finally {$env:LOCALAPPDATA=$kovaPreviousProfile;$env:KOVA_PERF=$kovaPreviousPerf}
    try {
        Start-Sleep -Seconds 3
        $kovaWindow=& "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Inspect | ConvertFrom-Json
        $kovaScale=$kovaWindow.Dpi/96.0
        for($kovaTrial=0;$kovaTrial -lt 3;$kovaTrial++) {
            & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Click -X ([int](500*$kovaScale)) -Y ([int](520*$kovaScale)) | Out-Null
            & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Keys -Text '{F5}' | Out-Null
            Start-Sleep -Milliseconds 1200
            & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Keys -Text '^f^aImage 999' | Out-Null
            Start-Sleep -Milliseconds 750
            & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Keys -Text '^a{BACKSPACE}' | Out-Null
            Start-Sleep -Milliseconds 750
            & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Click -X ([int](320*$kovaScale)) -Y ([int](262*$kovaScale)) | Out-Null
            Start-Sleep -Milliseconds 750
        }
        $kovaScrollStart=[DateTime]::UtcNow.ToString('o')
        & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Wheel -X ([int](600*$kovaScale)) -Y ([int](520*$kovaScale)) -WheelDelta -120 -RepeatCount 60 -IntervalMilliseconds 16 | Out-Null
        $kovaScrollEnd=[DateTime]::UtcNow.ToString('o')
        & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Screenshot -OutputPath (Join-Path $kovaOutput "$kovaCount.png") | Out-Null
        $kovaProcess.Refresh()
        $kovaMeasurements += [pscustomobject]@{Entries=$kovaCount;Dpi=$kovaWindow.Dpi;WindowWidth=$kovaWindow.Width;WindowHeight=$kovaWindow.Height;WorkingSetMiB=[math]::Round($kovaProcess.WorkingSet64/1MB,2);PeakWorkingSetMiB=[math]::Round($kovaProcess.PeakWorkingSet64/1MB,2);ScrollStartUtc=$kovaScrollStart;ScrollEndUtc=$kovaScrollEnd;Log=$kovaLog}
    } finally {
        & "$PSScriptRoot/runtime-window.ps1" -ProcessId $kovaProcess.Id -Action Close | Out-Null
        if(-not $kovaProcess.WaitForExit(5000)){throw 'Benchmark window did not exit cleanly.'}
    }
}
$kovaMeasurements|ConvertTo-Json -Depth 5|Set-Content (Join-Path $kovaOutput 'measurements.json')
$kovaMeasurements|ConvertTo-Json -Compress
