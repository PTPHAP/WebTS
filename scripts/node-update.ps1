param([switch]$Bootstrap)
# Windows PowerShell 5.1; download and verify before invoking the existing installer.
$ErrorActionPreference='Stop'
function Get-OfficialNodeFile([string]$Url,[string]$Path,[long]$Limit) {
    Add-Type -AssemblyName System.Net.Http
    $handler=[Net.Http.HttpClientHandler]::new();$handler.AllowAutoRedirect=$false
    $client=[Net.Http.HttpClient]::new($handler);$client.Timeout=[TimeSpan]::FromMinutes(5)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd('WebTS-Node-Updater/1')
    $response=$null
    try{
        for($redirect=0;$redirect -lt 6;$redirect++){
            $uri=[Uri]$Url
            if($uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Port -ne 443 -or $uri.Host -notin @('api.github.com','github.com','release-assets.githubusercontent.com','objects.githubusercontent.com')){throw '更新下载目标不在官方允许列表中。'}
            $response=$client.GetAsync($uri,[Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
            if([int]$response.StatusCode -in @(301,302,303,307,308)){
                $next=$response.Headers.Location
                if(!$next){throw '更新下载重定向无效。'}
                $Url=([Uri]::new($uri,$next)).AbsoluteUri;$response.Dispose();$response=$null;continue
            }
            [void]$response.EnsureSuccessStatusCode()
            if($response.Content.Headers.ContentLength -gt $Limit){throw '更新文件超过大小限制。'}
            $stream=$response.Content.ReadAsStreamAsync().GetAwaiter().GetResult();$file=[IO.File]::Open($Path,[IO.FileMode]::CreateNew);$cancel=[Threading.CancellationTokenSource]::new(300000)
            try{$buffer=[byte[]]::new(65536);$size=0L;while(($n=$stream.ReadAsync($buffer,0,$buffer.Length,$cancel.Token).GetAwaiter().GetResult()) -gt 0){$size+=$n;if($size -gt $Limit){throw '更新文件超过大小限制。'};$file.Write($buffer,0,$n)}}finally{$file.Dispose();$stream.Dispose();$cancel.Dispose()}
            return
        }
        throw '更新下载重定向次数超限。'
    }finally{if($response){$response.Dispose()};$client.Dispose();$handler.Dispose()}
}

function Expand-NodeUpdate([string]$Archive,[string]$Destination,[string]$Stem) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip=[IO.Compression.ZipFile]::OpenRead($Archive);$seen=@{};$total=0L
    try{
        if($zip.Entries.Count -gt 12000){throw '更新文件数量超限。'}
        foreach($entry in $zip.Entries){
            $name=$entry.FullName
            if(!$name.StartsWith($Stem+'/',[StringComparison]::Ordinal) -or $name.Contains('\') -or $name.Contains(':') -or $name -match '(^|/)\.\.?(/|$)' -or $seen.ContainsKey($name) -or $name.EndsWith('/') -or (($entry.ExternalAttributes -shr 16) -band 61440) -eq 40960){throw '更新包含重复、链接或非法路径。'}
            $seen[$name]=$true;$total+=$entry.Length;if($total -gt 536870912){throw '解压大小超限。'}
            $path=[IO.Path]::GetFullPath((Join-Path $Destination $name))
            if(!$path.StartsWith($Destination+'\',[StringComparison]::OrdinalIgnoreCase)){throw '更新解压路径越界。'}
            [void][IO.Directory]::CreateDirectory((Split-Path -Parent $path))
            $inputStream=$entry.Open();$output=[IO.File]::Open($path,[IO.FileMode]::CreateNew)
            try{$buffer=[byte[]]::new(65536);$written=0L;while(($n=$inputStream.Read($buffer,0,$buffer.Length)) -gt 0){$written+=$n;if($written -gt $entry.Length){throw '解压数据超过声明大小。'};$output.Write($buffer,0,$n)};if($written -ne $entry.Length){throw '解压文件不完整。'}}finally{$inputStream.Dispose();$output.Dispose()}
        }
    }finally{$zip.Dispose()}
}

function Invoke-NodeUpdate([string]$Root,[switch]$FirstInstall) {
    . (Join-Path $PSScriptRoot 'node-install.ps1')
    if(!$FirstInstall){
        Assert-NodeDirectory $Root
        if(!(Test-Path -LiteralPath (Join-Path $Root 'node.local.toml'))){throw '未找到原节点配置；请从已安装节点目录执行 webts-node.cmd update。'}
    }
    $stage=Join-Path $env:TEMP ('WebTS-Node-Update-'+[Guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($stage);Set-NodeAcl $stage -Directory
    Write-Host '正在检查 PTPHAP/WebTS 的最新正式发布；尚未停止节点。'
    $metadata=Join-Path $stage 'release.json';Get-OfficialNodeFile 'https://api.github.com/repos/PTPHAP/WebTS/releases/latest' $metadata 2097152
    $release=[IO.File]::ReadAllText($metadata)|ConvertFrom-Json
    if($release.draft -or $release.prerelease -or $release.tag_name -notmatch '^v(\d+\.\d+\.\d+)$'){throw '官方发布版本无效。'}
    $version=$Matches[1]
    $assets=@($release.assets|Where-Object {$_.name -match ('^webts-node-'+[regex]::Escape($version)+'-preview\.\d+-windows-x64\.zip$')})
    if($assets.Count -ne 1){throw '此发布尚未提供唯一的 Windows 节点包；保留当前节点，请稍后再试。'}
    $name=[string]$assets[0].name;$stem=$name.Substring(0,$name.Length-4)
    if(@($release.assets|Where-Object {$_.name -eq ($name+'.sha256')}).Count -ne 1){throw '发布缺少节点校验摘要。'}
    $base='https://github.com/PTPHAP/WebTS/releases/download/'+$release.tag_name+'/'
    Get-OfficialNodeFile ($base+$name+'.sha256') (Join-Path $stage 'checksum') 1024
    $checksum=[IO.File]::ReadAllText((Join-Path $stage 'checksum'))
    if($checksum -notmatch ('\A([a-f0-9]{64})  '+[regex]::Escape($name)+'\r?\n?\z')){throw '官方包摘要格式不符。'}
    $expected=$Matches[1];$archive=Join-Path $stage $name
    Get-OfficialNodeFile ($base+$name) $archive 268435456
    if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected){throw '更新包摘要不匹配，当前节点未停止。'}
    $destination=Join-Path $stage 'unpacked';[void][IO.Directory]::CreateDirectory($destination)
    Expand-NodeUpdate $archive $destination $stem
    $source=Join-Path $destination $stem;Assert-NodeDirectory $source;Assert-NodeManifest $source
    $info=[IO.File]::ReadAllText((Join-Path $source 'BUILD-INFO.json'))|ConvertFrom-Json
    if($info.platform -ne 'windows-x64' -or $info.version -ne ($version+'-node-preview.'+($name -replace '^.*-preview\.(\d+)-windows-x64.zip$','$1')) -or $info.base_commit -notmatch '^[a-f0-9]{40}$' -or $info.working_tree -ne 'committed source; protocol patch is included in repository'){throw '更新包构建来源与平台不符。'}
    Write-Host ('已校验：'+$name+'；源码：'+$info.base_commit)
    Write-Host '节点仍为预览组件。更新会短暂中断此节点的网页通话，保留配置及密钥，不操作 TeamSpeak。'
    if((Read-NodeValue '继续进入升级向导？（Y/N）' 'N') -notmatch '^(y|yes|是)$'){Write-Host ('已取消。下载暂存于 '+$stage);return}
    # Use the newly verified installer, with the original directory fixed explicitly.
    . (Join-Path $source 'scripts\node-install.ps1')
    if($FirstInstall){
        $defaultRoot=Find-InstalledNodeRoot (Join-Path $env:SystemDrive 'WebTS-Node')
        $Root=Get-NodeFullPath (Read-NodeValue '安装目录（已有节点请使用原目录）' $defaultRoot) $source
    }
    Invoke-NodeInstall $Root $source
    Write-Host ('升级完成。下载缓存：'+$stage+'；旧配置备份在原节点 backups 中。')
}

if($MyInvocation.InvocationName -ne '.'){
    $mutex=$null;$locked=$false
    try{
        $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
        if(!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
            $child=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -File "'+$PSCommandPath+'"'+$(if($Bootstrap){' -Bootstrap'}else{''})) -Verb RunAs -Wait -PassThru
            exit $child.ExitCode
        }
        foreach($module in @('Microsoft.PowerShell.Security','Microsoft.PowerShell.Utility','ScheduledTasks','NetSecurity','NetTCPIP','CimCmdlets')){Import-Module (Join-Path $PSHOME ('Modules\'+$module+'\'+$module+'.psd1')) -ErrorAction Stop}
        $mutex=[Threading.Mutex]::new($false,'Global\WebTS-Node-Install');try{$locked=$mutex.WaitOne(0)}catch [Threading.AbandonedMutexException]{$locked=$true}
        if(!$locked){throw '已有安装或更新向导运行，请先完成原窗口操作。'}
        Invoke-NodeUpdate (Split-Path -Parent $PSScriptRoot) -FirstInstall:$Bootstrap
    }catch{Write-Host ('节点更新未完成：'+$_.Exception.Message) -ForegroundColor Red;Write-Host '没有操作TeamSpeak。若进入安装阶段，请保留原目录/backups；下载或校验阶段失败则当前节点继续运行。';exit 1}
    finally{if($locked){$mutex.ReleaseMutex()};if($mutex){$mutex.Dispose()}}
}
