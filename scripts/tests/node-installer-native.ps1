param([Parameter(Mandatory=$true)][string]$PackageRoot,[Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Only an isolated GitHub Windows runner may mutate test task/firewall rules.'}
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1')
. (Join-Path $PackageRoot 'scripts\node-install.ps1')
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Native installer acceptance requires an elevated isolated runner.'}
if(Get-ScheduledTask -TaskName 'WebTS-Node' -ErrorAction SilentlyContinue){throw 'Preserve existing node task; this runner is not isolated.'}
$root=Join-Path $Scratch '真实任务 Chinese space'
[void][IO.Directory]::CreateDirectory($root)
Copy-NodePayload $PackageRoot $root
& (Join-Path $root 'scripts\node.ps1') init | Out-Null
$listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$listener.Start()
$port=$listener.LocalEndpoint.Port;$listener.Stop()
$config=Join-Path $root 'node.local.toml'
$text=[IO.File]::ReadAllText($config,[Text.Encoding]::UTF8).Replace('127.0.0.1:8095',('127.0.0.1:'+$port)).Replace('https://web.example.com','https://127.0.0.1:1').Replace('203.0.113.10','203.0.113.20')
[IO.File]::WriteAllText($config,$text,[Text.UTF8Encoding]::new($false))
$script:answers=@('','','','N','Y','');$script:index=0
function Read-Host {
    param($Prompt,[switch]$AsSecureString)
    if($script:index -ge $script:answers.Count){throw 'Unexpected native installer question'}
    $answer=$script:answers[$script:index];$script:index++
    if($AsSecureString){return [Security.SecureString]::new()}
    return $answer
}
$ruleName=Get-NodeRuleName $root
if(Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue){throw 'Preserve existing rule; runner is not isolated.'}
try{
    Invoke-NodeInstall $root $PackageRoot
    $task=Get-ScheduledTask -TaskName 'WebTS-Node'
    if([Xml.XmlConvert]::ToTimeSpan($task.Settings.ExecutionTimeLimit) -ne [TimeSpan]::Zero){throw 'Native task execution timeout is not disabled'}
    Assert-NodeRule $root '50000-50099'
    $processes=@(Get-NetTCPConnection -State Listen -LocalPort $port|ForEach-Object {Get-CimInstance Win32_Process -Filter ('ProcessId='+$_.OwningProcess)})
    if($processes.Count -ne 1 -or !(Test-NodeProcess $processes[0] $root)){throw 'Native task did not start this node'}
    $owner=Invoke-CimMethod -InputObject $processes[0] -MethodName GetOwnerSid
    if($owner.ReturnValue -ne 0 -or $owner.Sid -ne 'S-1-5-19'){throw 'Native node is not running as LOCAL SERVICE'}
    # Change only our own rule on a real reinstallation; do not accumulate ranges.
    $script:answers=@('','','','Y','','','51000','51099','Y','');$script:index=0
    Invoke-NodeInstall $root $PackageRoot
    Assert-NodeRule $root '51000-51099'
    if(@(Get-NetFirewallRule -Name $ruleName).Count -ne 1){throw 'Duplicate native firewall rule'}
    Write-Output 'NATIVE PASS: task registration, LOCAL SERVICE process, read-only startup, real UDP rule and range replacement. No real TS voice.'
}finally{
    # Never touch an unrelated task or rule, even during failed acceptance.
    $tasks=@(Get-NodeTasks $root)
    Stop-OwnNode $root $tasks $port
    foreach($task in $tasks){Unregister-ScheduledTask -TaskName $task.TaskName -TaskPath $task.TaskPath -Confirm:$false}
    $rule=Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue
    if($rule){
        $program=@($rule|Get-NetFirewallApplicationFilter)
        if($rule.Description -ne $root -or $program.Count -ne 1 -or $program[0].Program -ne (Join-Path $root 'web-ts.exe')){throw 'Refuse cleanup of an unrelated firewall rule'}
        Remove-NetFirewallRule -Name $ruleName
    }
}
