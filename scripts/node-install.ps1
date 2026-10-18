# Windows PowerShell 5.1. Dot-sourcing loads functions without changing the machine.
$ErrorActionPreference='Stop'

function Get-NodeFullPath([string]$Path,[string]$Root) {
    if(![IO.Path]::IsPathRooted($Path)){$Path=Join-Path $Root $Path}
    return [IO.Path]::GetFullPath($Path).TrimEnd('\')
}

function Assert-NodeDirectory([string]$Root) {
    $full=Get-NodeFullPath $Root $Root
    if($full.StartsWith('\\') -or $full -eq [IO.Path]::GetPathRoot($full).TrimEnd('\')){throw '请解压到本地专用文件夹，不能使用磁盘根目录或网络共享。'}
    for($item=Get-Item -LiteralPath $full; $null -ne $item; $item=$item.Parent){
        if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw '节点目录及父目录不能是链接或联接。'}
    }
    foreach($item in Get-ChildItem -LiteralPath $full -Recurse -Force){
        if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.LinkType -eq 'HardLink'){throw '节点包内含链接、硬链接或联接，未做任何安装操作。'}
    }
    $allowed=@('web-ts.exe','webts-node.cmd','开始安装.cmd','scripts','docs','deploy','licenses','LICENSE','README.md','THIRD_PARTY_NOTICES.md','node.example.toml','node.local.toml','secrets','backups','BUILD-INFO.json','SHA256SUMS')
    foreach($item in Get-ChildItem -LiteralPath $full -Force){
        if($item.Name -notin $allowed){throw ('请把节点包单独解压，不要与其他项目或文件混放：'+$item.Name)}
    }
}

function Invoke-NodeCli([string]$Root,[string]$Action,[string]$Json,[string]$BinaryRoot=$Root,[string]$ConfigName='node.local.toml') {
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.FileName=Join-Path $BinaryRoot 'web-ts.exe'
    $info.Arguments=$Action+' "'+(Join-Path $Root $ConfigName)+'"'
    $info.WorkingDirectory=$Root;$info.UseShellExecute=$false;$info.CreateNoWindow=$true
    $info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info
    try{
        [void]$process.Start()
        $out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
        if($Json){$process.StandardInput.WriteLine($Json)}
        $process.StandardInput.Close()
        if(!$process.WaitForExit(30000)){$process.Kill();$process.WaitForExit();throw '节点配置程序超时，已停止本次配置程序；没有结束其他进程。'}
        $text=$out.GetAwaiter().GetResult();$errorText=$err.GetAwaiter().GetResult()
        if($process.ExitCode -ne 0){throw $errorText}
        return $text.Trim()
    }finally{$process.Dispose()}
}

function Copy-NodePayload([string]$Source,[string]$Root) {
    if($Source -eq $Root){return}
    foreach($line in [IO.File]::ReadAllLines((Join-Path $Source 'SHA256SUMS'),[Text.Encoding]::UTF8)){
        $name=$line.Substring(66)
        $destination=Get-NodeFullPath $name $Root
        if(!$destination.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase)){throw '安装文件路径越界。'}
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $destination))
        [IO.File]::Copy((Get-NodeFullPath $name $Source),$destination,$true)
    }
    [IO.File]::Copy((Join-Path $Source 'SHA256SUMS'),(Join-Path $Root 'SHA256SUMS'),$true)
}

