$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
# Install wasm-bindgen 0.2.129 locally or pass its full executable path.
$bindingTool = if ($env:WEBTS_WASM_BINDGEN) { $env:WEBTS_WASM_BINDGEN } else { Join-Path $taskRoot '.tools\wasm-bindgen\wasm-bindgen.exe' }
if (!(Test-Path -LiteralPath $bindingTool)) { throw 'Install wasm-bindgen 0.2.129, then set WEBTS_WASM_BINDGEN to its executable path.' }
if ((& $bindingTool --version) -ne 'wasm-bindgen 0.2.129') { throw 'Expected wasm-bindgen 0.2.129' }
& "$PSScriptRoot\with-tools.ps1" -Command @('rustup','target','add','wasm32-unknown-unknown')
if ($LASTEXITCODE -ne 0) { throw 'WASM target install failed' }
& "$PSScriptRoot\with-tools.ps1" -Command @('cargo','build','--package','webts-crypto','--target','wasm32-unknown-unknown','--release','--locked')
if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
& $bindingTool "$taskRoot\.cache\target\wasm32-unknown-unknown\release\webts_crypto.wasm" --target web --out-dir "$taskRoot\web\public\crypto"
if ($LASTEXITCODE -ne 0) { throw 'WASM binding generation failed' }
