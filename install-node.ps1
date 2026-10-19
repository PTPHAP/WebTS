# Windows PowerShell 5.1. Run in an administrator terminal on the TS machine.
$ErrorActionPreference='Stop'
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw '请用管理员身份打开Windows PowerShell，再运行安装命令。尚未修改节点。'}
$architecture=if($env:PROCESSOR_ARCHITEW6432){$env:PROCESSOR_ARCHITEW6432}else{$env:PROCESSOR_ARCHITECTURE}
if($architecture -ne 'AMD64'){throw '此入口仅支持Windows x64。尚未修改节点。'}
$stage=Join-Path $env:TEMP ('WebTS-Node-Bootstrap-'+[Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory((Join-Path $stage 'scripts'))
try{
    # Prevent less-privileged local accounts from replacing an elevated installer.
    $acl=[Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true,$false)
    foreach($sid in @($identity.User.Value,'S-1-5-18','S-1-5-32-544')){
        $rule=[Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow')
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $stage -AclObject $acl
    Set-Acl -LiteralPath (Join-Path $stage 'scripts') -AclObject $acl
    Add-Type -AssemblyName System.Net.Http
    $handler=[Net.Http.HttpClientHandler]::new();$handler.AllowAutoRedirect=$false
    $client=[Net.Http.HttpClient]::new($handler);$client.Timeout=[TimeSpan]::FromSeconds(60)
    try{
        foreach($name in @('node-update.ps1','node-install.ps1')){
            $url='https://raw.githubusercontent.com/PTPHAP/WebTS/v1.1.0/scripts/'+$name
            $response=$client.GetAsync($url,[Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
            try{
                if([int]$response.StatusCode -ne 200){throw '官方安装器下载失败；未修改现有节点。'}
                $stream=$response.Content.ReadAsStreamAsync().GetAwaiter().GetResult();$memory=[IO.MemoryStream]::new();$cancel=[Threading.CancellationTokenSource]::new(60000)
                try{
                    $buffer=[byte[]]::new(8192)
                    while(($n=$stream.ReadAsync($buffer,0,$buffer.Length,$cancel.Token).GetAwaiter().GetResult()) -gt 0){
                        if($memory.Length+$n -gt 1048576){throw '安装器下载大小超限。'}
                        $memory.Write($buffer,0,$n)
                    }
                    $text=[Text.UTF8Encoding]::new($false,$true).GetString($memory.ToArray())
                    [IO.File]::WriteAllText((Join-Path $stage ('scripts\'+$name)),$text,[Text.UTF8Encoding]::new($true))
                }finally{$stream.Dispose();$memory.Dispose();$cancel.Dispose()}
            }finally{$response.Dispose()}
        }
    }finally{$client.Dispose();$handler.Dispose()}
    Write-Host 'WebTS同机语音节点 · 下载校验后的官方包，进入中文配置；不会安装主站或操作TeamSpeak。'
    & (Join-Path $PSHOME 'powershell.exe') -NoProfile -ExecutionPolicy Bypass -File (Join-Path $stage 'scripts\node-update.ps1') -Bootstrap
    if($LASTEXITCODE -ne 0){throw '节点向导未完成；请保留提示及原节点backups，不要清空密钥。'}
}finally{
    # Delete only these two known public helper files; never recurse into an install directory.
    foreach($name in @('node-update.ps1','node-install.ps1')){
        $file=Join-Path $stage ('scripts\'+$name)
        if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}
    }
    Remove-Item -LiteralPath (Join-Path $stage 'scripts')
    Remove-Item -LiteralPath $stage
}
