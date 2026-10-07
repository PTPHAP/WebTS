$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
git submodule update --init --recursive
if ($LASTEXITCODE -ne 0) { throw 'Submodule initialization failed' }
git -C vendor/tsclientlib apply --reverse --check ../../patches/raw-audio.patch 2>$null
if ($LASTEXITCODE -eq 0) { Write-Output 'raw-audio patch already applied'; exit 0 }
git -C vendor/tsclientlib apply --check ../../patches/raw-audio.patch
if ($LASTEXITCODE -ne 0) { throw 'Vendor patch conflict; preserve local changes and inspect' }
git -C vendor/tsclientlib apply ../../patches/raw-audio.patch
if ($LASTEXITCODE -ne 0) { throw 'Vendor patch failed' }
