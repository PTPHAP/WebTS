param([Parameter(Mandatory=$true)][string]$ProjectRoot,[Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Assert($value,$name){if(!$value){throw ('FAIL: '+$name)}}
# Execute the documented command with a harmless BOM-prefixed response, never the installer.
function Invoke-RestMethod($Uri){return ([char]0xFEFF)+'$script:decodedBootstrapRan=$true'}
$doc=[IO.File]::ReadAllText((Join-Path $ProjectRoot 'docs\NODE-INSTALL.md'),[Text.Encoding]::UTF8)
$command=[regex]::Match($doc,'(?s)```powershell\r?\n(.*?)\r?\n```').Groups[1].Value
Assert ($command.Length -gt 0) 'documented Windows command exists'
Invoke-Expression $command
Assert $script:decodedBootstrapRan 'decoded BOM is removed before evaluation'
# Evaluate the exact bootstrap command builder and launch a harmless PS5.1 child.
$stage=Join-Path $Scratch "bootstrap's path"
[void][IO.Directory]::CreateDirectory((Join-Path $stage 'scripts'))
$entry=Join-Path $stage 'scripts\node-update.ps1'
[IO.File]::WriteAllText($entry,'param([switch]$Bootstrap); Write-Output ([Net.ServicePointManager]::SecurityProtocol.ToString()+";bootstrap="+$Bootstrap)',[Text.UTF8Encoding]::new($true))
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $ProjectRoot 'install-node.ps1'),[ref]$tokens,[ref]$errors)
Assert ($errors.Count -eq 0) 'bootstrap PS5.1 syntax'
foreach($name in @('childEntry','childCommand','encodedCommand')){
    $node=$ast.Find({param($a) $a -is [Management.Automation.Language.AssignmentStatementAst] -and $a.Left.VariablePath.UserPath -eq $name},$true)
    Assert ($null -ne $node) ('bootstrap command assignment '+$name)
    Invoke-Expression $node.Extent.Text
}
$result=& (Join-Path $PSHOME 'powershell.exe') -NoProfile -EncodedCommand $encodedCommand
Assert ($LASTEXITCODE -eq 0 -and $result -eq 'Tls12;bootstrap=True') 'child TLS policy, bootstrap argument and quoted path'
Write-Output 'BOOTSTRAP PASS: real PS5.1 BOM evaluation and TLS child handoff; no installation or network.'
