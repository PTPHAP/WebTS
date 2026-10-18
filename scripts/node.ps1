param([ValidateSet('init','configure','check','start','update','help')][string]$Action='help')
$ErrorActionPreference='Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
$nodeRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $nodeRoot
$nodeBinary=Join-Path $nodeRoot 'web-ts.exe'
$nodeConfig=Join-Path $nodeRoot 'node.local.toml'
$nodeKey=Join-Path $nodeRoot 'secrets\node-pair.key'
if(!(Test-Path -LiteralPath $nodeBinary -PathType Leaf)){throw '请使用包含 web-ts.exe 的 Windows 节点包。'}
switch($Action){
    'init' {
        if(!(Test-Path -LiteralPath $nodeConfig)){
            $bytes=[IO.File]::ReadAllBytes((Join-Path $nodeRoot 'node.example.toml'))
            $file=[IO.File]::Open($nodeConfig,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
            try{$file.Write($bytes,0,$bytes.Length)}finally{$file.Dispose()}
        }
        if(!(Test-Path -LiteralPath $nodeKey)){
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $nodeKey) | Out-Null
            & $nodeBinary init-key $nodeKey
            if($LASTEXITCODE -ne 0){throw '创建节点配对密钥失败，不会覆盖已有文件。'}
            $acl=[Security.AccessControl.FileSecurity]::new()
            $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
            $acl.SetOwner($sid)
            $acl.SetAccessRuleProtection($true,$false)
            foreach($identity in @($sid,([Security.Principal.SecurityIdentifier]::new('S-1-5-18')),([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')))){
                $rule=[Security.AccessControl.FileSystemAccessRule]::new($identity,[Security.AccessControl.FileSystemRights]::FullControl,[Security.AccessControl.AccessControlType]::Allow)
                $acl.AddAccessRule($rule)
            }
            try{Set-Acl -LiteralPath $nodeKey -AclObject $acl}
            catch{
                # This invocation created this file; do not leave an unprotected new key.
                Remove-Item -LiteralPath $nodeKey -Force
                throw
            }
        }
        Write-Output '初始化完成；已有配置与密钥不会替换。请核对本机TS/文件端口，再运行：webts-node.cmd configure'
        Write-Output '配对码在主站后台生成，勿发群聊或上传Git；TS机器不需要公开节点HTTPS入口。'
    }
    'configure' {
        if(!(Test-Path -LiteralPath $nodeConfig)){throw '请先运行 webts-node.cmd init。'}
        $nodeMain=Read-Host 'WebTS主站HTTPS地址（例如 https://web.timemirror.top）'
        $nodePublic=Read-Host '语音公网IPv4（回车保留原配置；中继策略请按文档配置）'
        $nodeId=Read-Host '节点标识（与主站路线一致；回车保留原配置）'
        $nodeSecure=Read-Host '后台5分钟一次性配对码（回车保留已有配对密钥）' -AsSecureString
        $nodePointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($nodeSecure)
        $nodePlain=$null
        try{
            $nodePlain=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($nodePointer)
            $nodePayload=@{main_url=$nodeMain;code=$nodePlain}
            if($nodeId){$nodePayload.id=$nodeId}
            if($nodePublic){$nodePayload.public_ip=$nodePublic}
            $nodeInfo=[Diagnostics.ProcessStartInfo]::new()
            $nodeInfo.FileName=$nodeBinary
            $nodeInfo.Arguments='configure-node "'+$nodeConfig+'"'
            $nodeInfo.WorkingDirectory=$nodeRoot
            $nodeInfo.UseShellExecute=$false
            $nodeInfo.CreateNoWindow=$true
            $nodeInfo.RedirectStandardInput=$true
            $nodeInfo.RedirectStandardOutput=$true
            $nodeInfo.RedirectStandardError=$true
            $nodeInfo.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
            $nodeInfo.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
            $nodeProcess=[Diagnostics.Process]::new();$nodeProcess.StartInfo=$nodeInfo
            try{
                [void]$nodeProcess.Start()
                $nodeOut=$nodeProcess.StandardOutput.ReadToEndAsync();$nodeErr=$nodeProcess.StandardError.ReadToEndAsync()
                $nodeProcess.StandardInput.WriteLine(($nodePayload|ConvertTo-Json -Compress));$nodeProcess.StandardInput.Close()
                $nodeProcess.WaitForExit()
                Write-Output $nodeOut.GetAwaiter().GetResult()
                if($nodeProcess.ExitCode -ne 0){throw $nodeErr.GetAwaiter().GetResult()}
            }finally{$nodeProcess.Dispose()}
        }finally{
            [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($nodePointer)
            $nodePlain=$null;$nodePayload=$null;$nodeSecure.Dispose()
        }
    }
    'update' { & (Join-Path $PSScriptRoot 'node-update.ps1'); exit $LASTEXITCODE }
    'check' {
        & $nodeBinary check-node $nodeConfig
        exit $LASTEXITCODE
    }
    'start' {
        & $nodeBinary check-node $nodeConfig
        if($LASTEXITCODE -ne 0){throw '配置检查失败，未启动节点。'}
        Write-Output '节点在前台运行；按 Ctrl+C 正常停止。请保持此窗口，或按文档配置专用账号的计划任务。'
        & $nodeBinary serve-node $nodeConfig
        exit $LASTEXITCODE
    }
    default {Write-Output '推荐双击“开始安装.cmd”或 webts-node.cmd：中文问答、自动备份与停启、专用UDP规则、低权限开机任务。维护命令：webts-node.cmd init | configure | check | start。配置教程见 README.md。'}
}
