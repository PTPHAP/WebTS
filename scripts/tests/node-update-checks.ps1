param([Parameter(Mandatory=$true)][string]$PackageRoot,[Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
. (Join-Path $PackageRoot 'scripts\node-update.ps1')
function Assert($condition,$name){if(!$condition){throw ('FAIL: '+$name)}}
function Refuses([scriptblock]$action){$failed=$false;try{&$action}catch{$failed=$true};Assert $failed 'unsafe update refused'}
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression
$zip=Join-Path $Scratch 'update-paths.zip'
foreach($name in @('../escape','node/../escape','other/file','node/file','node/link')){
    $archive=[IO.Compression.ZipFile]::Open($zip,[IO.Compression.ZipArchiveMode]::Create)
    try{
        $entry=$archive.CreateEntry($name)
        if($name -eq 'node/link'){$entry.ExternalAttributes=([int]40960 -shl 16)}
        if($name -eq 'node/file'){[void]$archive.CreateEntry($name)}
    }finally{$archive.Dispose()}
    $out=Join-Path $Scratch ([Guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($out)
    Refuses {Expand-NodeUpdate $zip $out 'node'}
    Remove-Item -LiteralPath $zip
}
$archive=[IO.Compression.ZipFile]::Open($zip,[IO.Compression.ZipArchiveMode]::Create)
try{$entry=$archive.CreateEntry('node/scripts/check.txt');$writer=[IO.StreamWriter]::new($entry.Open());try{$writer.Write('verified fixture')}finally{$writer.Dispose()}}finally{$archive.Dispose()}
$out=Join-Path $Scratch 'valid-update';[void][IO.Directory]::CreateDirectory($out)
Expand-NodeUpdate $zip $out 'node'
Assert ([IO.File]::ReadAllText((Join-Path $out 'node/scripts/check.txt')) -eq 'verified fixture') 'valid payload extracted'
foreach($url in @('http://github.com/x','https://github.com.evil.invalid/x','https://user:secret@github.com/x','https://github.com:444/x')){Refuses {Get-OfficialNodeFile $url (Join-Path $Scratch 'must-not-write') 100}}
Assert (!(Test-Path -LiteralPath (Join-Path $Scratch 'must-not-write'))) 'network rejection creates no output'
# A first installation has no local config. Exercise download/manifest validation and
# handoff to the newly verified installer without touching tasks, processes or firewall.
$fixtureName='webts-node-1.2.3-preview.5-windows-x64.zip'
$fixtureStem=$fixtureName.Substring(0,$fixtureName.Length-4)
$fixtureZip=Join-Path $Scratch $fixtureName
$fixtureTarget=Join-Path $Scratch 'first-install-target'
$helper=@'
function Find-InstalledNodeRoot($default){return $script:fixtureTarget}
function Read-NodeValue($label,$default){return $default}
function Invoke-NodeInstall($root,$source){$script:bootstrapCalled=@($root,$source)}
'@
$files=@{
    'web-ts.exe'='synthetic fixture';'scripts/node.ps1'='fixture';'node.example.toml'='fixture'
    'webts-node.cmd'='fixture';'开始安装.cmd'='fixture';'scripts/node-install.ps1'=$helper
    'BUILD-INFO.json'='{"platform":"windows-x64","version":"1.2.3-node-preview.5","base_commit":"1111111111111111111111111111111111111111","working_tree":"committed source; protocol patch is included in repository"}'
}
$sha=[Security.Cryptography.SHA256]::Create()
try{
    $manifest=''
    foreach($name in @($files.Keys)){
        $hash=[BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($files[$name]))).Replace('-','').ToLowerInvariant()
        $manifest+=$hash+'  '+$name+"`n"
    }
    $files['SHA256SUMS']=$manifest
}finally{$sha.Dispose()}
$archive=[IO.Compression.ZipFile]::Open($fixtureZip,[IO.Compression.ZipArchiveMode]::Create)
try{
    foreach($name in $files.Keys){
        $entry=$archive.CreateEntry($fixtureStem+'/'+$name);$stream=$entry.Open()
        try{$bytes=[Text.Encoding]::UTF8.GetBytes($files[$name]);$stream.Write($bytes,0,$bytes.Length)}finally{$stream.Dispose()}
    }
}finally{$archive.Dispose()}
$fixtureHash=(Get-FileHash -LiteralPath $fixtureZip -Algorithm SHA256).Hash.ToLowerInvariant()
function Get-OfficialNodeFile($url,$path,$limit){
    if($url.EndsWith('/latest')){[IO.File]::WriteAllText($path,'{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[{"name":"'+$fixtureName+'"},{"name":"'+$fixtureName+'.sha256"}]}')}
    elseif($url.EndsWith('.sha256')){[IO.File]::WriteAllText($path,$fixtureHash+'  '+$fixtureName+"`n")}
    else{[IO.File]::Copy($fixtureZip,$path)}
}
function Read-Host($prompt){return 'Y'}
$oldTemp=$env:TEMP
try{
    $env:TEMP=Join-Path $Scratch 'bootstrap-temp';[void][IO.Directory]::CreateDirectory($env:TEMP)
    Invoke-NodeUpdate $fixtureTarget -FirstInstall
    Assert ($script:bootstrapCalled[0] -eq $fixtureTarget) 'bootstrap reaches verified installer without old config'
    Assert (!(Test-Path -LiteralPath $fixtureTarget)) 'fixture handoff made no installation changes'
}finally{$env:TEMP=$oldTemp}
Write-Output 'UPDATE PASS: Windows 5.1 archive traversal, duplicate/link refusal and HTTPS host validation. No live downloads or OS changes.'