function Assert-NodeManifest([string]$Root) {
    $manifest=Join-Path $Root 'SHA256SUMS'
    if(!(Test-Path -LiteralPath $manifest -PathType Leaf)){throw '缺少包内校验清单，请使用完整的新节点ZIP，不要只复制安装脚本。'}
    $seen=@{}
    foreach($line in [IO.File]::ReadAllLines($manifest,[Text.Encoding]::UTF8)){
        if($line -notmatch '^([a-f0-9]{64})  ([^\r\n]+)$'){throw '包内校验清单格式错误。'}
        $expected=$Matches[1];$name=$Matches[2]
        $path=Get-NodeFullPath $name $Root
        if(!$path.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase) -or $seen.ContainsKey($path) -or $name -match '(^|/)(secrets|backups|node\.local\.toml)(/|$)'){throw '校验清单包含重复或越界路径。'}
        $seen[$path]=$true
        $stream=[IO.File]::OpenRead($path);$sha=[Security.Cryptography.SHA256]::Create()
        try{$actual=[BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant()}finally{$stream.Dispose();$sha.Dispose()}
        if($actual -ne $expected){throw ('文件校验失败：'+$name+'；未停止节点，请重新解压已核对SHA256的新包。')}
    }
    foreach($name in @('web-ts.exe','scripts\node-install.ps1','scripts\node.ps1','node.example.toml','webts-node.cmd','开始安装.cmd')){
        if(!$seen.ContainsKey((Join-Path $Root $name))){throw ('校验清单缺少必需文件：'+$name)}
    }
}

function Test-NodeServeArguments([string]$Arguments,[string]$Root,[string]$WorkingDirectory='') {
    # No extra commands, flags, shell operators or alternative config files.
    if($Arguments -notmatch '^\s*serve-node\s+(?:"([^"\r\n]+)"|([^\s"]+))\s*$'){return $false}
    $config=if($Matches[1]){$Matches[1]}else{$Matches[2]}
    try{
        if(![IO.Path]::IsPathRooted($config) -and $WorkingDirectory -ne $Root){return $false}
        return (Get-NodeFullPath $config $Root) -eq (Join-Path $Root 'node.local.toml')
    }catch{return $false}
}

function Test-NodeProcess($Process,[string]$Root) {
    if(!$Process.ExecutablePath -or (Get-NodeFullPath $Process.ExecutablePath $Root) -ne (Join-Path $Root 'web-ts.exe')){return $false}
    if($Process.CommandLine -notmatch '^\s*(?:"[^"\r\n]+"|[^\s"]+)\s+(.+)$'){return $false}
    return Test-NodeServeArguments $Matches[1] $Root
}

