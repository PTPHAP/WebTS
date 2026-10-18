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
Write-Output 'UPDATE PASS: Windows 5.1 archive traversal, duplicate/link refusal and HTTPS host validation. No live downloads or OS changes.'
