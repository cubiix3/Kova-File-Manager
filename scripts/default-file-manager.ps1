param(
    [ValidateSet('Enable','Restore','Status')][string]$Mode = 'Status',
    [string]$Executable = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target/release/kova-desktop.exe')
)
$ErrorActionPreference = 'Stop'
$stateDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Kova'
$backupPath = Join-Path $stateDir 'folder-associations.json'
$commandBackupPath = Join-Path $stateDir 'folder-commands.json'
$installedExe = Join-Path $stateDir 'Kova.exe'
$classes = @('Directory','Drive')
$verb = 'Kova.OpenFolder'
$openVerbs = @('open','explore','opennewwindow')
$openCommand = '"'+$installedExe+'" --open "%1\."'
function Read-Value($key, [string]$name) {
    $present = $key -and ($key.GetValueNames() -contains $name)
    @{ Present=[bool]$present; Value=$(if($present){$key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}else{$null}); Kind=$(if($present){$key.GetValueKind($name).ToString()}else{'String'}) }
}
function Read-Default($key) {
    Read-Value $key ''
}
if($Mode -eq 'Status') {
    $enabled = Test-Path -LiteralPath $installedExe -PathType Leaf
    foreach($class in $classes){
        $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$class\shell")
        try { if(-not $key -or $key.GetValue('') -ne $verb){$enabled=$false} } finally {if($key){$key.Dispose()}}
        foreach($openVerb in $openVerbs){
            $key=[Microsoft.Win32.Registry]::ClassesRoot.OpenSubKey("$class\shell\$openVerb\command")
            try { if(-not $key -or $key.GetValue('') -ne $openCommand -or $key.GetValue('DelegateExecute',$null) -ne ''){$enabled=$false} } finally {if($key){$key.Dispose()}}
        }
    }
    if($enabled){'Kova handles default and explicit folder/drive opens.'}else{'Folder integration is incomplete or disabled. Enable it again to repair it.'}
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
    # Upgrade older installations without replacing their original default backup.
    # Snapshot every affected per-user value before overriding explicit Shell verbs.
    if(-not (Test-Path -LiteralPath $commandBackupPath)){
        $commands=@(foreach($class in $classes){foreach($openVerb in $openVerbs){
            $path="Software\Classes\$class\shell\$openVerb"
            $action=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($path)
            $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("$path\command")
            try {
                [pscustomobject]@{ Path=$path; ActionExisted=[bool]$action; CommandExisted=[bool]$key; Default=(Read-Default $key); Delegate=(Read-Value $key 'DelegateExecute') }
            } finally {if($key){$key.Dispose()};if($action){$action.Dispose()}}
        }})
        [IO.File]::WriteAllText($commandBackupPath, (ConvertTo-Json -InputObject $commands -Depth 5))
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
                    $command.SetValue('', $openCommand)
                } finally {$command.Dispose()}
            } finally {$action.Dispose()}
            $key.SetValue('', $verb)
        } finally {$key.Dispose()}
        foreach($openVerb in $openVerbs){
            $command=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("Software\Classes\$class\shell\$openVerb\command")
            try {
                $command.SetValue('', $openCommand)
                # An empty per-user value masks the machine's Explorer COM handler.
                $command.SetValue('DelegateExecute','')
            } finally {$command.Dispose()}
        }
    }
    'Kova now opens folders and drives by default. Restore is available in the Kova menu.'
} else {
    if(-not (Test-Path -LiteralPath $backupPath)){throw 'No Kova association backup exists; nothing changed.'}
    $backup=[IO.File]::ReadAllText($backupPath) | ConvertFrom-Json
    if(Test-Path -LiteralPath $commandBackupPath){
        $commands=[IO.File]::ReadAllText($commandBackupPath) | ConvertFrom-Json
        foreach($saved in $commands){
            # Only paths in our fixed scope may be restored from the backup.
            if($saved.Path -notmatch '^Software\\Classes\\(Directory|Drive)\\shell\\(open|explore|opennewwindow)$'){throw 'Invalid command backup path.'}
            $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(($saved.Path+'\command'),$true)
            if(-not $key){continue}
            try {
                # Leave a later replacement command (and its delegate) intact.
                if($key.GetValue('') -eq $openCommand){
                    foreach($entry in @(@{Name=''; Saved=$saved.Default; Expected=$openCommand},@{Name='DelegateExecute'; Saved=$saved.Delegate; Expected=''})){
                        if($key.GetValue($entry.Name,$null) -ne $entry.Expected){continue}
                        if($entry.Saved.Present){$key.SetValue($entry.Name,$entry.Saved.Value,[Microsoft.Win32.RegistryValueKind]$entry.Saved.Kind)}
                        else {$key.DeleteValue($entry.Name,$false)}
                    }
                }
                $empty=$key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0
            } finally {$key.Dispose()}
            $action=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($saved.Path,$true)
            if($action){
                try {
                    if(-not $saved.CommandExisted -and $empty){$action.DeleteSubKey('command',$false)}
                    $actionEmpty=$action.ValueCount -eq 0 -and $action.SubKeyCount -eq 0
                } finally {$action.Dispose()}
                if(-not $saved.ActionExisted -and $actionEmpty){[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKey($saved.Path,$false)}
            }
        }
        Remove-Item -LiteralPath $commandBackupPath
    }
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