function Get-NodeTasks([string]$Root) {
    $own=@()
    foreach($task in Get-ScheduledTask){
        $actions=@($task.Actions)
        if($actions.Count -ne 1){
            if($task.TaskName -eq 'WebTS-Node' -and $task.TaskPath -eq '\'){throw '已有同名任务包含多个动作，无法确认归属，未覆盖。'}
            continue
        }
        $action=$actions[0]
        # ScheduledTask.Execute can be quoted, an environment expression or a
        # non-file action. Never run Path APIs on unrelated system task fields.
        $execute=[string]$action.Execute
        if($execute -match '^"([^"\r\n]+)"$'){$execute=$Matches[1]}
        $isOwn=($execute.Replace('/','\') -eq (Join-Path $Root 'web-ts.exe')) -and (Test-NodeServeArguments $action.Arguments $Root $action.WorkingDirectory)
        if($task.TaskName -eq 'WebTS-Node' -and $task.TaskPath -eq '\' -and !$isOwn){throw '已有 WebTS-Node 任务指向其他程序或配置，未覆盖。'}
        if($isOwn){$own+=$task}
    }
    return $own
}

function Stop-OwnNode([string]$Root,$Tasks,[int]$Port) {
    # Check the listener owner BEFORE stopping anything, and re-check PID identity before killing.
    $listeners=@(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
    foreach($listener in $listeners){
        $process=Get-CimInstance Win32_Process -Filter ('ProcessId='+$listener.OwningProcess)
        if(!$process -or !(Test-NodeProcess $process $Root)){throw ('本机控制端口 '+$Port+' 被其他程序或无法确认归属的进程占用；未停止它。')}
    }
    foreach($task in $Tasks){Stop-ScheduledTask -InputObject $task}
    foreach($process in @(Get-CimInstance Win32_Process -Filter "Name='web-ts.exe'")){
        if(Test-NodeProcess $process $Root){
            $current=Get-CimInstance Win32_Process -Filter ('ProcessId='+$process.ProcessId)
            if($current -and $current.CreationDate -eq $process.CreationDate -and (Test-NodeProcess $current $Root)){
                Stop-Process -Id $current.ProcessId -Force -ErrorAction Stop
            }
        }
    }
    for($attempt=0;$attempt -lt 30;$attempt++){
        if(!@(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue).Count){return}
        Start-Sleep -Milliseconds 200
    }
    throw '节点端口仍被占用，可能有其他守护程序重新启动它；未领取配对码。'
}

function Set-NodeAcl([string]$Path,[switch]$Directory,[switch]$RuntimeRead) {
    $acl=if($Directory){[Security.AccessControl.DirectorySecurity]::new()}else{[Security.AccessControl.FileSecurity]::new()}
    $me=[Security.Principal.WindowsIdentity]::GetCurrent().User
    $existing=if($Directory){[IO.Directory]::GetAccessControl($Path,[Security.AccessControl.AccessControlSections]::Owner)}else{[IO.File]::GetAccessControl($Path,[Security.AccessControl.AccessControlSections]::Owner)}
    if($existing.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin @($me.Value,'S-1-5-18','S-1-5-32-544')){throw '节点文件归其他账号所有，未接管所有权；请使用原维护账号安装。'}
    $acl.SetAccessRuleProtection($true,$false)
    foreach($sid in @($me,([Security.Principal.SecurityIdentifier]::new('S-1-5-18')),([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')))){
        if($Directory){$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow')}
        else{$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','Allow')}
        [void]$acl.AddAccessRule($rule)
    }
    if($RuntimeRead){
        $sid=[Security.Principal.SecurityIdentifier]::new('S-1-5-19')
        if($Directory){$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow')}
        else{$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,'ReadAndExecute','Allow')}
        [void]$acl.AddAccessRule($rule)
    }
    # Write only our DACL through .NET without PowerShell's provider trying to
    # rewrite the existing SACL (which requires unrelated SeSecurityPrivilege).
    if($Directory){[IO.Directory]::SetAccessControl($Path,$acl)}else{[IO.File]::SetAccessControl($Path,$acl)}
}

function Get-NodeRuleName([string]$Root) {
    $sha=[Security.Cryptography.SHA256]::Create()
    try{$hash=[BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Root.ToLowerInvariant()))).Replace('-','').Substring(0,12)}finally{$sha.Dispose()}
    return 'WebTS-Node-Voice-'+$hash
}

function Assert-NodeRule([string]$Root,[string]$Ports) {
    $rule=Get-NetFirewallRule -Name (Get-NodeRuleName $Root) -ErrorAction SilentlyContinue
    if(!$rule){return}
    $program=@($rule|Get-NetFirewallApplicationFilter);$port=@($rule|Get-NetFirewallPortFilter)
    if($rule.Group -ne 'WebTS Node' -or $rule.Description -ne $Root -or $rule.Direction -ne 'Inbound' -or $rule.Action -ne 'Allow' -or
       $program.Count -ne 1 -or $program[0].Program -ne (Join-Path $Root 'web-ts.exe') -or $port.Count -ne 1 -or
       "$($port[0].Protocol)" -notin @('UDP','17') -or "$($port[0].LocalPort)" -ne $Ports){throw '已有节点专用防火墙规则与此目录或端口不符，未扩大或覆盖规则。'}
}

function Enable-NodeRule([string]$Root,[string]$Ports,[string]$PreviousPorts=$Ports) {
    Assert-NodeRule $Root $PreviousPorts
    $name=Get-NodeRuleName $Root
    if(Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue){
        Set-NetFirewallRule -Name $name -Protocol UDP -LocalPort $Ports -Enabled True|Out-Null
    }
    else{New-NetFirewallRule -Name $name -DisplayName 'WebTS 节点加密语音 UDP' -Group 'WebTS Node' -Description $Root -Direction Inbound -Action Allow -Protocol UDP -LocalPort $Ports -Program (Join-Path $Root 'web-ts.exe') -Profile Any -Enabled True|Out-Null}
}

function Read-NodeValue([string]$Prompt,[string]$Default) {
    $answer=Read-Host ($Prompt+$(if($Default){' ['+$Default+']'}else{''}))
    if([string]::IsNullOrWhiteSpace($answer)){return $Default}
    return $answer.Trim()
}

function Resolve-NodePublicIp([string]$Value) {
    $ip=$null
    if(![Net.IPAddress]::TryParse($Value,[ref]$ip)){
        if([Uri]::CheckHostName($Value) -ne [UriHostNameType]::Dns){throw '请填写公网IPv4或域名，不带协议、路径或端口。'}
        $addresses=@([Net.Dns]::GetHostAddresses($Value)|Where-Object {$_.AddressFamily -eq 'InterNetwork'}|Select-Object -ExpandProperty IPAddressToString -Unique)
        if($addresses.Count -ne 1){throw '域名未解析到唯一IPv4，请填写这台服务器控制台显示的公网IPv4。'}
        $ip=[Net.IPAddress]::Parse($addresses[0])
    }
    $b=$ip.GetAddressBytes()
    if($ip.AddressFamily -ne 'InterNetwork' -or $b[0] -in @(0,10,127) -or $b[0] -ge 224 -or ($b[0] -eq 169 -and $b[1] -eq 254) -or ($b[0] -eq 172 -and $b[1] -ge 16 -and $b[1] -le 31) -or ($b[0] -eq 192 -and $b[1] -eq 168) -or ($b[0] -eq 100 -and $b[1] -ge 64 -and $b[1] -le 127)){
        throw '浏览器需要连接服务器的真实公网IPv4，不能使用127.0.0.1、内网或共享地址。'
    }
    return $ip.IPAddressToString
}

function Invoke-NodeInstall([string]$Root,[string]$Source=$Root) {
    $Root=Get-NodeFullPath $Root $Root
    $Source=Get-NodeFullPath $Source $Source
    Assert-NodeDirectory $Source
    Assert-NodeManifest $Source
    if($Root -ne $Source -and ($Root.StartsWith($Source+'\',[StringComparison]::OrdinalIgnoreCase) -or $Source.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase))){throw '安装目录与解压目录不能互相嵌套。'}
    if(!(Test-Path -LiteralPath $Root)){[void][IO.Directory]::CreateDirectory($Root)}
    Assert-NodeDirectory $Root
    Set-Location -LiteralPath $Root
    Write-Host "`n========== WebTS 同机语音节点 · 中文安装 ==========" -ForegroundColor Cyan
    Write-Host '本程序只管理此文件夹的节点、专用计划任务及语音UDP规则。不会修改TeamSpeak或其他服务。'
    Write-Host '节点能接触授权身份和声音，请只安装在可信服务器。云安全组须由云控制台放行。'
    $hadConfig=Test-Path -LiteralPath (Join-Path $Root 'node.local.toml') -PathType Leaf
    if($hadConfig){$config=Invoke-NodeCli $Root 'inspect-node' '' $Source|ConvertFrom-Json}
    else{$config=Invoke-NodeCli $Source 'inspect-node' '' $Source 'node.example.toml'|ConvertFrom-Json}
    $key=Get-NodeFullPath $config.key_file $Root
    if(!$key.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase)){throw '现有密钥在节点目录之外，自动安装不会修改外部文件。'}
    # A dedicated directory is also required for its least-privilege task ACL.
    $allowedSecrets=@('node-pair.key')
    if(Test-Path -LiteralPath (Join-Path $Root 'secrets')){
        foreach($file in Get-ChildItem -LiteralPath (Join-Path $Root 'secrets') -Force){
            if($file.Name -notin $allowedSecrets -or $file.PSIsContainer){throw 'secrets 中有其他文件，自动安装不会扩大其读取权限。'}
        }
    }
    if($key -ne (Join-Path $Root 'secrets\node-pair.key')){throw '自动安装仅管理 secrets\node-pair.key，不修改其他密钥。'}
    $mainDefault=if($config.main_url -and $config.main_url -notmatch 'example\.(com|org|net)'){$config.main_url}else{''}
    $main=Read-NodeValue '① WebTS主站网址（浏览器登录的HTTPS网址，例如 https://web.example.com）' $mainDefault
    $uri=$null
    if(![Uri]::TryCreate($main,[UriKind]::Absolute,[ref]$uri) -or $uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/'){throw '主站须为HTTPS根网址，不能含密码、路径或参数。'}
    $ipDefault=if($config.public_ip -and $config.public_ip -ne '203.0.113.10'){$config.public_ip}else{''}
    $publicIp=Resolve-NodePublicIp (Read-NodeValue '② 本机语音公网IPv4或域名（云控制台的公网IP；不能填127.0.0.1）' $ipDefault)
    # IPEndPoint.Parse is not available in the Windows PowerShell 5.1 CLR.
    # These socket strings have already passed the native TOML validation.
    $tsPort=[int]($config.target -replace '^.*:','');$filePort=[int]$config.file_transfer_port
    Write-Host ('当前TS：'+$config.target+'；头像/图片文件端口：'+$filePort+'；节点ID：'+$config.id)
    $udpMin=[int]$config.udp_min;$udpMax=[int]$config.udp_max
    $oldPorts=if($udpMin -eq $udpMax){[string]$udpMin}else{"$udpMin-$udpMax"}
    if((Read-NodeValue '是否自定义TS及网页语音端口？默认沿用；要换端口选Y（Y/N）' 'N') -match '^(y|yes|是)$'){
        $tsPort=0;$filePort=0
        if(![int]::TryParse((Read-NodeValue 'TS语音UDP端口' ($config.target -replace '^.*:','')),[ref]$tsPort) -or $tsPort -lt 1 -or $tsPort -gt 65535){throw 'TS端口须为1–65535。'}
        if(![int]::TryParse((Read-NodeValue 'TS文件TCP端口（头像、频道图片；不是ServerQuery端口）' $config.file_transfer_port),[ref]$filePort) -or $filePort -lt 1 -or $filePort -gt 65535){throw 'TS文件端口须为1–65535。'}
        if(![int]::TryParse((Read-NodeValue '本机网页语音UDP起始端口（云安全组和Windows入站使用这段）' $config.udp_min),[ref]$udpMin) -or $udpMin -lt 1 -or $udpMin -gt 65535){throw 'UDP起始端口须为1–65535。'}
        if(![int]::TryParse((Read-NodeValue '本机网页语音UDP结束端口' $config.udp_max),[ref]$udpMax) -or $udpMax -lt $udpMin -or $udpMax -gt 65535 -or ($udpMax-$udpMin+1) -lt $config.max_connections){throw ('UDP范围必须递增、位于1–65535并至少容纳 '+$config.max_connections+' 个连接。')}
    }
    $port=[int]($config.bind -replace '^.*:','')
    $ports=if($udpMin -eq $udpMax){[string]$udpMin}else{"$udpMin-$udpMax"}
    $tasks=@(Get-NodeTasks $Root)
    Assert-NodeRule $Root $oldPorts
    Write-Host "`n将自动完成：备份 → 停止此节点 → 配对 → 专用读取权限 → UDP入站 $ports → 低权限开机任务 → 本机检查。"
    Write-Host '重新配置会短暂中断本节点上的网页通话；不重启TS，不公开节点HTTP/HTTPS。'
    Write-Host ('语音公网IP：'+$publicIp+'；TS仅本机端口：'+$tsPort+'；文件端口：'+$filePort)
    Write-Host ('[当前TS服务器] 云安全组：入站 UDP '+$ports+'；Windows防火墙：向导自动添加这条程序专用规则。') -ForegroundColor Yellow
    Write-Host ('[WebTS主站 '+$uri.Host+'] 保持现有HTTPS入站 TCP '+$uri.Port+'；不需要给主站新增这段语音UDP。')
    Write-Host ('[当前TS服务器] 需要出站连主站 TCP '+$uri.Port+'；默认允许出站则无需新增规则。')
    Write-Host ('[本机内部] '+$config.bind+' 是控制监听，不对公网开放；TS/文件端口保持原规则。')
    Write-Host '[用户手机/电脑] 不需要手动开放入站端口。'
    if((Read-NodeValue '确认继续？（Y/N）' 'Y') -notmatch '^(y|yes|是)$'){Write-Host '已取消，未停止节点。';return}
    $backup=Join-Path $Root ('backups\'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fffffff'))
    [void](New-Item -ItemType Directory -Path $backup)
    Set-NodeAcl (Join-Path $Root 'backups') -Directory
    Set-NodeAcl $backup -Directory
    foreach($name in @('node.local.toml','secrets\node-pair.key','web-ts.exe','webts-node.cmd','scripts\node.ps1','scripts\node-install.ps1')){
        $path=Join-Path $Root $name
        if(Test-Path -LiteralPath $path -PathType Leaf){
            $destination=Join-Path $backup $name
            [void][IO.Directory]::CreateDirectory((Split-Path -Parent $destination))
            Copy-Item -LiteralPath $path -Destination $destination
        }
    }
    foreach($task in $tasks){Export-ScheduledTask -TaskName $task.TaskName -TaskPath $task.TaskPath|Set-Content -LiteralPath (Join-Path $backup ('task-'+[Guid]::NewGuid().ToString('N')+'.xml')) -Encoding Unicode}
    Stop-OwnNode $Root $tasks $port
    Copy-NodePayload $Source $Root
    & (Join-Path $Root 'scripts\node.ps1') init | Out-Null
    Write-Host "`n③ 请在主站：站点管理 → 基础配置 → 同机语音节点 → 验证网站管理员密码 → 生成5分钟配对码。"
    Write-Host '配对码只在此输入，不发聊天、不写文件。已经成功配对且主站没换密钥时，回车保留。'
    $secure=Read-Host '一次性配对码（输入时隐藏）' -AsSecureString
    $pointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    $payload=$null;$plain=$null
    try{
        $plain=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
        if(!$plain -and (!$hadConfig -or !$mainDefault -or $main.TrimEnd('/') -ne $mainDefault.TrimEnd('/'))){throw '首次安装或更换主站必须输入配对码；旧密钥不能自动视为已配对。'}
        $payload=@{main_url=$main;public_ip=$publicIp;code=$plain;ts_port=$tsPort;file_transfer_port=$filePort;udp_min=$udpMin;udp_max=$udpMax}|ConvertTo-Json -Compress
        [void](Invoke-NodeCli $Root 'configure-node' $payload)
    }catch{
        Write-Host ('配对失败，节点保持停止；旧配置/密钥备份在 '+$backup+'。可重新双击安装，不用修改配置文件。') -ForegroundColor Yellow
        throw
    }finally{
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
        $payload=$null;$plain=$null;$secure.Dispose()
    }
    [void](Invoke-NodeCli $Root 'check-node')
    Set-NodeAcl $Root -Directory -RuntimeRead
    Set-NodeAcl (Join-Path $Root 'secrets') -Directory -RuntimeRead
    Set-NodeAcl (Join-Path $Root 'node.local.toml') -RuntimeRead
    Set-NodeAcl $key -RuntimeRead
    Enable-NodeRule $Root $ports $oldPorts
    $action=New-ScheduledTaskAction -Execute (Join-Path $Root 'web-ts.exe') -Argument ('serve-node "'+(Join-Path $Root 'node.local.toml')+'"') -WorkingDirectory $Root
    $principal=New-ScheduledTaskPrincipal -UserId 'S-1-5-19' -LogonType ServiceAccount -RunLevel Limited
    $settings=New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
    # Preserve matching legacy tasks, but disable them so two supervisors cannot race.
    foreach($task in $tasks){if($task.TaskName -ne 'WebTS-Node' -or $task.TaskPath -ne '\'){Disable-ScheduledTask -InputObject $task|Out-Null}}
    [void](Get-NodeTasks $Root) # Refuse a conflicting task created since preflight.
    Register-ScheduledTask -TaskName 'WebTS-Node' -TaskPath '\' -Action $action -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $principal -Settings $settings -Description ('WebTS node; '+$Root) -Force|Out-Null
    Start-ScheduledTask -TaskName 'WebTS-Node' -TaskPath '\'
    $healthy=$false
    for($attempt=0;$attempt -lt 15;$attempt++){
        try{
            $health=Invoke-RestMethod -Uri ('http://'+$config.bind+'/health') -TimeoutSec 1 -UseBasicParsing
            if($health.role -eq 'voice-node'){
                $listeners=@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction Stop)
                if($listeners.Count -and @($listeners|Where-Object {
                    $process=Get-CimInstance Win32_Process -Filter ('ProcessId='+$_.OwningProcess)
                    !$process -or !(Test-NodeProcess $process $Root)
                }).Count -eq 0){$healthy=$true;break}
            }
        }catch{}
        Start-Sleep -Milliseconds 500
    }
    if(!$healthy){throw '配置已保存，但节点启动检查失败；不要重复开端口。重新双击安装查看错误，或检查专用任务 WebTS-Node。'}
    Write-Host "`n安装完成：本机节点已启动，计划任务为 WebTS-Node（LOCAL SERVICE，非管理员）。" -ForegroundColor Green
    Write-Host ('Windows防火墙专用UDP入站已配置：'+$ports+'；未添加TCP公网管理规则。')
    Write-Host ('重要：云安全组请允许 入站 UDP '+$ports+'。本程序不能替你修改云控制台，也不能仅凭本机健康判断公网通话成功。') -ForegroundColor Yellow
    Write-Host '出站需要主站HTTPS及正常DNS；默认允许出站无需新增规则。本程序不修改整机出站策略。'
    Write-Host '最后到主站查看节点在线，选择绑定此节点的服务器，小规模测试双向语音。'
    Write-Host ('配置与密钥备份：'+$backup+'（只有维护账号、SYSTEM及管理员可读）。')
}

if($MyInvocation.InvocationName -ne '.'){
    $mutex=$null;$locked=$false
    try{
        foreach($module in @('Microsoft.PowerShell.Security','ScheduledTasks','NetSecurity','NetTCPIP','CimCmdlets')){
            Import-Module (Join-Path $PSHOME ('Modules\'+$module+'\'+$module+'.psd1')) -ErrorAction Stop
        }
        $source=Split-Path -Parent $PSScriptRoot
        $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
        if(!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
            $script=Join-Path $PSScriptRoot 'node-install.ps1'
            $child=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -File "'+$script+'"') -Verb RunAs -Wait -PassThru
            exit $child.ExitCode
        }
        $root=Read-NodeValue '安装目录（已有节点请使用原目录；会保留原配置和密钥）' $(if(Test-Path -LiteralPath (Join-Path $source 'node.local.toml')){$source}else{Join-Path $env:SystemDrive 'WebTS-Node'})
        $root=Get-NodeFullPath $root $source
        $mutex=[Threading.Mutex]::new($false,'Global\WebTS-Node-Install')
        try{$locked=$mutex.WaitOne(0)}catch [Threading.AbandonedMutexException]{$locked=$true}
        if(!$locked){throw '此机器已有节点安装向导打开，请返回原窗口完成操作，不会同时运行两次。'}
        Invoke-NodeInstall $root $source
        [void](Read-Host '按回车关闭窗口')
    }catch{
        Write-Host ('安装未完成：'+$_.Exception.Message) -ForegroundColor Red
        Write-Host '没有重启或修改TeamSpeak。请保留此错误；不要关闭防火墙、公开8095或清空其他规则。'
        [void](Read-Host '按回车关闭窗口')
        exit 1
    }finally{
        if($locked){$mutex.ReleaseMutex()}
        if($mutex){$mutex.Dispose()}
    }
}
