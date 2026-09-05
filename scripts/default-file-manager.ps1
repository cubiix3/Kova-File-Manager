param(
    [ValidateSet('Enable','Restore','Status')][string]$Mode = 'Status',
    [string]$Executable = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target/release/kova-desktop.exe')
)
$ErrorActionPreference = 'Stop'
$stateDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Kova'
$backupPath = Join-Path $stateDir 'folder-associations.json'
$installedExe = Join-Path $stateDir 'Kova.exe'
$classes = @('Directory','Drive')
$verb = 'Kova.OpenFolder'
function Read-Default($key) {
    $present = $key -and ($key.GetValueNames() -contains '')
    @{ Present=[bool]$present; Value=$(if($present){$key.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}else{$null}); Kind=$(if($present){$key.GetValueKind('').ToString()}else{'String'}) }
}
if($Mode -eq 'Status') {
    $enabled = $true
    foreach($class in $classes){
        $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$class\shell")
        try { if(-not $key -or $key.GetValue('') -ne $verb){$enabled=$false} } finally {if($key){$key.Dispose()}}
    }
    if($enabled){'Kova opens folders and drives by default.'}else{'Kova is not the default folder app.'}
    return
}
if($Mode -eq 'Enable') {
    if(-not (Test-Path -LiteralPath $Executable -PathType Leaf)){throw 'Build Kova release before enabling folder integration.'}
    New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
    if($PSScriptRoot){
        $notice=Join-Path (Split-Path $PSScriptRoot -Parent) 'apps/kova-desktop/ui/third-party/FILES-ICONS-LICENSE.txt'
        if(Test-Path -LiteralPath $notice){Copy-Item -LiteralPath $notice -Destination (Join-Path $stateDir 'FILES-ICONS-LICENSE.txt') -Force}
    }
    # Install first; never leave a registry command pointing into a build folder.
    if([IO.Path]::GetFullPath($Executable) -ne [IO.Path]::GetFullPath($installedExe)){
        Copy-Item -LiteralPath $Executable -Destination $installedExe -Force
    }
    if(-not (Test-Path -LiteralPath $backupPath)){
        $backup=@{}
        foreach($class in $classes){
            $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$class\shell")
            try {
                if($key -and ($key.GetSubKeyNames() -contains $verb)){throw "Existing $verb registration has no Kova backup; leaving it untouched."}
                $backup[$class]=Read-Default $key
            } finally {if($key){$key.Dispose()}}
        }
        [IO.File]::WriteAllText($backupPath,($backup | ConvertTo-Json -Depth 4))
    }
    foreach($class in $classes){
        $key=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("Software\Classes\$class\shell")
        try {
            $action=$key.CreateSubKey($verb)
            try {
                $action.SetValue('','Open in Kova')
                $action.SetValue('Icon',('"'+$installedExe+'",0'))
                $command=$action.CreateSubKey('command')
                try {
                    # A final dot avoids the Windows argv trailing-backslash/quote
                    # ambiguity for drive roots. The path resolver removes it.
                    $command.SetValue('',('"'+$installedExe+'" --open "%1\."'))
                } finally {$command.Dispose()}
            } finally {$action.Dispose()}
            $key.SetValue('', $verb)
        } finally {$key.Dispose()}
    }
    'Kova now opens folders and drives by default. Restore is available in the Kova menu.'
} else {
    if(-not (Test-Path -LiteralPath $backupPath)){throw 'No Kova association backup exists; nothing changed.'}
    $backup=[IO.File]::ReadAllText($backupPath) | ConvertFrom-Json
    foreach($class in $classes){
        $saved=$backup.$class
        if(-not $saved){throw "Invalid association backup for $class."}
        $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$class\shell",$true)
        if(-not $key){continue}
        try {
            # Respect a default the user or another app selected after Kova.
            if($key.GetValue('') -eq $verb){
                if($saved.Present){$key.SetValue('', $saved.Value, [Microsoft.Win32.RegistryValueKind]$saved.Kind)}
                else {$key.DeleteValue('', $false)}
            }
            $key.DeleteSubKeyTree($verb,$false)
        } finally {$key.Dispose()}
    }
    Remove-Item -LiteralPath $backupPath
    'Previous folder and drive defaults restored.'
}
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KovaAssociationNotice {
 [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e,uint flags,IntPtr a,IntPtr b);
}
'@
[KovaAssociationNotice]::SHChangeNotify(0x08000000,0,[IntPtr]::Zero,[IntPtr]::Zero)
