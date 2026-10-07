$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
$env:CARGO_HOME = Join-Path $taskRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $taskRoot '.tools\rustup'
$env:TEMP = Join-Path $taskRoot '.cache\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Path $env:CARGO_HOME,$env:RUSTUP_HOME,$env:TEMP -Force | Out-Null
if (!(Test-Path -LiteralPath '.tools\cargo\bin\rustup.exe')) {
    curl.exe --fail --location --connect-timeout 20 --max-time 180 --output .tools/rustup-init.exe https://win.rustup.rs/x86_64
    if ($LASTEXITCODE -ne 0) { throw 'Rust installer download failed' }
    & .\.tools\rustup-init.exe -y --no-modify-path --profile minimal --default-toolchain 1.99.0-x86_64-pc-windows-gnu
    if ($LASTEXITCODE -ne 0) { throw 'Rust installation failed' }
}
if (!(Test-Path -LiteralPath '.tools\w64devkit\bin\gcc.exe')) {
    $archive = Join-Path $taskRoot '.tools\w64devkit.7z.exe'
    curl.exe --fail --location --connect-timeout 20 --max-time 600 --output $archive https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe
    if ($LASTEXITCODE -ne 0) { throw 'Compiler download failed' }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '18d0a4c71a166f8401ab6305781bec5882b40b5e06ba9807c61cb5f3b3c6325e') { throw 'Compiler archive checksum mismatch' }
    & $archive '-y' ('-o' + (Join-Path $taskRoot '.tools'))
    if ($LASTEXITCODE -ne 0) { throw 'Compiler extraction failed' }
}
& .\scripts\with-tools.ps1 rustc --version
if ($LASTEXITCODE -ne 0) { throw 'Toolchain verification failed' }
