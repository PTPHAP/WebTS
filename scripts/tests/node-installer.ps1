param([Parameter(Mandatory=$true)][string]$PackageRoot,[Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::InputEncoding=[Text.UTF8Encoding]::new($true)
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1')
. (Join-Path $PackageRoot 'scripts\node-install.ps1')
function Assert($Condition,[string]$Name){if(!$Condition){throw ('FAIL: '+$Name)}}
function MustFail([scriptblock]$Call,[string]$Name){$failed=$false;try{&$Call}catch{$failed=$true};Assert $failed $Name}
$script:fakeProcess=$null;$script:foreignListener=$false;$script:stopped=@();$script:tasks=@();$script:rules=@();$script:started=$null
# Only these OS mutation APIs are mocked. Native configuration, files, ACLs,
# process discovery, stop/start and loopback HTTP are real inside Scratch.
function Get-ScheduledTask {return $script:tasks}
function Stop-ScheduledTask {param($InputObject) $script:taskStops++}
function Disable-ScheduledTask {param($InputObject) $script:disabled++}
function Export-ScheduledTask {param($TaskName,$TaskPath) return '<Task>synthetic-only</Task>'}
function Get-NetFirewallRule {param($Name,$ErrorAction) return @($script:rules|Where-Object {$_.Name -eq $Name})}
function Get-NetFirewallApplicationFilter {param([Parameter(ValueFromPipeline=$true)]$Rule) process{return [pscustomobject]@{Program=$Rule.Program}}}
function Get-NetFirewallPortFilter {param([Parameter(ValueFromPipeline=$true)]$Rule) process{return [pscustomobject]@{Protocol=$Rule.Protocol;LocalPort=$Rule.LocalPort}}}
function Set-NetFirewallRule {param($Name,$Protocol,$LocalPort,$Enabled) $script:ruleEnables++;foreach($rule in $script:rules){if($rule.Name -eq $Name){$rule.Protocol=$Protocol;$rule.LocalPort=$LocalPort}}}
function New-NetFirewallRule {
    param($Name,$DisplayName,$Group,$Description,$Direction,$Action,$Protocol,$LocalPort,$Program,$Profile,$Enabled)
    $script:rules+= [pscustomobject]@{Name=$Name;Group=$Group;Description=$Description;Direction=$Direction;Action=$Action;Protocol=$Protocol;LocalPort=$LocalPort;Program=$Program}
}
function New-ScheduledTaskAction {param($Execute,$Argument,$WorkingDirectory) return [pscustomobject]@{Execute=$Execute;Arguments=$Argument;WorkingDirectory=$WorkingDirectory}}
function New-ScheduledTaskPrincipal {param($UserId,$LogonType,$RunLevel) return [pscustomobject]@{UserId=$UserId;LogonType=$LogonType;RunLevel=$RunLevel}}
function New-ScheduledTaskSettingsSet {param($RestartCount,$RestartInterval,$ExecutionTimeLimit,$MultipleInstances,[switch]$StartWhenAvailable,[switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries) return [pscustomobject]@{ExecutionTimeLimit=$ExecutionTimeLimit;MultipleInstances=$MultipleInstances}}
function New-ScheduledTaskTrigger {param([switch]$AtStartup) return 'startup'}
function Register-ScheduledTask {
    param($TaskName,$TaskPath,$Action,$Trigger,$Principal,$Settings,$Description,[switch]$Force)
    $script:registered=[pscustomobject]@{Action=$Action;Principal=$Principal;Settings=$Settings;Name=$TaskName}
    $script:tasks=@([pscustomobject]@{TaskName=$TaskName;TaskPath=$TaskPath;Actions=@($Action)})
}
function Start-ScheduledTask {
    param($TaskName,$TaskPath)
    $action=$script:registered.Action
    $script:started=Start-Process -FilePath $action.Execute -ArgumentList $action.Arguments -WorkingDirectory $action.WorkingDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $Scratch 'native-out.log') -RedirectStandardError (Join-Path $Scratch 'native-error.log')
}
function Get-NetTCPConnection {
    param($State,$LocalPort,$ErrorAction)
    if($script:foreignListener){return [pscustomobject]@{OwningProcess=987654}}
    return NetTCPIP\Get-NetTCPConnection -State $State -LocalPort $LocalPort -ErrorAction SilentlyContinue
}
function Get-CimInstance {
    param($ClassName,$Filter)
    if($script:fakeProcess){return $script:fakeProcess}
    return CimCmdlets\Get-CimInstance -ClassName $ClassName -Filter $Filter
}
function Stop-Process {
    param($Id,[switch]$Force,$ErrorAction)
    $script:stopped+=$Id
    if(!$script:fakeProcess){Microsoft.PowerShell.Management\Stop-Process -Id $Id -Force -ErrorAction Stop}
}
function Read-Host {
    param($Prompt,[switch]$AsSecureString)
    Assert ($script:answerIndex -lt $script:answers.Count) 'unexpected wizard question'
    $value=$script:answers[$script:answerIndex];$script:answerIndex++
    if($AsSecureString){
        if(!$value){return [Security.SecureString]::new()}
        return ConvertTo-SecureString $value -AsPlainText -Force
    }
    return $value
}
function Answers($Values){$script:answers=$Values;$script:answerIndex=0}
function FreePort {
    $listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$listener.Start()
    try{return $listener.LocalEndpoint.Port}finally{$listener.Stop()}
}
function WaitHealth([int]$Port){
    for($i=0;$i -lt 30;$i++){
        try{if((Invoke-RestMethod ('http://127.0.0.1:'+$Port+'/health') -TimeoutSec 1).role -eq 'voice-node'){return}}catch{}
        Start-Sleep -Milliseconds 100
    }
    throw 'native health timeout'
}

$root=Join-Path $Scratch '安装位置 Chinese space'
[void][IO.Directory]::CreateDirectory($root)
$script:taskStops=0;$script:disabled=0;$script:ruleEnables=0
Assert-NodeDirectory $PackageRoot
Assert-NodeManifest $PackageRoot
Assert (Test-NodeServeArguments ('serve-node "'+$root+'\node.local.toml"') $root) 'absolute config recognized'
Assert (!(Test-NodeServeArguments 'serve-node node.local.toml' $root)) 'unknown working directory refused'
Assert (Test-NodeServeArguments 'serve-node node.local.toml' $root $root) 'explicit task working directory accepted'
Assert (!(Test-NodeServeArguments ('serve-node "'+$root+'\node.local.toml" & calc.exe') $root)) 'shell tail refused'
Assert (!(Test-NodeServeArguments ('serve-node "'+$root+'\other.toml"') $root)) 'other config refused'
foreach($ip in @('127.0.0.1','10.0.0.1','172.16.1.2','192.168.0.1','100.64.0.1','0.0.0.0','224.0.0.1','::1')){MustFail {Resolve-NodePublicIp $ip} ('private IP '+$ip)}
Assert ((Resolve-NodePublicIp '203.0.113.20') -eq '203.0.113.20') 'literal IPv4'
$script:tasks=@([pscustomobject]@{TaskName='WebTS-Node';TaskPath='\';Actions=@([pscustomobject]@{Execute='C:\unrelated.exe';Arguments='';WorkingDirectory=''})})
MustFail {Get-NodeTasks $root} 'unrelated same-name task preserved'
Assert ($script:taskStops -eq 0) 'no task stopped during preflight refusal'
$script:tasks=@([pscustomobject]@{TaskName='WebTS-Node';TaskPath='\';Actions=@([pscustomobject]@{Execute='web-ts.exe';Arguments=('serve-node "'+$root+'\node.local.toml"');WorkingDirectory=$root})})
MustFail {Get-NodeTasks $root} 'ambiguous PATH executable refused'
$script:tasks=@()
foreach($execute in @('"C:\Windows\System32\cmd.exe"','%windir%\System32\cmd.exe','C:\bad<path>\program.exe','$(Arg0)','')){
    $script:tasks=@([pscustomobject]@{TaskName='Unrelated';TaskPath='\';Actions=@([pscustomobject]@{Execute=$execute;Arguments='/c unrelated';WorkingDirectory=''})})
    Assert (@(Get-NodeTasks $root).Count -eq 0) 'special unrelated scheduled-task fields skipped'
}
Assert (!(Test-NodeServeArguments 'serve-node "C:\bad<path>\node.local.toml"' $root)) 'malformed config path safely refused'
$script:tasks=@()
$script:foreignListener=$true;$script:fakeProcess=[pscustomobject]@{ExecutablePath='C:\unrelated.exe';CommandLine='unrelated.exe';ProcessId=987654;CreationDate='fake'}
MustFail {Stop-OwnNode $root @() 8095} 'foreign port owner refused'
Assert ($script:stopped.Count -eq 0) 'foreign PID never killed'
$script:fakeProcess=$null;$script:foreignListener=$false
$script:rules=@([pscustomobject]@{Name=(Get-NodeRuleName $root);Group='unrelated';Description='other';Direction='Inbound';Action='Allow';Protocol='UDP';LocalPort='1-65535';Program='C:\unrelated.exe'})
MustFail {Assert-NodeRule $root '50000-50099'} 'foreign firewall rule refused'
$script:rules=@()

# Use an existing synthetic pairing to exercise offline reconfiguration without
# weakening TLS or contacting the real main site. Fresh empty pairing is tested below.
Copy-NodePayload $PackageRoot $root
& (Join-Path $root 'scripts\node.ps1') init
$port=FreePort
$main='https://127.0.0.1:'+(FreePort)
$config=Join-Path $root 'node.local.toml';$key=Join-Path $root 'secrets\node-pair.key'
$text=[IO.File]::ReadAllText($config,[Text.Encoding]::UTF8).Replace('127.0.0.1:8095',('127.0.0.1:'+$port)).Replace('https://web.example.com',$main).Replace('203.0.113.10','203.0.113.20')
[IO.File]::WriteAllText($config,$text,[Text.UTF8Encoding]::new($false))
$oldKey=[IO.File]::ReadAllText($key);$oldConfig=[IO.File]::ReadAllText($config)
$savedTasks=$script:tasks
$script:tasks=@([pscustomobject]@{TaskName='WebTS-Node';TaskPath='\';Actions=@([pscustomobject]@{Execute=(Join-Path $root 'web-ts.exe');Arguments=('serve-node "'+$config+'"');WorkingDirectory=$root})})
Assert ((Find-InstalledNodeRoot 'C:\WebTS-Node') -eq $root) 'existing installation is the default upgrade directory'
$script:tasks=$savedTasks
$oldProcess=Start-Process -FilePath (Join-Path $root 'web-ts.exe') -ArgumentList ('serve-node "'+$config+'"') -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $Scratch 'old-out.log') -RedirectStandardError (Join-Path $Scratch 'old-error.log')
try{
    WaitHealth $port
    # Cancellation preserves an actual running node and creates no backups.
    Answers @('','','','N','N')
    Invoke-NodeInstall $root $PackageRoot
    $oldProcess.Refresh();Assert (!$oldProcess.HasExited) 'cancel leaves old process running'
    Assert (!(Test-Path (Join-Path $root 'backups'))) 'cancel creates no backup'
    # Upgrade into an existing folder, stop its real native process, change both
    # TS ports through the actual JSON/stdin CLI, retain the key and restart.
    Answers @('','','','Y','19987','30034','51000','51099','Y','')
    Invoke-NodeInstall $root $PackageRoot
    Assert ($script:answerIndex -eq $script:answers.Count) 'all documented questions consumed'
    $oldProcess.Refresh();Assert $oldProcess.HasExited 'own running node automatically stopped'
    Assert ($script:stopped -contains $oldProcess.Id) 'exact PID stopped'
    Assert ($script:started -and !$script:started.HasExited) 'native node restarted'
    $current=Invoke-NodeCli $root 'inspect-node'|ConvertFrom-Json
    Assert ($current.target -eq '127.0.0.1:19987' -and $current.file_transfer_port -eq 30034) 'requested local TS ports saved'
    Assert ([IO.File]::ReadAllText($key) -eq $oldKey) 'existing pairing key retained'
    Assert ($script:registered.Principal.UserId -eq 'S-1-5-19' -and $script:registered.Principal.RunLevel -eq 'Limited') 'non-admin runtime principal planned'
    Assert ($script:registered.Settings.ExecutionTimeLimit -eq [TimeSpan]::Zero) 'no default 72-hour task termination'
    Assert ($current.udp_min -eq 51000 -and $current.udp_max -eq 51099) 'custom voice range saved'
    Assert ($script:rules.Count -eq 1 -and $script:rules[0].Protocol -eq 'UDP' -and $script:rules[0].LocalPort -eq '51000-51099' -and $script:rules[0].Program -eq (Join-Path $root 'web-ts.exe')) 'only program-scoped voice UDP planned'
    $acl=Get-Acl -LiteralPath $key
    $serviceRules=@($acl.Access|Where-Object {$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -eq 'S-1-5-19'})
    Assert ($acl.AreAccessRulesProtected -and $serviceRules.Count -eq 1 -and (($serviceRules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::Write) -eq 0)) 'key runtime access read-only'
    $backup=Get-ChildItem -LiteralPath (Join-Path $root 'backups') -Directory|Select-Object -First 1
    Assert ([IO.File]::ReadAllText((Join-Path $backup.FullName 'node.local.toml')) -eq $oldConfig) 'old configuration backed up'
    foreach($path in @((Join-Path $root 'backups'),$backup.FullName,(Join-Path $backup.FullName 'secrets\node-pair.key'))){
        $backupRules=@((Get-Acl -LiteralPath $path).Access|Where-Object {$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -eq 'S-1-5-19'})
        Assert ($backupRules.Count -eq 0) 'backup denies runtime account'
    }
    # Re-run is idempotent for the firewall and keeps its own task recognizable.
    Answers @('','','','Y','','','52000','52099','Y','')
    Invoke-NodeInstall $root $PackageRoot
    Assert ($script:rules.Count -eq 1 -and $script:ruleEnables -eq 1) 'no duplicate firewall rules'
    Assert ($script:rules[0].LocalPort -eq '52000-52099') 'old voice rule replaced without widening'
    $script:rules[0].LocalPort='50000-50099'
    Enable-NodeRule $root '52000-52099' '52000-52099'
    Assert ($script:rules.Count -eq 1 -and $script:rules[0].LocalPort -eq '52000-52099') 'interrupted install can reconcile its own stale rule'
    Assert (@(Get-NodeTasks $root).Count -eq 1) 'managed task recognized on repeat'
}finally{
    foreach($process in @($oldProcess,$script:started)){
        if($process){$process.Refresh();if(!$process.HasExited){$process.Kill();$process.WaitForExit()}}
    }
}
$script:tasks=@();$script:rules=@();$fresh=Join-Path $Scratch '首次安装'
Answers @($main,'203.0.113.20','','N','Y','')
MustFail {Invoke-NodeInstall $fresh $PackageRoot} 'fresh empty pairing refused'
Assert ($script:rules.Count -eq 0) 'no firewall change after failed pairing'
$tampered=Join-Path $Scratch '篡改包';[void][IO.Directory]::CreateDirectory($tampered)
Copy-NodePayload $PackageRoot $tampered
[IO.File]::AppendAllText((Join-Path $tampered 'webts-node.cmd'),'invalid')
MustFail {Assert-NodeManifest $tampered} 'tampered payload refused before stop'
Write-Output '{"powershell":"5.1","native_config_process_and_http":"PASS","real_project_only_acls":"PASS","scheduler_and_firewall_mutations":"MOCKED","foreign_resources_preserved":"PASS","repeat_install":"PASS","real_windows_server_and_voice":"NOT RUN"}'
