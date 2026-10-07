param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $projectRoot
if (-not (Test-Path -LiteralPath config.local.toml)) { Copy-Item -LiteralPath config.example.toml -Destination config.local.toml }
if (-not $SkipBuild) {
    Push-Location web
    try { & npm.cmd ci --cache ../.cache/npm --no-audit --no-fund; if ($LASTEXITCODE) { throw '前端依赖安装失败' }; & npm.cmd run build; if ($LASTEXITCODE) { throw '前端构建失败' } } finally { Pop-Location }
    & "$PSScriptRoot/with-tools.ps1" cargo build --package web-ts --locked
    if ($LASTEXITCODE) { throw '网关构建失败' }
}
if (-not (Test-Path -LiteralPath secrets/master.key)) {
    if (Test-Path -LiteralPath data/web-ts.db) { throw '数据库已存在但密钥缺失：请恢复原密钥，不能创建替代密钥' }
    New-Item -ItemType Directory -Force secrets | Out-Null
    & ./.cache/target/debug/web-ts.exe init-key secrets/master.key
    if ($LASTEXITCODE) { throw '密钥创建失败' }
}
Write-Host '本地界面：http://localhost:8080；注册和通话需要填写SMTP与TS服务器配置。'
& ./.cache/target/debug/web-ts.exe serve config.local.toml
