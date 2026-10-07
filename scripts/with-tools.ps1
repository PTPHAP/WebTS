param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Command)
# PowerShell consumes a bare '--'. Preserve it with -Command @('cargo', ...,
# '--', ...), or invoke the built executable directly as documented.
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
$env:CARGO_HOME = Join-Path $taskRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $taskRoot '.tools\rustup'
$env:CARGO_TARGET_DIR = Join-Path $taskRoot '.cache\target'
$env:TEMP = Join-Path $taskRoot '.cache\tmp'
$env:TMP = $env:TEMP
$env:npm_config_cache = Join-Path $taskRoot '.cache\npm'
$env:RUSTUP_TOOLCHAIN = if (Test-Path -LiteralPath (Join-Path $env:RUSTUP_HOME 'toolchains\1.99.0-x86_64-pc-windows-gnu')) { '1.99.0-x86_64-pc-windows-gnu' } else { 'stable-x86_64-pc-windows-gnu' }
$env:CARGO_HTTP_MULTIPLEXING = 'false'
$env:CARGO_HTTP_TIMEOUT = '60'
$env:PATH = (Join-Path $taskRoot '.tools\cargo\bin') + ';' + (Join-Path $taskRoot '.tools\w64devkit\bin') + ';' + $env:PATH
New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
$version = & rustc --version
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^rustc 1\.99\.0 ') { throw 'Expected project-local Rust 1.99.0; run bootstrap-rust.ps1 or install the pinned toolchain locally' }
$sysroot = & rustc --print sysroot
$lld = Join-Path $taskRoot '.tools\ld.lld.exe'
if (!(Test-Path -LiteralPath $lld)) {
    Copy-Item -LiteralPath (Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe') -Destination $lld
}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $taskRoot 'scripts\linker.cmd'
# LLVM handles Unicode paths in response files; GNU ld interprets them through
# the Windows ANSI code page. Encoded flags preserve paths containing spaces.
$env:CARGO_ENCODED_RUSTFLAGS = @('-Clinker-flavor=ld', '-Clink-self-contained=yes', ('-Lnative=' + (Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\lib\self-contained'))) -join [char]31
if ($Command.Count -gt 0) {
    $tool = $Command[0]
    [string[]]$toolArguments = if ($Command.Count -gt 1) { $Command[1..($Command.Count-1)] } else { @() }
    & $tool @toolArguments
    exit $LASTEXITCODE
}
