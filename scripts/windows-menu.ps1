param(
    [ValidateSet('menu','setup','start','status','admin','backup')][string]$Action = 'menu',
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [hashtable]$Configuration,
    [Security.SecureString]$SmtpPassword
)
$ErrorActionPreference = 'Stop'
$Root = [IO.Path]::GetFullPath($Root)
$binary = Join-Path $Root 'web-ts.exe'
$config = Join-Path $Root 'config.local.toml'
$utf8 = New-Object Text.UTF8Encoding($false)
$OutputEncoding = $utf8
[Console]::OutputEncoding = $utf8

function Assert-Directory {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw '安装目录不能是链接或重解析点。' }
    foreach ($child in Get-ChildItem -LiteralPath $Path -Force -Recurse) {
        if ($child.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw '目录中存在链接；请移至独立的普通目录后操作。' }
    }
}
function Protect-Directory {
    param([string]$Path)
    Assert-Directory $Path
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $acl = Get-Acl -LiteralPath $Path
    $acl.SetAccessRuleProtection($true,$false)
    foreach ($existing in @($acl.Access)) { $acl.RemoveAccessRuleAll($existing) }
    foreach ($identity in @($sid,(New-Object Security.Principal.SecurityIdentifier('S-1-5-18')))) {
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($identity,'FullControl','ContainerInherit,ObjectInherit','None','Allow')
        $acl.AddAccessRule($rule)
    }
    if ($PSVersionTable.PSVersion.Major -ge 6) {
        [IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($Path),$acl)
    } else {
        [IO.Directory]::SetAccessControl($Path,$acl)
    }
}
function Invoke-Gateway {
    param([string[]]$Arguments)
    & $binary @Arguments
    if ($LASTEXITCODE -ne 0) { throw '网关操作失败；请检查上方提示。' }
}
function Assert-Stopped {
    foreach ($process in Get-Process -Name 'web-ts' -ErrorAction SilentlyContinue) {
        try { $path = $process.Path } catch { throw '无法确认网关已停止，请先关闭网关再操作。' }
        if (!$path -or [IO.Path]::GetFullPath($path) -eq $binary) { throw '请先在网关窗口按 Ctrl+C，等待退出后再操作。' }
    }
}
function Ask {
    param([string]$Title,[string]$Default = '')
    $value = Read-Host "$Title [$Default]"
    if ([string]::IsNullOrWhiteSpace($value)) { return $Default }
    return $value.Trim()
}
function Quote-Toml { param([string]$Value); return ConvertTo-Json -InputObject $Value -Compress }
function Invoke-Settings {
    param([string]$Command,[string]$InputText = '')
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $binary
    $info.Arguments = "$Command config.local.toml"
    $info.WorkingDirectory = $Root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = $utf8
    $info.StandardErrorEncoding = $utf8
    $process = [Diagnostics.Process]::Start($info)
    try {
        $output = $process.StandardOutput.ReadToEndAsync()
        $errors = $process.StandardError.ReadToEndAsync()
        $bytes = $utf8.GetBytes($InputText)
        $process.StandardInput.BaseStream.Write($bytes,0,$bytes.Length)
        $process.StandardInput.Close()
        if (!$process.WaitForExit(30000)) { $process.Kill(); throw '配置操作超时。' }
        if ($process.ExitCode -ne 0) { throw '配置校验失败；请检查基础配置，不要重建密钥。' }
        return $output.GetAwaiter().GetResult()
    } finally { $process.Dispose() }
}
function Initialize-WebTS {
    Assert-Stopped
    if ((Test-Path -LiteralPath $config) -or (Test-Path -LiteralPath (Join-Path $Root 'data/web-ts.db')) -or (Test-Path -LiteralPath (Join-Path $Root 'secrets/master.key'))) { throw '已有配置或数据，不重复初始化。修改配置请查阅 docs/WINDOWS.md。' }
    if (!$Configuration) {
        Write-Host '本机 HTTP 仅供本机体验；公开网站需要 HTTPS 代理和可达公网 UDP。'
        $Configuration = @{
            url = Ask '网站地址（无路径）' 'http://localhost:8080'
            public_ip = Ask 'WebRTC 公网 IPv4（本机测试可留空）'
            server = Ask '默认 TS 公网地址:UDP端口（可留空）'
            site_name = Ask '站点名称' 'WebTS'
            operator = Ask '公开运营者名称'
            contact = Ask '公开联系途径'
            data_details = Ask '数据处理与保留说明'
            smtp_host = Ask 'TLS SMTP 主机（未填不能注册或找回）'
        }
        if ($Configuration.smtp_host) {
            $Configuration.smtp_port = Ask 'SMTP 端口' '465'
            $Configuration.smtp_user = Ask 'SMTP 用户名'
            $Configuration.smtp_from = Ask '发件邮箱'
            $SmtpPassword = Read-Host 'SMTP 授权码（隐藏输入）' -AsSecureString
        }
    }
    $url = [Uri]$Configuration.url
    $local = $url.Scheme -eq 'http' -and $url.Host -in @('localhost','127.0.0.1','::1')
    if ((!$local -and $url.Scheme -ne 'https') -or $url.AbsolutePath -ne '/' -or $url.Query -or $url.Fragment -or $url.UserInfo) { throw '网站地址必须是 HTTPS 站点或本机 HTTP，不含路径、凭据和查询参数。' }
    if ($Configuration.smtp_host -and (!$SmtpPassword -or $SmtpPassword.Length -eq 0)) { throw 'SMTP 需要授权码。' }
    Protect-Directory $Root
    foreach ($folder in @('data','secrets')) { New-Item -ItemType Directory -Path (Join-Path $Root $folder) -ErrorAction Stop | Out-Null }
    $bindPort = if ($local) { $url.Port } else { 8080 }
    $lines = @("bind = $(Quote-Toml ('127.0.0.1:'+$bindPort))","public_url = $(Quote-Toml $Configuration.url)",'database = "data/web-ts.db"','master_key_file = "secrets/master.key"','web_dir = "web/dist"','max_connections = 50',"allow_insecure_localhost = $($local.ToString().ToLowerInvariant())")
    if (!$local) { $lines += 'trusted_proxy = ["127.0.0.1", "::1"]' }
    if ($Configuration.server) { $lines += @('[[servers]]','id = "community"','name = "社区 TeamSpeak"',"address = $(Quote-Toml $Configuration.server)") }
    if ($Configuration.smtp_host) {
        $port = [int]$Configuration.smtp_port
        if ($port -lt 1 -or $port -gt 65535) { throw 'SMTP 端口无效。' }
        $lines += @('[smtp]',"host = $(Quote-Toml $Configuration.smtp_host)","port = $port","username = $(Quote-Toml $Configuration.smtp_user)","from = $(Quote-Toml $Configuration.smtp_from)",'password_file = "secrets/smtp-password"')
        $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($SmtpPassword)
        try { [IO.File]::WriteAllText((Join-Path $Root 'secrets/smtp-password'),[Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer),$utf8) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer) }
    }
    $lines += @('[rtc]',"public_ip = $(Quote-Toml $Configuration.public_ip)",'udp_min = 40000','udp_max = 40100')
    [IO.File]::WriteAllText($config,($lines -join "`n")+"`n",$utf8)
    Invoke-Gateway @('init-key','secrets/master.key')
    # Capture settings in memory: get-settings may include SMTP credentials.
    $settingsText = Invoke-Settings 'get-settings'
    $settings = $settingsText | ConvertFrom-Json
    foreach ($field in @('site_name','operator','contact','data_details')) { if ($Configuration.ContainsKey($field)) { $settings.home.$field = $Configuration[$field] } }
    Invoke-Settings 'set-settings' ($settings | ConvertTo-Json -Depth 20 -Compress) | Out-Null
    $settings = $null; $settingsText = $null
    Write-Host '初始化完成。运行 webts start；注册并验证邮箱后运行 webts admin 开通站长。'
}
function Backup-WebTS {
    Assert-Stopped
    if (!(Test-Path -LiteralPath $config)) { throw '尚未初始化。' }
    $destination = Join-Path $Root ('backups/'+(Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
    New-Item -ItemType Directory -Path $destination -ErrorAction Stop | Out-Null
    Protect-Directory $destination
    foreach ($name in @('config.local.toml','data','secrets')) { Copy-Item -LiteralPath (Join-Path $Root $name) -Destination $destination -Recurse -ErrorAction Stop }
    Write-Host "已停服备份至 $destination。含密钥和凭据，请移至加密且受控的备份位置。"
}
Assert-Directory $Root
if (!(Test-Path -LiteralPath $binary -PathType Leaf)) { throw '请使用完整 Windows 运行包，不能直接运行源码目录菜单。' }
Push-Location -LiteralPath $Root
try {
    do {
        $selected = $Action
        if ($Action -eq 'menu') {
            Write-Host "`nWebTS 中文管理：1 初始化  2 启动  3 状态  4 站长权限  5 停服备份  0 退出"
            $selected = switch (Read-Host '请选择') { '1' {'setup'} '2' {'start'} '3' {'status'} '4' {'admin'} '5' {'backup'} '0' {'exit'} default {'menu'} }
        }
        switch ($selected) {
            'setup' { Initialize-WebTS }
            'start' { Assert-Stopped; Invoke-Gateway @('serve','config.local.toml') }
            'status' {
                $binding = [regex]::Match([IO.File]::ReadAllText($config),'(?m)^bind\s*=\s*"127\.0\.0\.1:([0-9]+)"\s*$')
                if (!$binding.Success) { throw '状态工具只检查本机 IPv4 回环监听，请手动检查自定义 bind。' }
                Invoke-RestMethod -Uri ('http://127.0.0.1:'+$binding.Groups[1].Value+'/api/health') -TimeoutSec 5 | Select-Object status,version,smtp_ready | Format-List
            }
            'admin' { Assert-Stopped; Invoke-Gateway @('grant-admin','config.local.toml',(Ask '已完成邮箱验证的站长邮箱')) }
            'backup' { Backup-WebTS }
        }
    } while ($Action -eq 'menu' -and $selected -ne 'exit')
} finally { Pop-Location }
